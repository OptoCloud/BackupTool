#![allow(dead_code)] // sampling utility reserved for future large-file dedup pre-check

use sha2::{Digest, Sha256};
use std::io::{self, Read, Seek, SeekFrom};

pub const SAMPLE_PCT: f64 = 10.0;
pub const MIN_SIZE_BYTES: u64 = 256 * 1024; // 256 KiB
pub const MAX_BUDGET_BYTES: u64 = 32 * 1024 * 1024; // 32 MiB
pub const MAX_WINDOWS: u64 = 1024;

#[derive(Debug, Clone, Copy)]
struct SamplePlan {
    pub window_bytes: usize,
    pub windows: u64,
    pub stride_bytes: u64,
}

fn window_bytes_for_size(size: u64) -> usize {
    const MI: u64 = 1024 * 1024;
    match size {
        s if s < 64 * MI => 16 * 1024,  // 16 KiB
        s if s < 256 * MI => 32 * 1024, // 32 KiB
        s if s < 512 * MI => 48 * 1024, // 48 KiB
        _ => 64 * 1024,                 // 64 KiB
    }
}

fn compute_budget(size: u64) -> u64 {
    let pct = SAMPLE_PCT.clamp(0.0, 100.0);
    let mut budget = ((size as f64) * (pct / 100.0)).round() as u64;
    if budget > MAX_BUDGET_BYTES {
        budget = MAX_BUDGET_BYTES;
    }
    if budget > size {
        budget = size;
    }
    budget
}

/// Build a plan that **guarantees** every (non-shortened) window fits:
/// `(windows - 1) * stride + window_bytes <= size`.
fn make_sample_plan(size: u64) -> SamplePlan {
    let budget = compute_budget(size);
    if budget == 0 {
        return SamplePlan {
            window_bytes: 1,
            windows: 0,
            stride_bytes: 1,
        };
    }

    let window_bytes = window_bytes_for_size(size) as u64;

    // Initial windows from budget (ceil), at least 1.
    let mut windows = budget.div_ceil(window_bytes).max(1);

    // Enforce maximum window cap.
    if windows > MAX_WINDOWS {
        windows = MAX_WINDOWS;
    }

    // Aim for stride >= 2 * window_bytes when feasible.
    if windows > 0 {
        let min_stride = 2 * window_bytes;

        if size < min_stride {
            windows = 1;
        } else {
            let max_by_stride2 = (size / min_stride).max(1);
            if windows > max_by_stride2 {
                windows = max_by_stride2;
            }
        }
    }

    // Compute stride and enforce the **fit guarantee**.
    let mut stride = (size / windows).max(1);
    while windows > 0 {
        let last_start = (windows - 1).saturating_mul(stride);
        if last_start.saturating_add(window_bytes) <= size {
            break;
        }
        windows -= 1;
        if windows == 0 {
            break;
        }
        stride = (size / windows).max(1);
    }

    SamplePlan {
        window_bytes: window_bytes as usize,
        windows,
        stride_bytes: stride,
    }
}

/// Fingerprint by hashing evenly spaced windows, trusting the plan to fit.
pub fn quick_fingerprint_stream<R: Read + Seek>(
    reader: &mut R,
    size: u64,
) -> io::Result<Option<[u8; 32]>> {
    if size < MIN_SIZE_BYTES {
        return Ok(None);
    }

    let plan = make_sample_plan(size);
    if plan.windows == 0 {
        return Ok(None);
    }

    let budget = compute_budget(size);
    let wb = plan.window_bytes as u64;
    let last_rem = (budget % wb) as usize;

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; plan.window_bytes];

    for i in 0..plan.windows {
        let start = i.saturating_mul(plan.stride_bytes);
        let is_last = i + 1 == plan.windows;
        let want = if is_last && last_rem > 0 {
            last_rem
        } else {
            plan.window_bytes
        };

        debug_assert!(start < size, "plan produced a start >= size");
        debug_assert!(
            size - start >= want as u64,
            "plan says window fits, but want={} at start={} exceeds size={}",
            want,
            start,
            size
        );

        reader.seek(SeekFrom::Start(start))?;
        reader.read_exact(&mut buf[..want])?;
        hasher.update(&buf[..want]);
    }

    Ok(Some(hasher.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_budget_caps() {
        let size = 1_073_741_824u64; // 1 GiB
        assert_eq!(compute_budget(size), MAX_BUDGET_BYTES);
    }

    #[test]
    fn test_plan_fit_guarantee() {
        let size = 512 * 1024 * 1024; // 512 MiB
        let plan = make_sample_plan(size);
        assert!(plan.windows > 0);
        assert!(plan.windows <= MAX_WINDOWS);
        let last_start = (plan.windows - 1) * plan.stride_bytes;
        assert!(last_start + plan.window_bytes as u64 <= size);
    }

    #[test]
    fn test_small_returns_none() {
        let mut cur = Cursor::new(vec![0u8; (MIN_SIZE_BYTES - 1) as usize]);
        let fp = quick_fingerprint_stream(&mut cur, MIN_SIZE_BYTES - 1).unwrap();
        assert!(fp.is_none());
    }

    #[test]
    fn test_hash_some_bytes() {
        let size = 8 * 1024 * 1024; // 8 MiB
        let mut cur = Cursor::new(vec![42u8; size as usize]);
        let fp = quick_fingerprint_stream(&mut cur, size).unwrap();
        assert!(fp.is_some());

        let mut cur2 = Cursor::new(vec![42u8; size as usize]);
        let fp2 = quick_fingerprint_stream(&mut cur2, size).unwrap();
        assert_eq!(fp, fp2);
    }
}

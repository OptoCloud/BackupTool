use rayon::prelude::*;
use std::io::{self, Read};

pub struct EntropyStream {
    counts: [u64; 256],
    total: u64,
}

impl EntropyStream {
    pub fn new() -> Self {
        Self {
            counts: [0; 256],
            total: 0,
        }
    }

    /// Feed via any `Read` (small files)
    #[allow(dead_code)]
    pub fn feed<R: Read>(&mut self, reader: &mut R) -> io::Result<()> {
        let mut buf = vec![0u8; 8 * 1024 * 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            self.feed_slice(&buf[..n]);
        }
        Ok(())
    }

    /// Feed from a memory slice (mmap or in-memory buffer)
    pub fn feed_slice(&mut self, data: &[u8]) {
        self.total += data.len() as u64;
        let partial: Vec<[u64; 256]> = data.par_chunks(1024 * 1024).map(count_chunk).collect();
        for hist in partial {
            for (i, &c) in hist.iter().enumerate() {
                self.counts[i] += c;
            }
        }
    }

    /// Compute Shannon entropy from the accumulated counts.
    pub fn entropy(&self) -> f64 {
        let tot = self.total as f64;
        let sum: f64 = self
            .counts
            .iter()
            .filter(|&&c| c != 0)
            .map(|&c| {
                let p = (c as f64) / tot;
                -p * p.log2()
            })
            .sum();

        sum * 12.5
    }
}

/// Count a single chunk; LLVM will auto-vectorize this.
#[inline]
fn count_chunk(chunk: &[u8]) -> [u64; 256] {
    let mut local = [0u64; 256];

    // If you’re on x86 with AVX2, LLVM will use vector loads here
    for &b in chunk {
        local[b as usize] += 1;
    }

    local
}

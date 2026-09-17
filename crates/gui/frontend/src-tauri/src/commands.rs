use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;

use bpfs_core::constants::DATA_BLOCK_SIZE;
use bpfs_core::progress::{Monitor, Phase, Progress};
use bpfs_core::types::enums::CompressionType;
use bpfs_pack::pack::{
    pack_into_file, ExistingBlob, ExistingBlobLookup, NoExistingBlobs, PackOptions,
};
use bpfs_pack::policy::compression::DefaultCompressionPolicy;
use bpfs_read::iter::{current_files, extract_files};
use bpfs_read::verify::{verify_blob_hashes, verify_integrity};
use bpfs_read::{read_archive_with_monitor, Archive};

type ArchiveFile = Archive<BufReader<File>>;

const CANCELLED: &str = "cancelled";

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn map_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = n as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

fn open_archive(path: &str, monitor: &dyn Monitor) -> Result<ArchiveFile, String> {
    let file = File::open(path).map_err(|e| format!("opening {path}: {e}"))?;
    read_archive_with_monitor(BufReader::new(file), monitor).map_err(map_err)
}

// ---------------------------------------------------------------------------
// Task plumbing: progress/log events, throttling and cancellation.

#[derive(Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TaskEvent {
    Progress {
        phase: &'static str,
        done: u64,
        /// 0 when the total is unknown.
        total: u64,
        item: Option<String>,
        item_done: u64,
        item_total: u64,
    },
    Log {
        message: String,
    },
}

/// The cancel flag of the task currently running, if any.
#[derive(Default)]
pub struct TaskState {
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

fn phase_id(phase: Phase) -> &'static str {
    match phase {
        Phase::Opening => "opening",
        Phase::Scanning => "scanning",
        Phase::Hashing => "hashing",
        Phase::Packing => "packing",
        Phase::Verifying => "verifying",
        Phase::VerifyingContent => "verifyingContent",
        Phase::Extracting => "extracting",
    }
}

fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Opening => "Opening archive",
        Phase::Scanning => "Scanning files",
        Phase::Hashing => "Checking file contents",
        Phase::Packing => "Compressing and writing",
        Phase::Verifying => "Verifying archive data",
        Phase::VerifyingContent => "Checking stored file contents",
        Phase::Extracting => "Restoring files",
    }
}

struct PhaseRun {
    phase: Phase,
    started: Instant,
    done: u64,
    total: u64,
}

struct MonitorState {
    last_emit: Option<Instant>,
    current: Option<PhaseRun>,
}

/// Forwards progress and log messages to the frontend and exposes the
/// task's cancel flag to the library.
struct ChannelMonitor {
    channel: Channel<TaskEvent>,
    cancel: Arc<AtomicBool>,
    state: Mutex<MonitorState>,
}

impl ChannelMonitor {
    fn send_log(&self, message: String) {
        let _ = self.channel.send(TaskEvent::Log { message });
    }

    /// Logs how the phase that just ended went.
    fn log_phase_end(&self, run: &PhaseRun) {
        let secs = run.started.elapsed().as_secs_f64();
        let message = match run.phase {
            Phase::Scanning => format!("Scanned {} items in {secs:.1} s", run.done),
            Phase::Opening => format!("Opened archive in {secs:.1} s"),
            _ => {
                let rate = if secs > 0.0 {
                    format!(", {}/s", fmt_bytes((run.done as f64 / secs) as u64))
                } else {
                    String::new()
                };
                format!(
                    "{} finished: {} in {secs:.1} s{rate}",
                    phase_label(run.phase),
                    fmt_bytes(run.done)
                )
            }
        };
        self.send_log(message);
    }

    /// Logs the end of the last phase; call once the task succeeds.
    fn finish(&self) {
        let run = self.state.lock().unwrap().current.take();
        if let Some(run) = run {
            self.log_phase_end(&run);
        }
    }
}

impl Monitor for ChannelMonitor {
    fn progress(&self, p: Progress<'_>) {
        // Skip rather than wait if another thread is reporting right now.
        let Ok(mut state) = self.state.try_lock() else {
            return;
        };
        let now = Instant::now();
        let phase_changed = state
            .current
            .as_ref()
            .is_none_or(|run| run.phase != p.phase);
        if phase_changed {
            if let Some(prev) = state.current.take() {
                self.log_phase_end(&prev);
            }
            self.send_log(format!("{}…", phase_label(p.phase)));
            state.current = Some(PhaseRun {
                phase: p.phase,
                started: now,
                done: 0,
                total: 0,
            });
        }
        if let Some(run) = state.current.as_mut() {
            run.done = p.done;
            run.total = p.total;
        }

        let finished = p.total > 0 && p.done >= p.total;
        let due = state
            .last_emit
            .is_none_or(|at| now - at >= PROGRESS_INTERVAL);
        if !(phase_changed || finished || due) {
            return;
        }
        state.last_emit = Some(now);
        let _ = self.channel.send(TaskEvent::Progress {
            phase: phase_id(p.phase),
            done: p.done,
            total: p.total,
            item: p.item.map(|i| i.path.to_string_lossy().into_owned()),
            item_done: p.item.map_or(0, |i| i.done),
            item_total: p.item.map_or(0, |i| i.total),
        });
    }

    fn log(&self, message: &str) {
        self.send_log(message.to_string());
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Runs `work` on a blocking thread as the single active task. Returns
/// `Err("cancelled")` if it failed after cancellation was requested.
async fn run_task<T, F>(
    state: &TaskState,
    channel: Channel<TaskEvent>,
    work: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&ChannelMonitor) -> Result<T, String> + Send + 'static,
{
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut slot = state.cancel.lock().unwrap();
        if slot.is_some() {
            return Err("another operation is already running".into());
        }
        *slot = Some(cancel.clone());
    }

    let monitor = ChannelMonitor {
        channel,
        cancel: cancel.clone(),
        state: Mutex::new(MonitorState {
            last_emit: None,
            current: None,
        }),
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = work(&monitor);
        if result.is_ok() {
            monitor.finish();
        }
        result
    })
    .await
    .map_err(map_err)
    .and_then(|r| r);

    *state.cancel.lock().unwrap() = None;
    match result {
        Err(_) if cancel.load(Ordering::Relaxed) => Err(CANCELLED.into()),
        other => other,
    }
}

#[tauri::command]
pub fn cancel_task(state: State<'_, TaskState>) {
    if let Some(flag) = state.cancel.lock().unwrap().as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Back up

struct BlobIndex(HashMap<[u8; 32], ExistingBlob>);
impl ExistingBlobLookup for BlobIndex {
    fn find(&self, hash: &[u8; 32]) -> Option<ExistingBlob> {
        self.0.get(hash).copied()
    }
}

#[derive(Serialize)]
pub struct PackSummaryDto {
    pub generation_idx: u32,
    pub file_count: usize,
    pub dir_count: usize,
    pub blob_count: usize,
    pub new_blob_bytes: u64,
    pub bytes_written: usize,
    pub integrity_hash: String,
}

#[tauri::command]
pub async fn pack_archive(
    src: String,
    out: String,
    compression: String,
    on_event: Channel<TaskEvent>,
    state: State<'_, TaskState>,
) -> Result<PackSummaryDto, String> {
    let compression = match compression.as_str() {
        "zstd" => CompressionType::Zstd,
        "brotli" => CompressionType::Brotli,
        "none" => CompressionType::None,
        other => return Err(format!("unknown compression {other}")),
    };
    run_task(&state, on_event, move |monitor| {
        pack(&src, &out, compression, monitor)
    })
    .await
}

fn pack(
    src: &str,
    out: &str,
    compression: CompressionType,
    monitor: &ChannelMonitor,
) -> Result<PackSummaryDto, String> {
    let out_path = Path::new(out);
    let appending = out_path.exists();
    monitor.log(&format!(
        "Backing up {src} to {out} ({})",
        if appending {
            "new snapshot"
        } else {
            "new archive"
        }
    ));

    let (generation_idx, previous_integrity_hash, existing_blobs): (
        u32,
        [u8; 32],
        Box<dyn ExistingBlobLookup>,
    ) = if appending {
        let archive = open_archive(out, monitor)?;
        let mut map = HashMap::new();
        for gen in &archive.generations {
            for (i, b) in gen.blobs.iter().enumerate() {
                map.entry(gen.blob_hashes[i]).or_insert(ExistingBlob {
                    generation_idx: b.generation_idx,
                    section_idx: b.section_idx,
                    section_blob_idx: b.section_blob_idx,
                });
            }
        }
        monitor.log(&format!(
            "Archive has {} snapshots and {} stored blobs",
            archive.generations.len(),
            map.len()
        ));
        (
            archive.generations.len() as u32,
            archive.latest().integrity_hash,
            Box::new(BlobIndex(map)),
        )
    } else {
        (0, [0u8; 32], Box::new(NoExistingBlobs))
    };

    let policy = DefaultCompressionPolicy {
        incompressible_entropy: 96.25,
    };
    let summary = pack_into_file(
        out_path,
        Path::new(src),
        appending,
        PackOptions {
            generation_idx,
            previous_integrity_hash,
            policy: &policy,
            compression,
            block_size: DATA_BLOCK_SIZE,
            signing_key: None,
            existing_blobs: existing_blobs.as_ref(),
        },
        monitor,
    )
    .map_err(|e| format!("{e:#}"))?;

    Ok(PackSummaryDto {
        generation_idx,
        file_count: summary.file_count,
        dir_count: summary.dir_count,
        blob_count: summary.blob_count,
        new_blob_bytes: summary.new_blob_bytes,
        bytes_written: summary.bytes_written,
        integrity_hash: to_hex(&summary.integrity_hash),
    })
}

// ---------------------------------------------------------------------------
// Browse, restore, verify

#[derive(Serialize)]
pub struct FileEntryDto {
    pub path: String,
    pub size: u64,
}

#[tauri::command]
pub async fn list_archive(path: String) -> Result<Vec<FileEntryDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let archive = open_archive(&path, &bpfs_core::progress::NoMonitor)?;
        let gen = archive.latest();
        let resolved = current_files(gen).map_err(map_err)?;
        let mut out: Vec<FileEntryDto> = resolved
            .iter()
            .map(|r| FileEntryDto {
                path: r.path.to_string_lossy().into_owned(),
                size: gen.blobs[gen.files[r.file_idx].blob_idx as usize].raw_size,
            })
            .collect();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    })
    .await
    .map_err(map_err)?
}

#[tauri::command]
pub async fn extract_archive(
    path: String,
    dest: String,
    overwrite: bool,
    on_event: Channel<TaskEvent>,
    state: State<'_, TaskState>,
) -> Result<usize, String> {
    run_task(&state, on_event, move |monitor| {
        monitor.log(&format!("Restoring latest snapshot of {path} to {dest}"));
        let archive = open_archive(&path, monitor)?;
        let gen = archive.latest();
        let files = current_files(gen).map_err(map_err)?;
        let summary = extract_files(
            &archive,
            gen,
            &files,
            &PathBuf::from(&dest),
            overwrite,
            monitor,
        )
        .map_err(map_err)?;
        monitor.log(&format!(
            "Restored {} files ({})",
            summary.files,
            fmt_bytes(summary.bytes)
        ));
        Ok(summary.files)
    })
    .await
}

#[derive(Serialize)]
pub struct VerifyResultDto {
    pub generations: usize,
    pub ok: bool,
    pub message: String,
}

#[tauri::command]
pub async fn verify_archive(
    path: String,
    deep: bool,
    on_event: Channel<TaskEvent>,
    state: State<'_, TaskState>,
) -> Result<VerifyResultDto, String> {
    run_task(&state, on_event, move |monitor| {
        monitor.log(&format!(
            "Verifying {path}{}",
            if deep {
                " (including file contents)"
            } else {
                ""
            }
        ));
        let fail = |generations, message: String| {
            if monitor.is_cancelled() {
                return Err(CANCELLED.to_string());
            }
            monitor.log(&format!("Problem found: {message}"));
            Ok(VerifyResultDto {
                generations,
                ok: false,
                message,
            })
        };

        let archive = match open_archive(&path, monitor) {
            Ok(a) => a,
            Err(e) => return fail(0, e),
        };
        let generations = archive.generations.len();
        if let Err(e) = verify_integrity(&archive, monitor) {
            return fail(generations, e.to_string());
        }
        if deep {
            if let Err(e) = verify_blob_hashes(&archive, monitor) {
                return fail(generations, e.to_string());
            }
        }
        Ok(VerifyResultDto {
            generations,
            ok: true,
            message: "OK".to_string(),
        })
    })
    .await
}

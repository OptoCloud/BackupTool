//! Top-level orchestration: scan a directory, dedupe/analyze its files into
//! blobs, and write one BPFS `Generation` record for them.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};

use bpfs_core::progress::{Item, Monitor, NoMonitor, Phase, Progress};
use bpfs_core::types::enums::{CompressionType, SectionKind};
use bpfs_core::types::packed::{BlobEntry, DirectoryEntry, FileEntry, NO_PARENT};

use crate::analyze::blob::{collect_unique_blobs, BlobGroup};
use crate::encode::generation::{
    write_generation_with_progress, GenerationInput, SectionSource, SectionSpec,
};
use crate::encode::header::write_header;
use crate::interner::StringInterner;
use crate::policy::CompressionPolicy;
use crate::scan::collector::FileCollector;
use crate::scan::types::FileEntry as ScannedFile;

/// A blob already known to exist in some earlier generation of the archive
/// being appended to.
#[derive(Clone, Copy, Debug)]
pub struct ExistingBlob {
    pub generation_idx: u32,
    pub section_idx: u32,
    pub section_blob_idx: u32,
}

pub trait ExistingBlobLookup {
    fn find(&self, hash: &[u8; 32]) -> Option<ExistingBlob>;
}

/// No prior generations: every blob is new. Used when creating a fresh archive.
pub struct NoExistingBlobs;
impl ExistingBlobLookup for NoExistingBlobs {
    fn find(&self, _hash: &[u8; 32]) -> Option<ExistingBlob> {
        None
    }
}

pub struct PackOptions<'a> {
    /// Index of the generation being written (0 for a brand-new archive).
    pub generation_idx: u32,
    /// `integrity_hash` of the previous generation, or all-zero for the first.
    pub previous_integrity_hash: [u8; 32],
    pub policy: &'a dyn CompressionPolicy,
    /// Codec for the compressed section: `Zstd`, `Brotli` or `None`.
    pub compression: CompressionType,
    /// Uncompressed size of each data block.
    pub block_size: u32,
    pub signing_key: Option<&'a SigningKey>,
    pub existing_blobs: &'a dyn ExistingBlobLookup,
}

pub struct PackSummary {
    pub integrity_hash: [u8; 32],
    pub file_count: usize,
    pub dir_count: usize,
    pub blob_count: usize,
    pub new_blob_bytes: u64,
    pub bytes_written: usize,
}

fn now_archive_time() -> u64 {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    // 100-nanosecond intervals since UNIX epoch, matching FileTimes.
    dur.as_secs() * 10_000_000 + (dur.subsec_nanos() / 100) as u64
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub(crate) fn fmt_bytes(n: u64) -> String {
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

/// Scans `root` and writes one full `Generation` snapshot of it.
///
/// If `write_archive_header` is true, the BPFS archive header is written
/// first (only correct for a brand-new archive / generation 0).
pub fn pack_directory<W: Write>(
    writer: &mut W,
    root: &Path,
    write_archive_header: bool,
    options: PackOptions,
) -> Result<PackSummary> {
    pack_directory_with_monitor(writer, root, write_archive_header, options, &NoMonitor)
}

/// Like [`pack_directory`], reporting progress and log messages to `monitor`
/// and stopping with an `Interrupted` error once it is cancelled.
pub fn pack_directory_with_monitor<W: Write>(
    writer: &mut W,
    root: &Path,
    write_archive_header: bool,
    options: PackOptions,
    monitor: &dyn Monitor,
) -> Result<PackSummary> {
    if !root.is_dir() {
        anyhow::bail!("pack root must be a directory: {}", root.display());
    }
    if !matches!(
        options.compression,
        CompressionType::None | CompressionType::Brotli | CompressionType::Zstd
    ) {
        anyhow::bail!("unsupported compression {:?}", options.compression);
    }

    if write_archive_header {
        write_header(&mut *writer, 0)?;
    }

    // 1) Scan the filesystem tree.
    let mut collector = FileCollector::new();
    let mut scanned = 0u64;
    let report_scan = |done| {
        monitor.progress(Progress {
            phase: Phase::Scanning,
            done,
            total: 0,
            item: None,
        })
    };
    report_scan(0);
    for step in collector.add_path(root.to_path_buf(), PathBuf::new(), false) {
        step.with_context(|| format!("scanning {}", root.display()))?;
        monitor.checkpoint()?;
        scanned += 1;
        report_scan(scanned);
    }
    let source_bytes: u64 = collector.files.iter().map(|f| f.size_hint).sum();
    monitor.log(&format!(
        "Found {} files in {} folders ({})",
        collector.files.len(),
        collector.directories.len(),
        fmt_bytes(source_bytes)
    ));

    // 2) Dedupe + hash + entropy-analyze file contents into blobs.
    let hashed = AtomicU64::new(0);
    let groups: Vec<BlobGroup> =
        collect_unique_blobs(&collector.files, &|path, file_done, file_total, delta| {
            monitor.checkpoint()?;
            let done = hashed.fetch_add(delta, Ordering::Relaxed) + delta;
            monitor.progress(Progress {
                phase: Phase::Hashing,
                done,
                total: source_bytes,
                item: Some(Item {
                    path,
                    done: file_done,
                    total: file_total,
                }),
            });
            Ok(())
        })
        .context("analyzing files")?;
    monitor.log(&format!(
        "{} unique blobs ({} duplicate files)",
        groups.len(),
        collector.files.len() - groups.len()
    ));

    // file index -> which group (== eventual blob_idx) it belongs to
    let mut file_to_group: HashMap<usize, usize> = HashMap::new();
    for (gi, g) in groups.iter().enumerate() {
        for &fi in &g.indices {
            file_to_group.insert(fi, gi);
        }
    }

    // 3) Decide new-vs-existing and stored-vs-compressed for every blob.
    enum Resolved {
        Existing(ExistingBlob),
        New {
            section: SectionKind,
            sort_ext: String,
            sort_size: u64,
        },
    }

    let mut resolved: Vec<Resolved> = Vec::with_capacity(groups.len());
    let mut new_blob_bytes: u64 = 0;
    let mut section_bytes = [0u64; SectionKind::COUNT];
    let mut reused = 0usize;
    for g in &groups {
        if let Some(existing) = options.existing_blobs.find(&g.info.hash) {
            resolved.push(Resolved::Existing(existing));
            reused += 1;
            continue;
        }
        let ext = g
            .indices
            .first()
            .map(|&fi| extension_of(&collector.files[fi].archive_path))
            .unwrap_or_default();
        let section = if options.compression != CompressionType::None
            && options.policy.should_compress(&ext, g.info.entropy as f64)
        {
            SectionKind::Compressed
        } else {
            SectionKind::Stored
        };
        new_blob_bytes += g.info.size as u64;
        section_bytes[section as usize] += g.info.size as u64;
        resolved.push(Resolved::New {
            section,
            sort_ext: ext,
            sort_size: g.info.size as u64,
        });
    }
    monitor.log(&format!(
        "{} new ({}: {} to compress, {} stored as-is), {} already in archive",
        groups.len() - reused,
        fmt_bytes(new_blob_bytes),
        fmt_bytes(section_bytes[SectionKind::Compressed as usize]),
        fmt_bytes(section_bytes[SectionKind::Stored as usize]),
        reused
    ));

    // 4) Order new blobs within each section for better compression locality.
    let mut new_order: [Vec<usize>; SectionKind::COUNT] = [Vec::new(), Vec::new()];
    for (gi, r) in resolved.iter().enumerate() {
        if let Resolved::New { section, .. } = r {
            new_order[*section as usize].push(gi);
        }
    }
    for bucket in &mut new_order {
        bucket.sort_by_key(|&gi| match &resolved[gi] {
            Resolved::New {
                sort_ext,
                sort_size,
                ..
            } => bpfs_core::sort::sort_key(sort_ext, *sort_size),
            Resolved::Existing(_) => unreachable!(),
        });
    }

    // 5) Record each new blob's position within its section.
    let mut section_blob_idx: HashMap<usize, u32> = HashMap::new();
    for order in &new_order {
        for (pos, &gi) in order.iter().enumerate() {
            section_blob_idx.insert(gi, pos as u32);
        }
    }

    // 6) Interner for directory/file/section names.
    let mut interner = StringInterner::default();
    let stored_name = interner.intern(SectionKind::Stored.name());
    let compressed_name = interner.intern(SectionKind::Compressed.name());

    // 7) Directories, preserving collector order (parents precede children).
    let mut dir_index_of: HashMap<PathBuf, u32> = HashMap::new();
    let mut dirs = Vec::with_capacity(collector.directories.len());
    for (idx, d) in collector.directories.iter().enumerate() {
        let parent_id = d
            .archive_path
            .parent()
            .and_then(|p| dir_index_of.get(p))
            .copied()
            .unwrap_or(NO_PARENT);
        let name = d
            .archive_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        dirs.push(DirectoryEntry {
            parent_id,
            name_stridx: interner.intern(name),
            created_at: d.filetimes.created as u64,
            modified_at: d.filetimes.modified as u64,
        });
        dir_index_of.insert(d.archive_path.clone(), idx as u32);
    }

    // 8) Files.
    let mut files = Vec::with_capacity(collector.files.len());
    for (fi, f) in collector.files.iter().enumerate() {
        let dir_idx = f
            .archive_path
            .parent()
            .and_then(|p| dir_index_of.get(p))
            .copied()
            .with_context(|| {
                format!(
                    "no parent directory recorded for {}",
                    f.archive_path.display()
                )
            })?;
        let name = f
            .archive_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let blob_idx = *file_to_group
            .get(&fi)
            .expect("every scanned file belongs to a blob group") as u32;
        files.push(FileEntry {
            blob_idx,
            dir_idx,
            name_stridx: interner.intern(name),
            created_at: f.filetimes.created as u64,
            modified_at: f.filetimes.modified as u64,
        });
    }

    // 9) Blob table + parallel hash array, in group order.
    let mut blobs = Vec::with_capacity(groups.len());
    let mut blob_hashes = Vec::with_capacity(groups.len());
    for (gi, g) in groups.iter().enumerate() {
        let entry = match &resolved[gi] {
            Resolved::Existing(e) => BlobEntry {
                raw_size: g.info.size as u64,
                generation_idx: e.generation_idx,
                section_idx: e.section_idx,
                section_blob_idx: e.section_blob_idx,
            },
            Resolved::New { section, .. } => BlobEntry {
                raw_size: g.info.size as u64,
                generation_idx: options.generation_idx,
                section_idx: *section as u32,
                section_blob_idx: *section_blob_idx.get(&gi).expect("assigned above"),
            },
        };
        blobs.push(entry);
        blob_hashes.push(g.info.hash);
    }

    // 10) Stream the new blobs into the data sections while writing.
    let data_sections = [
        SectionSpec {
            name_stridx: stored_name,
            compression: CompressionType::None,
        },
        SectionSpec {
            name_stridx: compressed_name,
            compression: options.compression,
        },
    ];

    let strings = interner.as_strs();
    let input = GenerationInput {
        created_at: now_archive_time(),
        strings: &strings,
        blobs: &blobs,
        blob_hashes: &blob_hashes,
        dirs: &dirs,
        files: &files,
        data_sections: &data_sections,
        block_size: options.block_size,
        previous_integrity_hash: options.previous_integrity_hash,
        signing_key: options.signing_key,
    };

    let state = PackState {
        packed: AtomicU64::new(0),
        total: new_blob_bytes,
        item_path: Mutex::new(PathBuf::new()),
        item_done: AtomicU64::new(0),
        item_total: AtomicU64::new(0),
    };
    let mut source = FileSections {
        order: &new_order,
        groups: &groups,
        files: &collector.files,
        monitor,
        state: &state,
        buf: vec![0u8; READ_CHUNK],
    };
    state.report(monitor);
    let output = write_generation_with_progress(writer, &input, &mut source, &|n| {
        state.packed.fetch_add(n, Ordering::Relaxed);
        state.report(monitor);
    })?;

    monitor.log(&format!(
        "Wrote snapshot #{}: {} on disk for {} of new data",
        options.generation_idx + 1,
        fmt_bytes(output.bytes_written as u64),
        fmt_bytes(new_blob_bytes)
    ));

    Ok(PackSummary {
        integrity_hash: output.integrity_hash,
        file_count: files.len(),
        dir_count: dirs.len(),
        blob_count: blobs.len(),
        new_blob_bytes,
        bytes_written: output.bytes_written,
    })
}

const READ_CHUNK: usize = 1024 * 1024;

/// Shared progress state for the packing phase, updated by the file reader
/// and by compression workers.
struct PackState {
    packed: AtomicU64,
    total: u64,
    item_path: Mutex<PathBuf>,
    item_done: AtomicU64,
    item_total: AtomicU64,
}

impl PackState {
    fn report(&self, monitor: &dyn Monitor) {
        // Skip rather than block if another thread is reporting.
        let Ok(path) = self.item_path.try_lock() else {
            return;
        };
        monitor.progress(Progress {
            phase: Phase::Packing,
            done: self.packed.load(Ordering::Relaxed),
            total: self.total,
            item: (!path.as_os_str().is_empty()).then(|| Item {
                path: &path,
                done: self.item_done.load(Ordering::Relaxed),
                total: self.item_total.load(Ordering::Relaxed),
            }),
        });
    }
}

/// Streams each new blob's file contents into its data section, checking
/// that files did not change since they were hashed.
struct FileSections<'a> {
    order: &'a [Vec<usize>; SectionKind::COUNT],
    groups: &'a [BlobGroup],
    files: &'a [ScannedFile],
    monitor: &'a dyn Monitor,
    state: &'a PackState,
    buf: Vec<u8>,
}

impl SectionSource for FileSections<'_> {
    fn write_section(&mut self, section_idx: usize, out: &mut dyn Write) -> io::Result<()> {
        for &gi in &self.order[section_idx] {
            let group = &self.groups[gi];
            let path = &self.files[group.indices[0]].fs_path;
            let expected = group.info.size as u64;
            *self.state.item_path.lock().unwrap() = path.clone();
            self.state.item_done.store(0, Ordering::Relaxed);
            self.state.item_total.store(expected, Ordering::Relaxed);

            let changed = || {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} changed during the backup", path.display()),
                )
            };
            let with_path =
                |e: io::Error| io::Error::new(e.kind(), format!("{}: {e}", path.display()));

            let mut file = File::open(path).map_err(with_path)?;
            let mut hasher = Sha256::new();
            let mut done = 0u64;
            loop {
                self.monitor.checkpoint()?;
                let n = file.read(&mut self.buf).map_err(with_path)?;
                if n == 0 {
                    break;
                }
                done += n as u64;
                if done > expected {
                    return Err(changed());
                }
                hasher.update(&self.buf[..n]);
                out.write_all(&self.buf[..n])?;
                self.state.item_done.store(done, Ordering::Relaxed);
                self.state.report(self.monitor);
            }
            let hash: [u8; 32] = hasher.finalize().into();
            if done != expected || hash != group.info.hash {
                return Err(changed());
            }
        }
        Ok(())
    }
}

/// Packs `root` into the archive file at `archive_path`, appending a new
/// generation when `append` is true and creating the file otherwise.
///
/// If packing fails or is cancelled, the file is restored to its previous
/// length (or removed if it was created), so the archive is never left with a
/// partial generation.
pub fn pack_into_file(
    archive_path: &Path,
    root: &Path,
    append: bool,
    options: PackOptions,
    monitor: &dyn Monitor,
) -> Result<PackSummary> {
    let (file, original_len) = if append {
        let file = OpenOptions::new()
            .write(true)
            .open(archive_path)
            .with_context(|| format!("opening {}", archive_path.display()))?;
        let len = file.metadata()?.len();
        (file, Some(len))
    } else {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(archive_path)
            .with_context(|| format!("creating {}", archive_path.display()))?;
        (file, None)
    };

    let result = (|| {
        let mut handle = &file;
        handle.seek(SeekFrom::End(0))?;
        let mut out = BufWriter::with_capacity(4 * 1024 * 1024, handle);
        let summary = pack_directory_with_monitor(&mut out, root, !append, options, monitor)?;
        out.flush()?;
        drop(out);
        file.sync_all()?;
        Ok(summary)
    })();

    if result.is_err() {
        let rollback = match original_len {
            Some(len) => file.set_len(len),
            None => {
                drop(file);
                fs::remove_file(archive_path)
            }
        };
        match rollback {
            Ok(()) => monitor.log("Archive restored to its previous state"),
            Err(e) => monitor.log(&format!(
                "Warning: could not restore {}: {e}",
                archive_path.display()
            )),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::compression::DefaultCompressionPolicy;
    use bpfs_core::constants::DATA_BLOCK_SIZE;
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use tempfile::tempdir;

    fn default_policy() -> DefaultCompressionPolicy {
        DefaultCompressionPolicy {
            incompressible_entropy: 96.25,
        }
    }

    fn options(policy: &DefaultCompressionPolicy) -> PackOptions<'_> {
        PackOptions {
            generation_idx: 0,
            previous_integrity_hash: [0u8; 32],
            policy,
            compression: CompressionType::Zstd,
            block_size: DATA_BLOCK_SIZE,
            signing_key: None,
            existing_blobs: &NoExistingBlobs,
        }
    }

    #[test]
    fn rejects_non_directory_root() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("f.txt");
        fs::write(&file_path, b"x").unwrap();

        let policy = default_policy();
        let mut out = Vec::new();
        assert!(pack_directory(&mut out, &file_path, true, options(&policy)).is_err());
    }

    #[test]
    fn packs_a_small_tree_and_writes_header() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"hello world").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.bin"), b"\x00\x01\x02\x03").unwrap();

        let policy = default_policy();
        let mut out = Vec::new();
        let summary = pack_directory(&mut out, dir.path(), true, options(&policy)).unwrap();

        assert_eq!(summary.file_count, 2);
        assert_eq!(summary.dir_count, 2); // root + sub
        assert_eq!(summary.blob_count, 2);
        assert_eq!(&out[0..4], b"BPFS");
        assert_eq!(out.len(), 16 + summary.bytes_written);
    }

    #[test]
    fn duplicate_content_dedupes_into_one_blob() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"same bytes").unwrap();
        fs::write(dir.path().join("b.txt"), b"same bytes").unwrap();

        let policy = default_policy();
        let mut out = Vec::new();
        let summary = pack_directory(&mut out, dir.path(), true, options(&policy)).unwrap();

        assert_eq!(summary.file_count, 2);
        assert_eq!(summary.blob_count, 1);
    }

    #[test]
    fn empty_directory_tree_packs_without_error() {
        let dir = tempdir().unwrap();
        let policy = default_policy();
        let mut out = Vec::new();
        let summary = pack_directory(&mut out, dir.path(), true, options(&policy)).unwrap();
        assert_eq!(summary.file_count, 0);
        assert_eq!(summary.dir_count, 1);
        assert_eq!(summary.blob_count, 0);
    }

    #[test]
    fn no_header_when_appending() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"x").unwrap();

        let policy = default_policy();
        let mut out = Vec::new();
        let opts = PackOptions {
            generation_idx: 1,
            ..options(&policy)
        };
        let summary = pack_directory(&mut out, dir.path(), false, opts).unwrap();
        assert_ne!(&out[0..4], b"BPFS");
        assert_eq!(out.len(), summary.bytes_written);
    }

    #[test]
    fn existing_blob_lookup_avoids_rewriting_bytes() {
        struct AlwaysExisting;
        impl ExistingBlobLookup for AlwaysExisting {
            fn find(&self, _hash: &[u8; 32]) -> Option<ExistingBlob> {
                Some(ExistingBlob {
                    generation_idx: 0,
                    section_idx: 0,
                    section_blob_idx: 0,
                })
            }
        }

        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"some content").unwrap();

        let policy = default_policy();
        let mut out = Vec::new();
        let opts = PackOptions {
            generation_idx: 1,
            existing_blobs: &AlwaysExisting,
            ..options(&policy)
        };
        let summary = pack_directory(&mut out, dir.path(), false, opts).unwrap();
        assert_eq!(summary.new_blob_bytes, 0);
    }

    /// (phase, done, total, current file)
    type Event = (Phase, u64, u64, Option<PathBuf>);

    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<Event>>,
        logs: Mutex<Vec<String>>,
        cancel_on: Option<Phase>,
        cancelled: AtomicBool,
    }

    impl Monitor for Recorder {
        fn progress(&self, p: Progress<'_>) {
            if Some(p.phase) == self.cancel_on {
                self.cancelled.store(true, Ordering::Relaxed);
            }
            self.events.lock().unwrap().push((
                p.phase,
                p.done,
                p.total,
                p.item.map(|i| i.path.to_path_buf()),
            ));
        }
        fn log(&self, message: &str) {
            self.logs.lock().unwrap().push(message.to_string());
        }
        fn is_cancelled(&self) -> bool {
            self.cancelled.load(Ordering::Relaxed)
        }
    }

    #[test]
    fn reports_progress_for_every_phase_in_order() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), vec![b'a'; 3000]).unwrap();
        fs::write(dir.path().join("b.bin"), vec![7u8; 5000]).unwrap();

        let recorder = Recorder::default();
        let policy = default_policy();
        let mut out = Vec::new();
        let opts = PackOptions {
            block_size: 1024,
            ..options(&policy)
        };
        pack_directory_with_monitor(&mut out, dir.path(), true, opts, &recorder).unwrap();

        let events = recorder.events.into_inner().unwrap();
        let mut phases: Vec<Phase> = events.iter().map(|e| e.0).collect();
        phases.dedup();
        assert_eq!(phases, [Phase::Scanning, Phase::Hashing, Phase::Packing]);

        for phase in [Phase::Hashing, Phase::Packing] {
            let last = events.iter().rev().find(|e| e.0 == phase).unwrap();
            assert_eq!((last.1, last.2), (8000, 8000), "{phase:?}");
        }
        assert!(
            events
                .iter()
                .any(|e| e.0 == Phase::Packing
                    && e.3.as_deref().is_some_and(|p| p.ends_with("b.bin")))
        );
        assert!(!recorder.logs.into_inner().unwrap().is_empty());
    }

    #[test]
    fn cancelled_new_archive_is_removed() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("a.txt"), vec![b'a'; 3000]).unwrap();
        let out_dir = tempdir().unwrap();
        let archive = out_dir.path().join("new.bpfs");

        let recorder = Recorder {
            cancel_on: Some(Phase::Hashing),
            ..Default::default()
        };
        let policy = default_policy();
        let err = pack_into_file(&archive, src.path(), false, options(&policy), &recorder);
        assert!(err.is_err());
        assert!(!archive.exists());
    }

    #[test]
    fn failed_append_restores_previous_length() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("a.txt"), vec![b'a'; 3000]).unwrap();
        let out_dir = tempdir().unwrap();
        let archive = out_dir.path().join("a.bpfs");

        let policy = default_policy();
        pack_into_file(&archive, src.path(), false, options(&policy), &NoMonitor).unwrap();
        let before = fs::read(&archive).unwrap();

        fs::write(src.path().join("b.txt"), vec![b'b'; 3000]).unwrap();
        let recorder = Recorder {
            cancel_on: Some(Phase::Packing),
            ..Default::default()
        };
        let opts = PackOptions {
            generation_idx: 1,
            ..options(&policy)
        };
        assert!(pack_into_file(&archive, src.path(), true, opts, &recorder).is_err());
        assert_eq!(fs::read(&archive).unwrap(), before);
    }

    #[test]
    fn creating_over_existing_file_fails_without_touching_it() {
        let src = tempdir().unwrap();
        let out_dir = tempdir().unwrap();
        let archive = out_dir.path().join("a.bpfs");
        fs::write(&archive, b"keep me").unwrap();

        let policy = default_policy();
        assert!(pack_into_file(&archive, src.path(), false, options(&policy), &NoMonitor).is_err());
        assert_eq!(fs::read(&archive).unwrap(), b"keep me");
    }
}

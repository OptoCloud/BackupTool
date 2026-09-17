//! Top-level orchestration: scan a directory, dedupe/analyze its files into
//! blobs, and write one BPFS `Generation` record for them.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use ed25519_dalek::SigningKey;

use bpfs_core::types::enums::{CompressionType, SectionKind};
use bpfs_core::types::packed::{BlobEntry, DirectoryEntry, FileEntry, NO_PARENT};

use crate::analyze::blob::{collect_unique_blobs, BlobGroup};
use crate::encode::generation::{write_generation, GenerationInput, PendingDataSection};
use crate::encode::header::write_header;
use crate::interner::StringInterner;
use crate::policy::CompressionPolicy;
use crate::scan::collector::FileCollector;

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
    if !root.is_dir() {
        anyhow::bail!("pack root must be a directory: {}", root.display());
    }

    if write_archive_header {
        write_header(&mut *writer, 0)?;
    }

    // 1) Scan the filesystem tree.
    let mut collector = FileCollector::new();
    for step in collector.add_path(root.to_path_buf(), PathBuf::new(), false) {
        step.with_context(|| format!("scanning {}", root.display()))?;
    }

    // 2) Dedupe + hash + entropy-analyze file contents into blobs.
    let groups: Vec<BlobGroup> =
        collect_unique_blobs(&collector.files).context("blob analysis failed")?;

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
    for g in &groups {
        if let Some(existing) = options.existing_blobs.find(&g.info.hash) {
            resolved.push(Resolved::Existing(existing));
            continue;
        }
        let ext = g
            .indices
            .first()
            .map(|&fi| extension_of(&collector.files[fi].archive_path))
            .unwrap_or_default();
        let section = if options.policy.should_compress(&ext, g.info.entropy as f64) {
            SectionKind::Compressed
        } else {
            SectionKind::Stored
        };
        new_blob_bytes += g.info.size as u64;
        resolved.push(Resolved::New {
            section,
            sort_ext: ext,
            sort_size: g.info.size as u64,
        });
    }

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

    // 5) Concatenate raw bytes per section (in the sorted order above) and
    //    record each new blob's section_blob_idx.
    let mut section_blob_idx: HashMap<usize, u32> = HashMap::new(); // group idx -> idx within its section
    let mut section_raw: [Vec<u8>; SectionKind::COUNT] = [Vec::new(), Vec::new()];
    for (section_kind, order) in new_order.iter().enumerate() {
        for (pos, &gi) in order.iter().enumerate() {
            section_blob_idx.insert(gi, pos as u32);
            let fi = groups[gi].indices[0];
            let bytes = fs::read(&collector.files[fi].fs_path)
                .with_context(|| format!("reading {}", collector.files[fi].fs_path.display()))?;
            section_raw[section_kind].extend_from_slice(&bytes);
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

    let data_sections = vec![
        PendingDataSection {
            name_stridx: stored_name,
            compression: CompressionType::None,
            raw: std::mem::take(&mut section_raw[SectionKind::Stored as usize]),
        },
        PendingDataSection {
            name_stridx: compressed_name,
            compression: CompressionType::Brotli,
            raw: std::mem::take(&mut section_raw[SectionKind::Compressed as usize]),
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
        previous_integrity_hash: options.previous_integrity_hash,
        signing_key: options.signing_key,
    };

    let output = write_generation(writer, &input)?;

    Ok(PackSummary {
        integrity_hash: output.integrity_hash,
        file_count: files.len(),
        dir_count: dirs.len(),
        blob_count: blobs.len(),
        new_blob_bytes,
        bytes_written: output.bytes_written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::compression::DefaultCompressionPolicy;
    use std::fs;
    use tempfile::tempdir;

    fn default_policy() -> DefaultCompressionPolicy {
        DefaultCompressionPolicy {
            incompressible_entropy: 96.25,
        }
    }

    #[test]
    fn rejects_non_directory_root() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("f.txt");
        fs::write(&file_path, b"x").unwrap();

        let policy = default_policy();
        let mut out = Vec::new();
        let opts = PackOptions {
            generation_idx: 0,
            previous_integrity_hash: [0u8; 32],
            policy: &policy,
            signing_key: None,
            existing_blobs: &NoExistingBlobs,
        };
        assert!(pack_directory(&mut out, &file_path, true, opts).is_err());
    }

    #[test]
    fn packs_a_small_tree_and_writes_header() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"hello world").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.bin"), b"\x00\x01\x02\x03").unwrap();

        let policy = default_policy();
        let mut out = Vec::new();
        let opts = PackOptions {
            generation_idx: 0,
            previous_integrity_hash: [0u8; 32],
            policy: &policy,
            signing_key: None,
            existing_blobs: &NoExistingBlobs,
        };
        let summary = pack_directory(&mut out, dir.path(), true, opts).unwrap();

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
        let opts = PackOptions {
            generation_idx: 0,
            previous_integrity_hash: [0u8; 32],
            policy: &policy,
            signing_key: None,
            existing_blobs: &NoExistingBlobs,
        };
        let summary = pack_directory(&mut out, dir.path(), true, opts).unwrap();

        assert_eq!(summary.file_count, 2);
        assert_eq!(summary.blob_count, 1);
    }

    #[test]
    fn empty_directory_tree_packs_without_error() {
        let dir = tempdir().unwrap();
        let policy = default_policy();
        let mut out = Vec::new();
        let opts = PackOptions {
            generation_idx: 0,
            previous_integrity_hash: [0u8; 32],
            policy: &policy,
            signing_key: None,
            existing_blobs: &NoExistingBlobs,
        };
        let summary = pack_directory(&mut out, dir.path(), true, opts).unwrap();
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
            previous_integrity_hash: [9u8; 32],
            policy: &policy,
            signing_key: None,
            existing_blobs: &NoExistingBlobs,
        };
        pack_directory(&mut out, dir.path(), false, opts).unwrap();
        assert_ne!(&out[0..4], b"BPFS");
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
            previous_integrity_hash: [0u8; 32],
            policy: &policy,
            signing_key: None,
            existing_blobs: &AlwaysExisting,
        };
        let summary = pack_directory(&mut out, dir.path(), false, opts).unwrap();
        assert_eq!(summary.new_blob_bytes, 0);
    }
}

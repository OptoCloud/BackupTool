use std::fs::{self, File};
use std::io::{Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use bpfs_core::errors::{ArchiveError, Result};
use bpfs_core::progress::{Item, Monitor, Phase, Progress};
use bpfs_core::types::packed::NO_PARENT;

use crate::archive::Archive;
use crate::generation::Generation;

#[derive(Debug, Clone)]
pub struct ResolvedFile {
    pub path: PathBuf,
    pub file_idx: usize,
}

/// Reconstructs a directory's full path by walking its `parent_id` chain.
pub fn resolve_dir_path(gen: &Generation, dir_idx: u32) -> Result<PathBuf> {
    let mut parts = Vec::new();
    let mut cur = Some(dir_idx);
    let mut steps = 0usize;

    while let Some(idx) = cur {
        steps += 1;
        if steps > gen.dirs.len() + 1 {
            return Err(ArchiveError::Format(
                "directory parent chain cycle detected".into(),
            ));
        }
        let d = gen
            .dirs
            .get(idx as usize)
            .ok_or(ArchiveError::IndexOutOfBounds("DirectoryEntry.parent_id"))?;
        let name = gen.strings.get(d.name_stridx)?;
        if !name.is_empty() {
            parts.push(name.to_string());
        }
        cur = if d.parent_id == NO_PARENT {
            None
        } else {
            Some(d.parent_id)
        };
    }

    parts.reverse();
    Ok(parts.into_iter().collect())
}

/// Resolves every file in `gen` to its full archive-relative path. Since
/// every `pack()` call writes a complete snapshot, the newest generation
/// (`archive.latest()`) is the current state of the backed-up tree.
pub fn current_files(gen: &Generation) -> Result<Vec<ResolvedFile>> {
    let mut out = Vec::with_capacity(gen.files.len());
    for (idx, f) in gen.files.iter().enumerate() {
        let dir_path = resolve_dir_path(gen, f.dir_idx)?;
        let name = gen.strings.get(f.name_stridx)?;
        out.push(ResolvedFile {
            path: dir_path.join(name),
            file_idx: idx,
        });
    }
    Ok(out)
}

/// Reads one resolved file's bytes into memory.
pub fn extract_file_bytes<R: Read + Seek>(
    archive: &Archive<R>,
    gen: &Generation,
    resolved: &ResolvedFile,
) -> Result<Vec<u8>> {
    let file = gen
        .files
        .get(resolved.file_idx)
        .ok_or(ArchiveError::IndexOutOfBounds("ResolvedFile.file_idx"))?;
    let blob = gen
        .blobs
        .get(file.blob_idx as usize)
        .ok_or(ArchiveError::IndexOutOfBounds("FileEntry.blob_idx"))?;
    archive.read_blob(blob)
}

/// Rejects archive paths that could escape the destination directory.
fn check_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(ArchiveError::Format(format!(
            "unsafe path in archive: {}",
            path.display()
        )));
    }
    Ok(())
}

pub struct ExtractSummary {
    pub files: usize,
    pub bytes: u64,
}

const COPY_CHUNK: usize = 1024 * 1024;

/// Writes `files` (resolved from `gen`) under `dest`, streaming each blob.
/// Fails before writing anything if a target exists and `overwrite` is false.
/// A file interrupted by an error or cancellation is removed.
pub fn extract_files<R: Read + Seek>(
    archive: &Archive<R>,
    gen: &Generation,
    files: &[ResolvedFile],
    dest: &Path,
    overwrite: bool,
    monitor: &dyn Monitor,
) -> Result<ExtractSummary> {
    let mut total = 0u64;
    for r in files {
        check_relative(&r.path)?;
        let file = gen
            .files
            .get(r.file_idx)
            .ok_or(ArchiveError::IndexOutOfBounds("ResolvedFile.file_idx"))?;
        total += gen.blobs[file.blob_idx as usize].raw_size;
        if !overwrite && dest.join(&r.path).exists() {
            return Err(ArchiveError::Format(format!(
                "{} already exists",
                dest.join(&r.path).display()
            )));
        }
    }

    fs::create_dir_all(dest)?;
    let mut buf = vec![0u8; COPY_CHUNK];
    let mut done = 0u64;
    for r in files {
        monitor.checkpoint()?;
        let entry = &gen.files[r.file_idx];
        let blob = &gen.blobs[entry.blob_idx as usize];
        let out_path = dest.join(&r.path);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut copy = || -> Result<()> {
            let mut reader = archive.blob_reader(blob)?;
            let mut out = File::create(&out_path)?;
            let mut file_done = 0u64;
            let report = |done, file_done| {
                monitor.progress(Progress {
                    phase: Phase::Extracting,
                    done,
                    total,
                    item: Some(Item {
                        path: &r.path,
                        done: file_done,
                        total: blob.raw_size,
                    }),
                })
            };
            report(done, 0);
            loop {
                monitor.checkpoint()?;
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                file_done += n as u64;
                done += n as u64;
                report(done, file_done);
            }
            Ok(())
        };
        if let Err(e) = copy() {
            let _ = fs::remove_file(&out_path);
            return Err(match e {
                ArchiveError::Io(io) if io.kind() != std::io::ErrorKind::Interrupted => {
                    ArchiveError::Format(format!("{}: {io}", r.path.display()))
                }
                other => other,
            });
        }
    }

    Ok(ExtractSummary {
        files: files.len(),
        bytes: done,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::read_archive;
    use bpfs_core::constants::DATA_BLOCK_SIZE;
    use bpfs_core::progress::NoMonitor;
    use bpfs_core::types::enums::CompressionType;
    use bpfs_pack::pack::{pack_directory, NoExistingBlobs, PackOptions};
    use bpfs_pack::policy::compression::DefaultCompressionPolicy;
    use std::collections::HashSet;
    use std::io::Cursor;
    use tempfile::tempdir;

    fn pack_tree(dir: &Path) -> Archive<Cursor<Vec<u8>>> {
        let policy = DefaultCompressionPolicy {
            incompressible_entropy: 96.25,
        };
        let mut buf = Vec::new();
        pack_directory(
            &mut buf,
            dir,
            true,
            PackOptions {
                generation_idx: 0,
                previous_integrity_hash: [0u8; 32],
                policy: &policy,
                compression: CompressionType::Zstd,
                block_size: DATA_BLOCK_SIZE,
                signing_key: None,
                existing_blobs: &NoExistingBlobs,
            },
        )
        .unwrap();
        read_archive(Cursor::new(buf)).unwrap()
    }

    #[test]
    fn resolves_nested_paths() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        fs::write(dir.path().join("a/b/c.txt"), b"deep").unwrap();
        fs::write(dir.path().join("top.txt"), b"shallow").unwrap();

        let archive = pack_tree(dir.path());
        let gen = archive.latest();
        let resolved = current_files(gen).unwrap();

        let paths: HashSet<PathBuf> = resolved.iter().map(|r| r.path.clone()).collect();
        assert!(paths.contains(&PathBuf::from("a/b/c.txt")));
        assert!(paths.contains(&PathBuf::from("top.txt")));
    }

    #[test]
    fn extracts_correct_bytes_for_each_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"content A").unwrap();
        fs::write(dir.path().join("b.txt"), b"content B, longer").unwrap();

        let archive = pack_tree(dir.path());
        let gen = archive.latest();
        let resolved = current_files(gen).unwrap();

        for r in &resolved {
            let bytes = extract_file_bytes(&archive, gen, r).unwrap();
            let expected = fs::read(dir.path().join(&r.path)).unwrap();
            assert_eq!(bytes, expected, "mismatch for {:?}", r.path);
        }
    }

    #[test]
    fn extract_files_writes_tree_and_respects_overwrite() {
        let src = tempdir().unwrap();
        fs::create_dir_all(src.path().join("x/y")).unwrap();
        fs::write(src.path().join("x/y/z.txt"), b"nested").unwrap();
        fs::write(src.path().join("empty.txt"), b"").unwrap();

        let archive = pack_tree(src.path());
        let gen = archive.latest();
        let resolved = current_files(gen).unwrap();

        let dest = tempdir().unwrap();
        let summary =
            extract_files(&archive, gen, &resolved, dest.path(), false, &NoMonitor).unwrap();
        assert_eq!(summary.files, 2);
        assert_eq!(summary.bytes, 6);
        assert_eq!(fs::read(dest.path().join("x/y/z.txt")).unwrap(), b"nested");
        assert_eq!(fs::read(dest.path().join("empty.txt")).unwrap(), b"");

        assert!(extract_files(&archive, gen, &resolved, dest.path(), false, &NoMonitor).is_err());
        assert!(extract_files(&archive, gen, &resolved, dest.path(), true, &NoMonitor).is_ok());
    }

    #[test]
    fn rejects_paths_that_escape_destination() {
        assert!(check_relative(Path::new("../evil")).is_err());
        assert!(check_relative(Path::new("/etc/passwd")).is_err());
        assert!(check_relative(Path::new("a/./b")).is_ok());
        assert!(check_relative(Path::new("a/b")).is_ok());
    }

    #[test]
    fn empty_tree_resolves_to_no_files() {
        let dir = tempdir().unwrap();
        let archive = pack_tree(dir.path());
        assert!(current_files(archive.latest()).unwrap().is_empty());
    }
}

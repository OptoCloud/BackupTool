use std::path::PathBuf;

use bpfs_core::errors::{ArchiveError, Result};
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

/// Extracts one resolved file's bytes from the archive.
pub fn extract_file_bytes(
    archive: &Archive,
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
    Ok(archive.blob_bytes(blob)?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::read_archive;
    use bpfs_pack::pack::{pack_directory, NoExistingBlobs, PackOptions};
    use bpfs_pack::policy::compression::DefaultCompressionPolicy;
    use std::collections::HashSet;
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;
    use tempfile::tempdir;

    fn pack_tree(dir: &Path) -> Archive {
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
    fn empty_tree_resolves_to_no_files() {
        let dir = tempdir().unwrap();
        let archive = pack_tree(dir.path());
        assert!(current_files(archive.latest()).unwrap().is_empty());
    }
}

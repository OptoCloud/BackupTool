//! Structural sanity checks over a parsed generation, independent of
//! cryptographic hash verification (see `bpfs-read`'s verify module for that).

use crate::errors::{ArchiveError, Result};
use crate::types::packed::{DirectoryEntry, FileEntry, NO_PARENT};

/// Verifies that every directory's `parent_id` either is `NO_PARENT` or
/// refers to an earlier directory in the slice (parents must be declared
/// before their children), and that there are no cycles.
pub fn validate_directory_tree(dirs: &[DirectoryEntry]) -> Result<()> {
    for (idx, dir) in dirs.iter().enumerate() {
        if dir.parent_id == NO_PARENT {
            continue;
        }
        let parent_idx = dir.parent_id as usize;
        if parent_idx >= dirs.len() {
            return Err(ArchiveError::IndexOutOfBounds("DirectoryEntry.parent_id"));
        }
        if parent_idx >= idx {
            return Err(ArchiveError::Format(format!(
                "directory {idx} references parent {parent_idx} which is not declared before it"
            )));
        }
    }
    Ok(())
}

/// Verifies every `FileEntry.dir_idx` and `FileEntry.blob_idx` is in bounds.
pub fn validate_file_refs(files: &[FileEntry], dir_count: usize, blob_count: usize) -> Result<()> {
    for (idx, file) in files.iter().enumerate() {
        if file.dir_idx as usize >= dir_count {
            return Err(ArchiveError::Format(format!(
                "file {idx} references out-of-bounds directory {}",
                file.dir_idx
            )));
        }
        if file.blob_idx as usize >= blob_count {
            return Err(ArchiveError::Format(format!(
                "file {idx} references out-of-bounds blob {}",
                file.blob_idx
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(parent_id: u32) -> DirectoryEntry {
        DirectoryEntry {
            parent_id,
            name_stridx: 0,
            created_at: 0,
            modified_at: 0,
        }
    }

    fn file(dir_idx: u32, blob_idx: u32) -> FileEntry {
        FileEntry {
            blob_idx,
            dir_idx,
            name_stridx: 0,
            created_at: 0,
            modified_at: 0,
        }
    }

    #[test]
    fn empty_tree_is_valid() {
        assert!(validate_directory_tree(&[]).is_ok());
    }

    #[test]
    fn single_root_is_valid() {
        assert!(validate_directory_tree(&[dir(NO_PARENT)]).is_ok());
    }

    #[test]
    fn parent_declared_before_child_is_valid() {
        let dirs = vec![dir(NO_PARENT), dir(0)];
        assert!(validate_directory_tree(&dirs).is_ok());
    }

    #[test]
    fn forward_reference_is_rejected() {
        // dir 0 claims dir 1 (not yet declared) as its parent
        let dirs = vec![dir(1), dir(NO_PARENT)];
        assert!(validate_directory_tree(&dirs).is_err());
    }

    #[test]
    fn out_of_bounds_parent_is_rejected() {
        let dirs = vec![dir(42)];
        assert!(matches!(
            validate_directory_tree(&dirs),
            Err(ArchiveError::IndexOutOfBounds(_))
        ));
    }

    #[test]
    fn self_reference_is_rejected() {
        // dir 0 pointing at itself: parent_idx (0) >= idx (0) triggers the ordering check
        let dirs = vec![dir(0)];
        assert!(validate_directory_tree(&dirs).is_err());
    }

    #[test]
    fn file_refs_in_bounds_ok() {
        assert!(validate_file_refs(&[file(0, 0)], 1, 1).is_ok());
    }

    #[test]
    fn file_dir_out_of_bounds_rejected() {
        assert!(validate_file_refs(&[file(5, 0)], 1, 1).is_err());
    }

    #[test]
    fn file_blob_out_of_bounds_rejected() {
        assert!(validate_file_refs(&[file(0, 5)], 1, 1).is_err());
    }
}

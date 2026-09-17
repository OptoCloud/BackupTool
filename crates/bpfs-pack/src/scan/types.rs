use bpfs_core::filetimes::FileTimes;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    pub fs_path: PathBuf,
    pub archive_path: PathBuf,
    pub filetimes: FileTimes,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub fs_path: PathBuf,
    pub archive_path: PathBuf,
    pub size_hint: u64,
    pub filetimes: FileTimes,
}

// TODO: Support file and directory sym/hard-links, internal sym/hard-links (links pointing to files included in archive)
#[derive(Debug, Clone)]
pub struct SymlinkEntry {
    pub fs_path: PathBuf,
    pub archive_path: PathBuf,
    pub fs_target_path: PathBuf,
    pub filetimes: FileTimes,
}

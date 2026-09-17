//! In-memory representation of records exactly as they appear on disk within
//! one generation (after parsing, before/after (de)serialization). See
//! `imhex.pattern` at the repo root for the authoritative byte layout.

/// 8 bytes on disk: `u32 offset, u32 length` into a generation's decompressed
/// string heap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringEntry {
    pub offset: u32,
    pub length: u32,
}

/// 16 bytes on disk. Blobs are content-addressed and may be dedup'd against
/// any earlier generation: `generation_idx` names the owning generation and
/// `section_idx`/`section_blob_idx` locate it within that generation's data
/// sections. A blob physically stored in the *current* generation has
/// `generation_idx` equal to that generation's own index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlobEntry {
    pub raw_size: u64,
    pub generation_idx: u32,
    pub section_idx: u32,
    pub section_blob_idx: u32,
}

/// 24 bytes on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub parent_id: u32,
    pub name_stridx: u32,
    pub created_at: u64,
    pub modified_at: u64,
}

/// 24 bytes on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub blob_idx: u32,
    pub dir_idx: u32,
    pub name_stridx: u32,
    pub created_at: u64,
    pub modified_at: u64,
}

/// Sentinel meaning "no parent" (this directory is a root).
pub const NO_PARENT: u32 = u32::MAX;

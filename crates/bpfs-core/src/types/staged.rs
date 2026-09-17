use crate::filetimes::FileTimes;
use crate::types::enums::SectionKind;

/// Where a staged blob's bytes should come from when a generation is written.
#[derive(Clone, Copy, Debug)]
pub enum StagedBlobSource {
    /// Bytes must be written fresh into the generation being built. `input_idx`
    /// indexes into the packer's own list of scanned files (opaque to bpfs-core).
    New { input_idx: usize },
    /// An identical blob already exists in an earlier generation; reference it
    /// instead of writing the bytes again.
    Existing {
        generation_idx: u32,
        section_idx: u32,
        section_blob_idx: u32,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct StagedBlobEntry {
    pub hash: [u8; 32],
    pub raw_size: u64,
    pub section: SectionKind,
    pub source: StagedBlobSource,
}

#[derive(Clone, Copy, Debug)]
pub struct StagedDirectoryEntry {
    pub parent_idx: Option<u32>,
    pub name_stridx: u32,
    pub filetimes: FileTimes,
}

#[derive(Clone, Copy, Debug)]
pub struct StagedFileEntry {
    pub blob_idx: u32,
    pub dir_idx: u32,
    pub name_stridx: u32,
    pub filetimes: FileTimes,
}

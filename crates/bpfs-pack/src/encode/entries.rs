use byteorder::{LittleEndian, WriteBytesExt};
use std::io::{self, Write};

use bpfs_core::types::packed::{BlobEntry, DirectoryEntry, FileEntry};

/// 20 bytes: `u64 raw_size, u32 generation_idx, u32 section_idx, u32 section_blob_idx`.
pub fn write_blob_entry<W: Write>(w: &mut W, e: &BlobEntry) -> io::Result<()> {
    w.write_u64::<LittleEndian>(e.raw_size)?;
    w.write_u32::<LittleEndian>(e.generation_idx)?;
    w.write_u32::<LittleEndian>(e.section_idx)?;
    w.write_u32::<LittleEndian>(e.section_blob_idx)?;
    Ok(())
}

pub const BLOB_ENTRY_SIZE: usize = 8 + 4 + 4 + 4;

/// 24 bytes: `u32 parent_id, u32 name_stridx, u64 created_at, u64 modified_at`.
pub fn write_directory_entry<W: Write>(w: &mut W, e: &DirectoryEntry) -> io::Result<()> {
    w.write_u32::<LittleEndian>(e.parent_id)?;
    w.write_u32::<LittleEndian>(e.name_stridx)?;
    w.write_u64::<LittleEndian>(e.created_at)?;
    w.write_u64::<LittleEndian>(e.modified_at)?;
    Ok(())
}

pub const DIRECTORY_ENTRY_SIZE: usize = 4 + 4 + 8 + 8;

/// 24 bytes: `u32 blob_idx, u32 dir_idx, u32 name_stridx, u64 created_at, u64 modified_at`.
pub fn write_file_entry<W: Write>(w: &mut W, e: &FileEntry) -> io::Result<()> {
    w.write_u32::<LittleEndian>(e.blob_idx)?;
    w.write_u32::<LittleEndian>(e.dir_idx)?;
    w.write_u32::<LittleEndian>(e.name_stridx)?;
    w.write_u64::<LittleEndian>(e.created_at)?;
    w.write_u64::<LittleEndian>(e.modified_at)?;
    Ok(())
}

pub const FILE_ENTRY_SIZE: usize = 4 + 4 + 4 + 8 + 8;

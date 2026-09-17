use bpfs_core::types::enums::CompressionType;
use sha2::{Digest, Sha256};
use std::io::{self, Error, ErrorKind, Read, Seek, SeekFrom};

use crate::io::ReadLeExt;

/// Largest block (raw or stored) the reader will allocate for.
const MAX_BLOCK_BYTES: u32 = 1024 * 1024 * 1024;

/// Where one compressed block lives and what it decodes to.
#[derive(Clone, Debug)]
pub struct BlockRef {
    /// Offset of this block's first byte within the section's raw stream.
    pub raw_offset: u64,
    pub raw_size: u32,
    /// Absolute offset of the block's data in the archive.
    pub stored_offset: u64,
    pub stored_size: u32,
    pub hash: [u8; 32],
}

/// A DataSection's header and block index; the data itself stays on disk.
#[derive(Clone, Debug)]
pub struct SectionIndex {
    pub name_stridx: u32,
    pub compression: CompressionType,
    pub block_size: u32,
    pub blocks: Vec<BlockRef>,
    /// Total uncompressed size of the section.
    pub raw_size: u64,
}

impl SectionIndex {
    /// Index of the block containing raw offset `pos`.
    pub fn block_at(&self, pos: u64) -> Option<usize> {
        let idx = self.blocks.partition_point(|b| b.raw_offset <= pos);
        let idx = idx.checked_sub(1)?;
        let b = &self.blocks[idx];
        (pos < b.raw_offset + b.raw_size as u64).then_some(idx)
    }
}

fn invalid(msg: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidData, msg.into())
}

/// Reads a DataSection header and its block headers, seeking past block data.
/// See `bpfs_pack::encode::data::SectionEncoder` for the layout.
pub fn index_data_section<R: Read + Seek>(reader: &mut R) -> io::Result<SectionIndex> {
    let start = reader.stream_position()?;
    let file_len = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(start))?;

    let name_stridx = reader.read_u32_le()?;
    let mut comp = [0u8; 4];
    reader.read_exact(&mut comp)?;
    let compression = CompressionType::try_from(comp[0])
        .map_err(|b| invalid(format!("unknown compression type {b}")))?;
    let block_size = reader.read_u32_le()?;
    if block_size == 0 || block_size > MAX_BLOCK_BYTES {
        return Err(invalid(format!("invalid block size {block_size}")));
    }

    let mut blocks = Vec::new();
    let mut raw_offset = 0u64;
    loop {
        let raw_size = reader.read_u32_le()?;
        let stored_size = reader.read_u32_le()?;
        if raw_size == 0 {
            if stored_size != 0 {
                return Err(invalid("malformed section terminator"));
            }
            break;
        }
        if raw_size > block_size || stored_size > MAX_BLOCK_BYTES {
            return Err(invalid("block size exceeds section limits"));
        }
        let mut hash = [0u8; 32];
        reader.read_exact(&mut hash)?;
        let stored_offset = reader.stream_position()?;
        let end = reader.seek(SeekFrom::Current(stored_size as i64))?;
        if end > file_len {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "data block is truncated",
            ));
        }
        blocks.push(BlockRef {
            raw_offset,
            raw_size,
            stored_offset,
            stored_size,
            hash,
        });
        raw_offset += raw_size as u64;
    }

    Ok(SectionIndex {
        name_stridx,
        compression,
        block_size,
        blocks,
        raw_size: raw_offset,
    })
}

/// Reads, checks and decompresses one block.
pub fn read_block<R: Read + Seek>(
    reader: &mut R,
    compression: CompressionType,
    block: &BlockRef,
) -> io::Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(block.stored_offset))?;
    let mut stored = vec![0u8; block.stored_size as usize];
    reader.read_exact(&mut stored)?;
    if Sha256::digest(&stored)[..] != block.hash {
        return Err(invalid("data block checksum mismatch"));
    }

    let raw = match compression {
        CompressionType::None => stored,
        CompressionType::Brotli => {
            let mut out = Vec::with_capacity(block.raw_size as usize);
            brotli::Decompressor::new(&stored[..], 64 * 1024)
                .take(block.raw_size as u64 + 1)
                .read_to_end(&mut out)?;
            out
        }
        CompressionType::Zstd => zstd::bulk::decompress(&stored, block.raw_size as usize)?,
        other => {
            return Err(invalid(format!(
                "CompressionType {other:?} is not implemented for decoding"
            )))
        }
    };
    if raw.len() != block.raw_size as usize {
        return Err(invalid("data block has the wrong decompressed size"));
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpfs_pack::encode::data::write_data_section;
    use std::io::Cursor;

    fn roundtrip(compression: CompressionType, block_size: u32, raw: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        write_data_section(&mut buf, 3, compression, block_size, raw).unwrap();
        let mut cursor = Cursor::new(buf);
        let index = index_data_section(&mut cursor).unwrap();
        assert_eq!(index.name_stridx, 3);
        assert_eq!(index.compression, compression);
        assert_eq!(index.raw_size, raw.len() as u64);
        assert_eq!(cursor.position(), cursor.get_ref().len() as u64);

        let mut out = Vec::new();
        for b in &index.blocks {
            out.extend(read_block(&mut cursor, compression, b).unwrap());
        }
        out
    }

    #[test]
    fn roundtrips_every_codec_across_blocks() {
        let raw: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
        for compression in [
            CompressionType::None,
            CompressionType::Brotli,
            CompressionType::Zstd,
        ] {
            assert_eq!(roundtrip(compression, 4096, &raw), raw, "{compression:?}");
        }
    }

    #[test]
    fn empty_section_has_no_blocks() {
        assert!(roundtrip(CompressionType::Zstd, 1024, b"").is_empty());
    }

    #[test]
    fn block_at_finds_containing_block() {
        let mut buf = Vec::new();
        write_data_section(&mut buf, 0, CompressionType::None, 10, &[0u8; 25]).unwrap();
        let index = index_data_section(&mut Cursor::new(buf)).unwrap();
        assert_eq!(index.block_at(0), Some(0));
        assert_eq!(index.block_at(9), Some(0));
        assert_eq!(index.block_at(10), Some(1));
        assert_eq!(index.block_at(24), Some(2));
        assert_eq!(index.block_at(25), None);
    }

    #[test]
    fn detects_corrupted_block() {
        let mut buf = Vec::new();
        write_data_section(&mut buf, 0, CompressionType::None, 1024, b"payload").unwrap();
        let len = buf.len();
        buf[len - 10] ^= 0xFF; // inside the block data
        let mut cursor = Cursor::new(buf);
        let index = index_data_section(&mut cursor).unwrap();
        assert!(read_block(&mut cursor, index.compression, &index.blocks[0]).is_err());
    }

    #[test]
    fn truncated_section_errors() {
        let mut buf = Vec::new();
        write_data_section(&mut buf, 0, CompressionType::None, 1024, b"payload").unwrap();
        buf.truncate(buf.len() - 5);
        assert!(index_data_section(&mut Cursor::new(buf)).is_err());
    }
}

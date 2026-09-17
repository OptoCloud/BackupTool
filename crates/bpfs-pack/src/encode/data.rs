use crate::io::le::WriteLeExt;
use bpfs_core::types::enums::CompressionType;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::io::{self, Error, ErrorKind, Write};

/// zstd level for data blocks.
pub const ZSTD_LEVEL: i32 = 6;
/// zstd window (2^26 = 64 MiB) with long-distance matching, so a whole block
/// can reference itself.
const ZSTD_WINDOW_LOG: u32 = 26;
/// Brotli quality and window (2^24 = 16 MiB, the standard maximum).
pub const BROTLI_QUALITY: u32 = 9;
const BROTLI_WINDOW: u32 = 24;

/// Upper bound on memory held by blocks waiting to be compressed and their
/// compressed output.
const MEMORY_BUDGET: usize = 1024 * 1024 * 1024;

/// Size of the fixed DataSection header.
pub const SECTION_HEADER_SIZE: usize = 4 + 1 + 3 + 4;
/// Size of each block header.
pub const BLOCK_HEADER_SIZE: usize = 4 + 4 + 32;

/// Streams one DataSection (generation-local):
///
/// ```text
/// u32 name_stridx
/// u8  compression
/// u8  padding[3]
/// u32 block_size             // nominal uncompressed size of each block
/// Block blocks[]             // until a block with raw_size == 0
///   u32 raw_size             // uncompressed length, <= block_size
///   u32 stored_size          // on-disk length of data
///   u8  hash[32]             // SHA-256 of data
///   u8  data[stored_size]
/// u32 0, u32 0               // terminator (8 bytes, no hash)
/// ```
///
/// Raw bytes written to the encoder are cut into `block_size` blocks, which
/// are compressed in parallel batches and written in order.
pub struct SectionEncoder<'a, W: Write> {
    out: &'a mut W,
    compression: CompressionType,
    block_size: usize,
    max_batch: usize,
    pending: Vec<Vec<u8>>,
    current: Vec<u8>,
    on_block_done: &'a (dyn Fn(u64) + Sync),
}

impl<'a, W: Write> SectionEncoder<'a, W> {
    /// Writes the section header. `on_block_done` is called (possibly from
    /// worker threads) with each block's uncompressed size once compressed.
    pub fn new(
        out: &'a mut W,
        name_stridx: u32,
        compression: CompressionType,
        block_size: u32,
        on_block_done: &'a (dyn Fn(u64) + Sync),
    ) -> io::Result<Self> {
        match compression {
            CompressionType::None | CompressionType::Brotli | CompressionType::Zstd => {}
            other => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!("CompressionType {other:?} is not implemented for encoding"),
                ))
            }
        }
        if block_size == 0 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "block size must be non-zero",
            ));
        }

        out.write_u32_le(name_stridx)?;
        out.write_u8(compression as u8)?;
        out.write_all(&[0u8; 3])?;
        out.write_u32_le(block_size)?;

        let block_size = block_size as usize;
        let max_batch = (MEMORY_BUDGET / (2 * block_size)).clamp(1, rayon::current_num_threads());
        Ok(Self {
            out,
            compression,
            block_size,
            max_batch,
            pending: Vec::new(),
            current: Vec::new(),
            on_block_done,
        })
    }

    /// Flushes remaining data and writes the terminator.
    pub fn finish(mut self) -> io::Result<()> {
        if !self.current.is_empty() {
            let block = std::mem::take(&mut self.current);
            self.pending.push(block);
        }
        self.flush_pending()?;
        self.out.write_u32_le(0)?;
        self.out.write_u32_le(0)?;
        Ok(())
    }

    fn flush_pending(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let compression = self.compression;
        let on_block_done = self.on_block_done;
        let encoded = std::mem::take(&mut self.pending)
            .into_par_iter()
            .map(|raw| {
                let raw_len = raw.len();
                let stored = compress_block(compression, raw)?;
                let hash: [u8; 32] = Sha256::digest(&stored).into();
                on_block_done(raw_len as u64);
                Ok((raw_len, stored, hash))
            })
            .collect::<io::Result<Vec<_>>>()?;

        for (raw_len, stored, hash) in encoded {
            self.out.write_u32_le(raw_len as u32)?;
            self.out
                .write_u32_le(u32::try_from(stored.len()).map_err(|_| {
                    Error::new(ErrorKind::InvalidData, "compressed block exceeds 4 GiB")
                })?)?;
            self.out.write_all(&hash)?;
            self.out.write_all(&stored)?;
        }
        Ok(())
    }
}

impl<W: Write> Write for SectionEncoder<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.current.capacity() == 0 {
            self.current.reserve_exact(self.block_size);
        }
        let n = buf.len().min(self.block_size - self.current.len());
        self.current.extend_from_slice(&buf[..n]);
        if self.current.len() == self.block_size {
            let block = std::mem::take(&mut self.current);
            self.pending.push(block);
            if self.pending.len() >= self.max_batch {
                self.flush_pending()?;
            }
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn compress_block(compression: CompressionType, raw: Vec<u8>) -> io::Result<Vec<u8>> {
    match compression {
        CompressionType::None => Ok(raw),
        CompressionType::Brotli => {
            let mut writer = brotli::CompressorWriter::new(
                Vec::with_capacity(raw.len() / 2),
                64 * 1024,
                BROTLI_QUALITY,
                BROTLI_WINDOW,
            );
            writer.write_all(&raw)?;
            writer.flush()?;
            Ok(writer.into_inner())
        }
        CompressionType::Zstd => {
            use zstd::zstd_safe::CParameter;
            let mut compressor = zstd::bulk::Compressor::new(ZSTD_LEVEL)?;
            compressor.set_parameter(CParameter::EnableLongDistanceMatching(true))?;
            compressor.set_parameter(CParameter::WindowLog(ZSTD_WINDOW_LOG))?;
            compressor.compress(&raw)
        }
        other => Err(Error::new(
            ErrorKind::InvalidInput,
            format!("CompressionType {other:?} is not implemented for encoding"),
        )),
    }
}

/// Writes a whole section from an in-memory buffer.
pub fn write_data_section<W: Write>(
    writer: &mut W,
    name_stridx: u32,
    compression: CompressionType,
    block_size: u32,
    raw: &[u8],
) -> io::Result<()> {
    let mut encoder = SectionEncoder::new(writer, name_stridx, compression, block_size, &|_| {})?;
    encoder.write_all(raw)?;
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_headers(buf: &[u8]) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut pos = SECTION_HEADER_SIZE;
        loop {
            let raw = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
            let stored = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into().unwrap());
            if raw == 0 {
                assert_eq!(pos + 8, buf.len(), "terminator must end the section");
                return out;
            }
            out.push((raw, stored));
            pos += BLOCK_HEADER_SIZE + stored as usize;
        }
    }

    #[test]
    fn stored_section_splits_into_blocks() {
        let raw: Vec<u8> = (0..250u32).map(|i| i as u8).collect();
        let mut buf = Vec::new();
        write_data_section(&mut buf, 0, CompressionType::None, 100, &raw).unwrap();
        assert_eq!(block_headers(&buf), [(100, 100), (100, 100), (50, 50)]);
    }

    #[test]
    fn compressed_sections_shrink_repetitive_data() {
        for compression in [CompressionType::Brotli, CompressionType::Zstd] {
            let raw = vec![b'a'; 10_000];
            let mut buf = Vec::new();
            write_data_section(&mut buf, 0, compression, 4096, &raw).unwrap();
            let blocks = block_headers(&buf);
            assert_eq!(blocks.len(), 3, "{compression:?}");
            assert!(blocks.iter().all(|&(r, s)| s < r), "{compression:?}");
        }
    }

    #[test]
    fn empty_section_is_header_and_terminator() {
        let mut buf = Vec::new();
        write_data_section(&mut buf, 0, CompressionType::Zstd, 1024, b"").unwrap();
        assert_eq!(buf.len(), SECTION_HEADER_SIZE + 8);
    }

    #[test]
    fn reports_each_block() {
        let total = std::sync::atomic::AtomicU64::new(0);
        let on_block = |n| {
            total.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
        };
        let mut buf = Vec::new();
        let mut enc =
            SectionEncoder::new(&mut buf, 0, CompressionType::Zstd, 64, &on_block).unwrap();
        enc.write_all(&[1u8; 1000]).unwrap();
        enc.finish().unwrap();
        assert_eq!(total.into_inner(), 1000);
    }

    #[test]
    fn unsupported_compression_errors() {
        let mut buf = Vec::new();
        let err = write_data_section(&mut buf, 0, CompressionType::Lz4, 1024, b"x").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }
}

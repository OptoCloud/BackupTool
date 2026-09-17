use crate::io::le::WriteLeExt;
use crate::io::writers::HashWriter;
use brotli::CompressorWriter;
use sha2::{Digest, Sha256};
use std::io::{self, Write};

/// StringSection (generation-local):
///   u32 count
///   StringEntry lookup[count]  // { u32 offset, u32 length } into the decompressed heap
///   u32 uncompressed_size
///   u32 stored_size
///   u8  chunk[stored_size]     // brotli-compressed concatenation of all strings
///   u8  hash[32]               // SHA-256 of everything above
pub fn write_string_section<W: Write>(writer: &mut W, strings: &[&str]) -> io::Result<()> {
    let count = u32::try_from(strings.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many strings"))?;

    let mut heap = Vec::new();
    let mut lookup = Vec::with_capacity(strings.len());
    for s in strings {
        let offset = u32::try_from(heap.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "string heap too large"))?;
        let length = u32::try_from(s.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "string too long"))?;
        heap.extend_from_slice(s.as_bytes());
        lookup.push((offset, length));
    }

    let mut compressed = Vec::new();
    {
        let mut brotli = CompressorWriter::new(&mut compressed, 8192, 9, 22);
        brotli.write_all(&heap)?;
        brotli.flush()?;
    }

    let stored_size = u32::try_from(compressed.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "compressed string heap too large",
        )
    })?;
    let uncompressed_size = u32::try_from(heap.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "string heap too large"))?;

    let mut hasher = Sha256::new();
    {
        let mut hw = HashWriter::new(writer, &mut hasher);
        hw.write_u32_le(count)?;
        for (offset, length) in &lookup {
            hw.write_u32_le(*offset)?;
            hw.write_u32_le(*length)?;
        }
        hw.write_u32_le(uncompressed_size)?;
        hw.write_u32_le(stored_size)?;
        hw.write_all(&compressed)?;
    }

    let digest = hasher.finalize();
    writer.write_all(&digest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_without_error() {
        let mut buf = Vec::new();
        write_string_section(&mut buf, &["hello", "world", ""]).unwrap();
        assert!(!buf.is_empty());
    }

    #[test]
    fn empty_string_list() {
        let mut buf = Vec::new();
        write_string_section(&mut buf, &[]).unwrap();
        // count(4) + size fields(8) + brotli-of-nothing (a few bytes) + hash(32)
        assert!(buf.len() >= 4 + 4 + 4 + 32);
    }
}

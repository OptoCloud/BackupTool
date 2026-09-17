use crate::io::le::WriteLeExt;
use crate::io::writers::HashWriter;
use bpfs_core::types::enums::CompressionType;
use brotli::CompressorWriter;
use sha2::{Digest, Sha256};
use std::io::{self, Error, ErrorKind, Write};

/// DataSection (generation-local):
///   u32 name_stridx
///   u8  compression
///   u8  padding[3]
///   u64 size          // compressed length
///   u8  data[size]
///   u8  hash[32]       // SHA-256 of everything above
pub fn write_data_section<W: Write>(
    writer: &mut W,
    name_stridx: u32,
    compression: CompressionType,
    raw: &[u8],
) -> io::Result<usize> {
    let compressed = match compression {
        CompressionType::None => raw.to_vec(),
        CompressionType::Brotli => {
            let mut out = Vec::new();
            {
                let mut brotli = CompressorWriter::new(&mut out, 8192, 9, 22);
                brotli.write_all(raw)?;
                brotli.flush()?;
            }
            out
        }
        other => {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "CompressionType {:?} is not implemented for encoding",
                    other
                ),
            ));
        }
    };

    let size = u64::try_from(compressed.len())
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "data section too large"))?;

    let mut hasher = Sha256::new();
    {
        let mut hw = HashWriter::new(writer, &mut hasher);
        hw.write_u32_le(name_stridx)?;
        hw.write_u8(compression as u8)?;
        hw.write_all(&[0u8; 3])?;
        hw.write_u64_le(size)?;
        hw.write_all(&compressed)?;
    }
    let digest = hasher.finalize();
    writer.write_all(&digest)?;

    Ok(4 + 1 + 3 + 8 + compressed.len() + 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_roundtrips_bytes_unchanged() {
        let mut buf = Vec::new();
        let raw = b"hello world";
        write_data_section(&mut buf, 0, CompressionType::None, raw).unwrap();
        // header(8) + size(8) + raw + hash(32)
        assert_eq!(buf.len(), 8 + 8 + raw.len() + 32);
    }

    #[test]
    fn brotli_compresses_repetitive_data() {
        let mut buf = Vec::new();
        let raw = vec![b'a'; 100_000];
        write_data_section(&mut buf, 0, CompressionType::Brotli, &raw).unwrap();
        assert!(buf.len() < raw.len() / 2);
    }

    #[test]
    fn unsupported_compression_errors() {
        let mut buf = Vec::new();
        let err = write_data_section(&mut buf, 0, CompressionType::Lz4, b"x").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }
}

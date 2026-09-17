use bpfs_core::types::enums::CompressionType;
use brotli::Decompressor;
use sha2::{Digest, Sha256};
use std::io::{self, Error, ErrorKind, Read};

const BLOCK_SIZE: usize = 16 * 1024;

pub struct DecodedDataSection {
    pub name_stridx: u32,
    pub compression: CompressionType,
    pub data: Vec<u8>,
}

/// Reads a DataSection written by `bpfs_pack::encode::data::write_data_section`.
///
/// Layout: `u32 name_stridx, u8 compression, u8 pad[3], u64 size, u8 data[size], u8 hash[32]`.
pub fn read_data_section<R: Read>(reader: &mut R) -> io::Result<DecodedDataSection> {
    let mut hasher = Sha256::new();

    let mut header = [0u8; 4 + 1 + 3 + 8];
    reader.read_exact(&mut header)?;
    hasher.update(header);

    let name_stridx = u32::from_le_bytes(header[0..4].try_into().unwrap());
    let comp_byte = header[4];
    let compression = CompressionType::try_from(comp_byte).map_err(|b| {
        Error::new(
            ErrorKind::InvalidData,
            format!("unknown compression type {b}"),
        )
    })?;
    let size = u64::from_le_bytes(header[8..16].try_into().unwrap());

    let mut compressed = vec![0u8; size as usize];
    reader.read_exact(&mut compressed)?;
    hasher.update(&compressed);

    let mut expected_hash = [0u8; 32];
    reader.read_exact(&mut expected_hash)?;
    let actual_hash = hasher.finalize();
    if actual_hash[..] != expected_hash {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "DataSection checksum mismatch",
        ));
    }

    let data = match compression {
        CompressionType::None => compressed,
        CompressionType::Brotli => {
            let mut out = Vec::new();
            Decompressor::new(&compressed[..], BLOCK_SIZE).read_to_end(&mut out)?;
            out
        }
        other => {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("CompressionType {other:?} is not implemented for decoding"),
            ));
        }
    };

    Ok(DecodedDataSection {
        name_stridx,
        compression,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpfs_pack::encode::data::write_data_section;

    #[test]
    fn roundtrip_none() {
        let mut buf = Vec::new();
        write_data_section(&mut buf, 3, CompressionType::None, b"hello world").unwrap();
        let section = read_data_section(&mut &buf[..]).unwrap();
        assert_eq!(section.name_stridx, 3);
        assert_eq!(section.compression, CompressionType::None);
        assert_eq!(section.data, b"hello world");
    }

    #[test]
    fn roundtrip_brotli() {
        let raw = vec![b'z'; 50_000];
        let mut buf = Vec::new();
        write_data_section(&mut buf, 1, CompressionType::Brotli, &raw).unwrap();
        let section = read_data_section(&mut &buf[..]).unwrap();
        assert_eq!(section.data, raw);
    }

    #[test]
    fn empty_payload_roundtrips() {
        let mut buf = Vec::new();
        write_data_section(&mut buf, 0, CompressionType::None, b"").unwrap();
        let section = read_data_section(&mut &buf[..]).unwrap();
        assert!(section.data.is_empty());
    }

    #[test]
    fn detects_corruption() {
        let mut buf = Vec::new();
        write_data_section(&mut buf, 0, CompressionType::None, b"payload").unwrap();
        let mid = buf.len() / 2;
        buf[mid] ^= 0xFF;
        assert!(read_data_section(&mut &buf[..]).is_err());
    }

    #[test]
    fn truncated_input_errors() {
        let mut buf = Vec::new();
        write_data_section(&mut buf, 0, CompressionType::None, b"payload").unwrap();
        buf.truncate(buf.len() - 5);
        assert!(read_data_section(&mut &buf[..]).is_err());
    }
}

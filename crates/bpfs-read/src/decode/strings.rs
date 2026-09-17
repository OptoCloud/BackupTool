use brotli::Decompressor;
use sha2::{Digest, Sha256};
use std::io::{self, Error, ErrorKind, Read};

const BLOCK_SIZE: usize = 16 * 1024;

struct HasherSink<'a> {
    hasher: &'a mut Sha256,
    bytes: Vec<u8>,
}
impl<'a> io::Write for HasherSink<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.hasher.update(buf);
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Reads a StringSection written by `bpfs_pack::encode::strings::write_string_section`.
///
/// Layout: `u32 count, StringEntry lookup[count], u32 uncompressed_size,
/// u32 stored_size, u8 chunk[stored_size], u8 hash[32]`.
pub fn read_string_section<R: Read>(reader: &mut R) -> io::Result<Vec<String>> {
    let mut hasher = Sha256::new();
    let mut sink = HasherSink {
        hasher: &mut hasher,
        bytes: Vec::new(),
    };

    let count = {
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf)?;
        io::Write::write_all(&mut sink, &buf)?;
        u32::from_le_bytes(buf) as usize
    };

    let mut lookup = Vec::with_capacity(count);
    for _ in 0..count {
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        io::Write::write_all(&mut sink, &buf)?;
        let offset = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        let length = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        lookup.push((offset, length));
    }

    let (uncompressed_size, stored_size) = {
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        io::Write::write_all(&mut sink, &buf)?;
        (
            u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            u32::from_le_bytes(buf[4..8].try_into().unwrap()),
        )
    };

    let mut chunk = vec![0u8; stored_size as usize];
    reader.read_exact(&mut chunk)?;
    io::Write::write_all(&mut sink, &chunk)?;

    let mut expected_hash = [0u8; 32];
    reader.read_exact(&mut expected_hash)?;
    let actual_hash = hasher.finalize();
    if actual_hash[..] != expected_hash {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "StringSection checksum mismatch",
        ));
    }

    let mut heap = Vec::with_capacity(uncompressed_size as usize);
    Decompressor::new(&chunk[..], BLOCK_SIZE).read_to_end(&mut heap)?;
    if heap.len() != uncompressed_size as usize {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "string heap size mismatch",
        ));
    }

    let mut strings = Vec::with_capacity(count);
    for (offset, length) in lookup {
        let start = offset as usize;
        let end = start
            .checked_add(length as usize)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "string range overflow"))?;
        if end > heap.len() {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "string data out of range",
            ));
        }
        strings.push(
            String::from_utf8(heap[start..end].to_vec())
                .map_err(|_| Error::new(ErrorKind::InvalidData, "invalid UTF-8 in string heap"))?,
        );
    }

    Ok(strings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpfs_pack::encode::strings::write_string_section;

    #[test]
    fn roundtrip_basic_strings() {
        let mut buf = Vec::new();
        write_string_section(&mut buf, &["hello", "world", ""]).unwrap();
        let strings = read_string_section(&mut &buf[..]).unwrap();
        assert_eq!(
            strings,
            vec!["hello".to_string(), "world".to_string(), "".to_string()]
        );
    }

    #[test]
    fn roundtrip_empty() {
        let mut buf = Vec::new();
        write_string_section(&mut buf, &[]).unwrap();
        let strings = read_string_section(&mut &buf[..]).unwrap();
        assert!(strings.is_empty());
    }

    #[test]
    fn detects_corruption() {
        let mut buf = Vec::new();
        write_string_section(&mut buf, &["hello"]).unwrap();
        let last = buf.len() - 1;
        buf[last] ^= 0xFF;
        assert!(read_string_section(&mut &buf[..]).is_err());
    }

    #[test]
    fn roundtrip_many_strings() {
        let owned: Vec<String> = (0..500).map(|i| format!("string_{i}")).collect();
        let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
        let mut buf = Vec::new();
        write_string_section(&mut buf, &refs).unwrap();
        let strings = read_string_section(&mut &buf[..]).unwrap();
        assert_eq!(strings, owned);
    }

    #[test]
    fn roundtrip_utf8() {
        let mut buf = Vec::new();
        write_string_section(&mut buf, &["🌍🚀", "café", "普通话"]).unwrap();
        let strings = read_string_section(&mut &buf[..]).unwrap();
        assert_eq!(
            strings,
            vec!["🌍🚀".to_string(), "café".to_string(), "普通话".to_string()]
        );
    }
}

use sha2::Digest;
use sha2::Sha256;
use std::io::{self, Read};

/// Reader wrapper that feeds every byte it reads into a running SHA-256 hash.
pub struct TeeHashReader<'a, R: Read> {
    inner: &'a mut R,
    hasher: &'a mut Sha256,
}

impl<'a, R: Read> TeeHashReader<'a, R> {
    pub fn new(inner: &'a mut R, hasher: &'a mut Sha256) -> Self {
        Self { inner, hasher }
    }
}

impl<'a, R: Read> Read for TeeHashReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
}

/// Little-endian integer reads for any `Read`.
pub trait ReadLeExt: Read {
    fn read_u16_le(&mut self) -> io::Result<u16> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_u32_le(&mut self) -> io::Result<u32> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_u64_le(&mut self) -> io::Result<u64> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }
}

impl<R: Read + ?Sized> ReadLeExt for R {}

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

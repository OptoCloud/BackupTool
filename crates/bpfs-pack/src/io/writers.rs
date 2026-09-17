#![allow(dead_code)] // full writer variants kept for external/future use

use sha2::{Digest, Sha256};
use std::io::{self, Write};

/// Hash-only writer: forwards to `inner` and updates `hasher`.
pub struct HashWriter<'a, W: Write> {
    inner: &'a mut W,
    hasher: &'a mut Sha256,
}

impl<'a, W: Write> HashWriter<'a, W> {
    pub fn new(inner: &'a mut W, hasher: &'a mut Sha256) -> Self {
        Self { inner, hasher }
    }
    /// Direct access to the underlying writer (no hashing).
    pub fn inner(&mut self) -> &mut W {
        self.inner
    }
    /// Consume and return the inner writer + hasher.
    pub fn into_inner(self) -> (&'a mut W, &'a mut Sha256) {
        (self.inner, self.hasher)
    }
}

impl<'a, W: Write> Write for HashWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Count-only writer: forwards to `inner` and tracks bytes written.
pub struct CountingWriter<'a, W: Write> {
    inner: &'a mut W,
    written: usize,
}

impl<'a, W: Write> CountingWriter<'a, W> {
    pub fn new(inner: &'a mut W) -> Self {
        Self { inner, written: 0 }
    }
    pub fn bytes_written(&self) -> usize {
        self.written
    }
    pub fn inner(&mut self) -> &mut W {
        self.inner
    }
    pub fn into_inner(self) -> (&'a mut W, usize) {
        (self.inner, self.written)
    }
}

impl<'a, W: Write> Write for CountingWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.written = self
            .written
            .checked_add(n)
            .ok_or_else(|| io::Error::other("CountingWriter byte count overflow"))?;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Hash + count writer: updates `hasher` and counts bytes written.
pub struct CountingHashWriter<'a, W: Write> {
    inner: &'a mut W,
    hasher: &'a mut Sha256,
    written: usize,
}

impl<'a, W: Write> CountingHashWriter<'a, W> {
    pub fn new(inner: &'a mut W, hasher: &'a mut Sha256) -> Self {
        Self {
            inner,
            hasher,
            written: 0,
        }
    }
    pub fn bytes_written(&self) -> usize {
        self.written
    }
    pub fn inner(&mut self) -> &mut W {
        self.inner
    }
    pub fn into_inner(self) -> (&'a mut W, &'a mut Sha256, usize) {
        (self.inner, self.hasher, self.written)
    }
}

impl<'a, W: Write> Write for CountingHashWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.written = self
            .written
            .checked_add(n)
            .ok_or_else(|| io::Error::other("CountingHashWriter byte count overflow"))?;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

use std::io::{self, Write};

/// Little-endian integer writes for any `Write`.
pub trait WriteLeExt: Write {
    fn write_u8(&mut self, v: u8) -> io::Result<()> {
        self.write_all(&[v])
    }

    fn write_u16_le(&mut self, v: u16) -> io::Result<()> {
        self.write_all(&v.to_le_bytes())
    }

    fn write_u32_le(&mut self, v: u32) -> io::Result<()> {
        self.write_all(&v.to_le_bytes())
    }

    fn write_u64_le(&mut self, v: u64) -> io::Result<()> {
        self.write_all(&v.to_le_bytes())
    }
}

impl<W: Write + ?Sized> WriteLeExt for W {}

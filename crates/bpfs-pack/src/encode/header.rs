use bpfs_core::constants::{MAGIC, VERSION};
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::{self, Write};

/// BPFS archive header layout:
///   MAGIC[4]
///   u16 version
///   u16 flags
///   u8[8] reserved
///
/// Always 16 bytes total.
pub fn write_header<W: Write>(mut sink: W, flags: u16) -> io::Result<()> {
    sink.write_all(&MAGIC)?; // 4 bytes
    sink.write_u16::<LittleEndian>(VERSION)?; // 2 bytes
    sink.write_u16::<LittleEndian>(flags)?; // 2 bytes
    sink.write_all(&[0u8; 8])?; // 8 bytes reserved
    Ok(())
}

pub const HEADER_SIZE: usize = 16;

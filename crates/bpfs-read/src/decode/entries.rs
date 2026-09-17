use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{self, Read};

use bpfs_core::types::packed::{BlobEntry, DirectoryEntry, FileEntry};

pub fn read_blob_entry<R: Read>(r: &mut R) -> io::Result<BlobEntry> {
    Ok(BlobEntry {
        raw_size: r.read_u64::<LittleEndian>()?,
        generation_idx: r.read_u32::<LittleEndian>()?,
        section_idx: r.read_u32::<LittleEndian>()?,
        section_blob_idx: r.read_u32::<LittleEndian>()?,
    })
}

pub fn read_directory_entry<R: Read>(r: &mut R) -> io::Result<DirectoryEntry> {
    Ok(DirectoryEntry {
        parent_id: r.read_u32::<LittleEndian>()?,
        name_stridx: r.read_u32::<LittleEndian>()?,
        created_at: r.read_u64::<LittleEndian>()?,
        modified_at: r.read_u64::<LittleEndian>()?,
    })
}

pub fn read_file_entry<R: Read>(r: &mut R) -> io::Result<FileEntry> {
    Ok(FileEntry {
        blob_idx: r.read_u32::<LittleEndian>()?,
        dir_idx: r.read_u32::<LittleEndian>()?,
        name_stridx: r.read_u32::<LittleEndian>()?,
        created_at: r.read_u64::<LittleEndian>()?,
        modified_at: r.read_u64::<LittleEndian>()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpfs_pack::encode::entries::{write_blob_entry, write_directory_entry, write_file_entry};

    #[test]
    fn blob_entry_roundtrip() {
        let e = BlobEntry {
            raw_size: 12345,
            generation_idx: 1,
            section_idx: 2,
            section_blob_idx: 3,
        };
        let mut buf = Vec::new();
        write_blob_entry(&mut buf, &e).unwrap();
        let got = read_blob_entry(&mut &buf[..]).unwrap();
        assert_eq!(e, got);
    }

    #[test]
    fn directory_entry_roundtrip() {
        let e = DirectoryEntry {
            parent_id: 7,
            name_stridx: 2,
            created_at: 111,
            modified_at: 222,
        };
        let mut buf = Vec::new();
        write_directory_entry(&mut buf, &e).unwrap();
        let got = read_directory_entry(&mut &buf[..]).unwrap();
        assert_eq!(e, got);
    }

    #[test]
    fn file_entry_roundtrip() {
        let e = FileEntry {
            blob_idx: 4,
            dir_idx: 5,
            name_stridx: 6,
            created_at: 333,
            modified_at: 444,
        };
        let mut buf = Vec::new();
        write_file_entry(&mut buf, &e).unwrap();
        let got = read_file_entry(&mut &buf[..]).unwrap();
        assert_eq!(e, got);
    }
}

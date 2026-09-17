use byteorder::{LittleEndian, ReadBytesExt};
use sha2::{Digest, Sha256};
use std::io::Read;

use bpfs_core::constants::GENERATION_SUFFIX;
use bpfs_core::errors::{ArchiveError, Result};
use bpfs_core::strings::StringTable;
use bpfs_core::types::packed::{BlobEntry, DirectoryEntry, FileEntry};
use bpfs_core::validate::{validate_directory_tree, validate_file_refs};

use crate::decode::data::DecodedDataSection;
use crate::decode::{data, entries, strings};
use crate::io::TeeHashReader;

pub struct Generation {
    pub created_at: u64,
    pub strings: StringTable,
    pub blobs: Vec<BlobEntry>,
    pub dirs: Vec<DirectoryEntry>,
    pub files: Vec<FileEntry>,
    pub data_sections: Vec<DecodedDataSection>,
    pub blob_hashes: Vec<[u8; 32]>,
    pub previous_integrity_hash: [u8; 32],
    pub integrity_hash: [u8; 32],
    pub signature_type: u32,
    pub signature: Vec<u8>,
}

/// Reads one `Generation` record as written by
/// `bpfs_pack::encode::generation::write_generation`, verifying its
/// `integrity_hash` and structural consistency along the way.
pub fn read_generation<R: Read>(reader: &mut R) -> Result<Generation> {
    let mut hasher = Sha256::new();

    let created_at;
    let string_list;
    let blobs;
    let dirs;
    let files;
    let data_sections;
    let blob_hashes;
    let previous_integrity_hash;

    {
        let mut tee = TeeHashReader::new(reader, &mut hasher);

        created_at = tee.read_u64::<LittleEndian>()?;
        let file_count = tee.read_u32::<LittleEndian>()? as usize;
        let dir_count = tee.read_u32::<LittleEndian>()? as usize;
        let blob_count = tee.read_u32::<LittleEndian>()? as usize;

        string_list = strings::read_string_section(&mut tee)?;

        blobs = (0..blob_count)
            .map(|_| entries::read_blob_entry(&mut tee))
            .collect::<std::io::Result<Vec<_>>>()?;
        dirs = (0..dir_count)
            .map(|_| entries::read_directory_entry(&mut tee))
            .collect::<std::io::Result<Vec<_>>>()?;
        files = (0..file_count)
            .map(|_| entries::read_file_entry(&mut tee))
            .collect::<std::io::Result<Vec<_>>>()?;

        let data_section_count = tee.read_u32::<LittleEndian>()? as usize;
        data_sections = (0..data_section_count)
            .map(|_| data::read_data_section(&mut tee))
            .collect::<std::io::Result<Vec<_>>>()?;

        blob_hashes = (0..blob_count)
            .map(|_| {
                let mut h = [0u8; 32];
                tee.read_exact(&mut h)?;
                Ok::<_, std::io::Error>(h)
            })
            .collect::<std::io::Result<Vec<_>>>()?;

        let mut prev = [0u8; 32];
        tee.read_exact(&mut prev)?;
        previous_integrity_hash = prev;
    }

    let computed_integrity_hash: [u8; 32] = hasher.finalize().into();

    let mut stored_integrity_hash = [0u8; 32];
    reader.read_exact(&mut stored_integrity_hash)?;
    if computed_integrity_hash != stored_integrity_hash {
        return Err(ArchiveError::HashMismatch("generation integrity_hash"));
    }

    let signature_type = reader.read_u32::<LittleEndian>()?;
    let signature_size = reader.read_u32::<LittleEndian>()?;
    if signature_size > 4096 {
        return Err(ArchiveError::Format("implausible signature size".into()));
    }
    let mut signature = vec![0u8; signature_size as usize];
    reader.read_exact(&mut signature)?;

    let suffix = reader.read_u32::<LittleEndian>()?;
    if suffix != GENERATION_SUFFIX {
        return Err(ArchiveError::Format(format!(
            "bad generation suffix marker: {suffix:#x}"
        )));
    }

    validate_directory_tree(&dirs)?;
    validate_file_refs(&files, dirs.len(), blobs.len())?;

    if blob_hashes.len() != blobs.len() {
        return Err(ArchiveError::Format(
            "blob_hashes/blobs length mismatch".into(),
        ));
    }

    Ok(Generation {
        created_at,
        strings: StringTable::new(string_list),
        blobs,
        dirs,
        files,
        data_sections,
        blob_hashes,
        previous_integrity_hash,
        integrity_hash: computed_integrity_hash,
        signature_type,
        signature,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpfs_core::types::enums::CompressionType;
    use bpfs_pack::encode::generation::{write_generation, GenerationInput, PendingDataSection};

    fn sample_input() -> (
        Vec<BlobEntry>,
        Vec<[u8; 32]>,
        Vec<DirectoryEntry>,
        Vec<FileEntry>,
    ) {
        let blobs = vec![BlobEntry {
            raw_size: 5,
            generation_idx: 0,
            section_idx: 0,
            section_blob_idx: 0,
        }];
        let blob_hashes = vec![[1u8; 32]];
        let dirs = vec![DirectoryEntry {
            parent_id: bpfs_core::types::packed::NO_PARENT,
            name_stridx: 0,
            created_at: 0,
            modified_at: 0,
        }];
        let files = vec![FileEntry {
            blob_idx: 0,
            dir_idx: 0,
            name_stridx: 1,
            created_at: 0,
            modified_at: 0,
        }];
        (blobs, blob_hashes, dirs, files)
    }

    #[test]
    fn roundtrip_unsigned_generation() {
        let (blobs, blob_hashes, dirs, files) = sample_input();
        let strings = ["", "a.txt"];
        let data_sections = vec![
            PendingDataSection {
                name_stridx: 0,
                compression: CompressionType::None,
                raw: b"hello".to_vec(),
            },
            PendingDataSection {
                name_stridx: 0,
                compression: CompressionType::Brotli,
                raw: vec![],
            },
        ];
        let input = GenerationInput {
            created_at: 42,
            strings: &strings,
            blobs: &blobs,
            blob_hashes: &blob_hashes,
            dirs: &dirs,
            files: &files,
            data_sections: &data_sections,
            previous_integrity_hash: [0u8; 32],
            signing_key: None,
        };
        let mut buf = Vec::new();
        let out = write_generation(&mut buf, &input).unwrap();

        let gen = read_generation(&mut &buf[..]).unwrap();
        assert_eq!(gen.created_at, 42);
        assert_eq!(gen.blobs, blobs);
        assert_eq!(gen.dirs, dirs);
        assert_eq!(gen.files, files);
        assert_eq!(gen.blob_hashes, blob_hashes);
        assert_eq!(gen.integrity_hash, out.integrity_hash);
        assert_eq!(gen.signature_type, 0);
        assert!(gen.signature.is_empty());
        assert_eq!(gen.data_sections[0].data, b"hello");
    }

    #[test]
    fn roundtrip_signed_generation() {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        let (blobs, blob_hashes, dirs, files) = sample_input();
        let strings = ["", "a.txt"];
        let data_sections = vec![
            PendingDataSection {
                name_stridx: 0,
                compression: CompressionType::None,
                raw: b"hello".to_vec(),
            },
            PendingDataSection {
                name_stridx: 0,
                compression: CompressionType::Brotli,
                raw: vec![],
            },
        ];
        let key = SigningKey::generate(&mut OsRng);
        let input = GenerationInput {
            created_at: 1,
            strings: &strings,
            blobs: &blobs,
            blob_hashes: &blob_hashes,
            dirs: &dirs,
            files: &files,
            data_sections: &data_sections,
            previous_integrity_hash: [0u8; 32],
            signing_key: Some(&key),
        };
        let mut buf = Vec::new();
        write_generation(&mut buf, &input).unwrap();

        let gen = read_generation(&mut &buf[..]).unwrap();
        assert_eq!(gen.signature_type, 1);
        assert_eq!(gen.signature.len(), 64);
        assert!(bpfs_core::signing::verify_integrity_hash(
            &key.verifying_key(),
            &gen.integrity_hash,
            &gen.signature
        ));
    }

    #[test]
    fn corrupted_generation_fails_integrity_check() {
        let (blobs, blob_hashes, dirs, files) = sample_input();
        let strings = ["", "a.txt"];
        let data_sections = vec![
            PendingDataSection {
                name_stridx: 0,
                compression: CompressionType::None,
                raw: b"hello".to_vec(),
            },
            PendingDataSection {
                name_stridx: 0,
                compression: CompressionType::Brotli,
                raw: vec![],
            },
        ];
        let input = GenerationInput {
            created_at: 42,
            strings: &strings,
            blobs: &blobs,
            blob_hashes: &blob_hashes,
            dirs: &dirs,
            files: &files,
            data_sections: &data_sections,
            previous_integrity_hash: [0u8; 32],
            signing_key: None,
        };
        let mut buf = Vec::new();
        write_generation(&mut buf, &input).unwrap();

        // Flip a byte inside the file table region.
        buf[40] ^= 0xFF;
        assert!(read_generation(&mut &buf[..]).is_err());
    }

    #[test]
    fn truncated_generation_errors() {
        let (blobs, blob_hashes, dirs, files) = sample_input();
        let strings = ["", "a.txt"];
        let data_sections = vec![
            PendingDataSection {
                name_stridx: 0,
                compression: CompressionType::None,
                raw: b"hello".to_vec(),
            },
            PendingDataSection {
                name_stridx: 0,
                compression: CompressionType::Brotli,
                raw: vec![],
            },
        ];
        let input = GenerationInput {
            created_at: 42,
            strings: &strings,
            blobs: &blobs,
            blob_hashes: &blob_hashes,
            dirs: &dirs,
            files: &files,
            data_sections: &data_sections,
            previous_integrity_hash: [0u8; 32],
            signing_key: None,
        };
        let mut buf = Vec::new();
        write_generation(&mut buf, &input).unwrap();
        buf.truncate(buf.len() - 10);
        assert!(read_generation(&mut &buf[..]).is_err());
    }
}

use crate::io::ReadLeExt;
use std::io::{Read, Seek};

use bpfs_core::constants::GENERATION_SUFFIX;
use bpfs_core::errors::{ArchiveError, Result};
use bpfs_core::strings::StringTable;
use bpfs_core::types::packed::{BlobEntry, DirectoryEntry, FileEntry};
use bpfs_core::validate::{validate_directory_tree, validate_file_refs};

use crate::decode::data::{index_data_section, SectionIndex};
use crate::decode::{entries, strings};

/// Offset value for blobs whose bytes live in another generation.
pub const NOT_OWNED: u64 = u64::MAX;

/// One generation's metadata. Data sections are indexed, not loaded.
pub struct Generation {
    pub created_at: u64,
    pub strings: StringTable,
    pub blobs: Vec<BlobEntry>,
    pub dirs: Vec<DirectoryEntry>,
    pub files: Vec<FileEntry>,
    pub data_sections: Vec<SectionIndex>,
    pub blob_hashes: Vec<[u8; 32]>,
    pub previous_integrity_hash: [u8; 32],
    /// As stored in the archive; checked by `verify::verify_integrity`.
    pub integrity_hash: [u8; 32],
    pub signature_type: u32,
    pub signature: Vec<u8>,
    /// Archive offset where this generation starts.
    pub offset: u64,
    /// Number of bytes covered by `integrity_hash`, starting at `offset`.
    pub hashed_len: u64,
    /// For each blob stored in this generation, its offset within its data
    /// section's raw stream; `NOT_OWNED` for blobs stored elsewhere.
    pub blob_offsets: Vec<u64>,
    /// For each data section, the indices (into `blobs`) of the blobs stored
    /// in it, ordered by `section_blob_idx`.
    pub section_blobs: Vec<Vec<usize>>,
}

impl Generation {
    /// Bytes of blob data physically stored in this generation.
    pub fn owned_blob_bytes(&self) -> u64 {
        self.blobs
            .iter()
            .zip(&self.blob_offsets)
            .filter(|(_, &off)| off != NOT_OWNED)
            .map(|(b, _)| b.raw_size)
            .sum()
    }
}

/// Reads the metadata of the generation at the reader's position (index
/// `gen_idx` in the archive) as written by
/// `bpfs_pack::encode::generation::write_generation`, leaving the reader just
/// past it. Data blocks are skipped and hashes are not recomputed.
pub fn read_generation<R: Read + Seek>(reader: &mut R, gen_idx: u32) -> Result<Generation> {
    let offset = reader.stream_position()?;

    let created_at = reader.read_u64_le()?;
    let file_count = reader.read_u32_le()? as usize;
    let dir_count = reader.read_u32_le()? as usize;
    let blob_count = reader.read_u32_le()? as usize;

    let string_list = strings::read_string_section(reader)?;

    let blobs = (0..blob_count)
        .map(|_| entries::read_blob_entry(reader))
        .collect::<std::io::Result<Vec<_>>>()?;
    let dirs = (0..dir_count)
        .map(|_| entries::read_directory_entry(reader))
        .collect::<std::io::Result<Vec<_>>>()?;
    let files = (0..file_count)
        .map(|_| entries::read_file_entry(reader))
        .collect::<std::io::Result<Vec<_>>>()?;

    let data_section_count = reader.read_u32_le()? as usize;
    if data_section_count > 64 {
        return Err(ArchiveError::Format(
            "implausible data section count".into(),
        ));
    }
    let data_sections = (0..data_section_count)
        .map(|_| index_data_section(reader))
        .collect::<std::io::Result<Vec<_>>>()?;

    let blob_hashes = (0..blob_count)
        .map(|_| {
            let mut h = [0u8; 32];
            reader.read_exact(&mut h)?;
            Ok::<_, std::io::Error>(h)
        })
        .collect::<std::io::Result<Vec<_>>>()?;

    let mut previous_integrity_hash = [0u8; 32];
    reader.read_exact(&mut previous_integrity_hash)?;
    let hashed_len = reader.stream_position()? - offset;

    let mut integrity_hash = [0u8; 32];
    reader.read_exact(&mut integrity_hash)?;

    let signature_type = reader.read_u32_le()?;
    let signature_size = reader.read_u32_le()?;
    if signature_size > 4096 {
        return Err(ArchiveError::Format("implausible signature size".into()));
    }
    let mut signature = vec![0u8; signature_size as usize];
    reader.read_exact(&mut signature)?;

    let suffix = reader.read_u32_le()?;
    if suffix != GENERATION_SUFFIX {
        return Err(ArchiveError::Format(format!(
            "bad generation suffix marker: {suffix:#x}"
        )));
    }

    validate_directory_tree(&dirs)?;
    validate_file_refs(&files, dirs.len(), blobs.len())?;
    let (blob_offsets, section_blobs) = locate_blobs(&blobs, &data_sections, gen_idx)?;

    Ok(Generation {
        created_at,
        strings: StringTable::new(string_list),
        blobs,
        dirs,
        files,
        data_sections,
        blob_hashes,
        previous_integrity_hash,
        integrity_hash,
        signature_type,
        signature,
        offset,
        hashed_len,
        blob_offsets,
        section_blobs,
    })
}

/// Computes where each blob owned by this generation starts within its data
/// section: blobs are packed back-to-back in `section_blob_idx` order.
fn locate_blobs(
    blobs: &[BlobEntry],
    sections: &[SectionIndex],
    gen_idx: u32,
) -> Result<(Vec<u64>, Vec<Vec<usize>>)> {
    let mut offsets = vec![NOT_OWNED; blobs.len()];
    let mut owned: Vec<usize> = Vec::new();
    for (i, b) in blobs.iter().enumerate() {
        if b.generation_idx > gen_idx {
            return Err(ArchiveError::Format(
                "blob refers to a later generation".into(),
            ));
        }
        if b.generation_idx == gen_idx {
            if b.section_idx as usize >= sections.len() {
                return Err(ArchiveError::IndexOutOfBounds("BlobEntry.section_idx"));
            }
            owned.push(i);
        }
    }
    owned.sort_by_key(|&i| (blobs[i].section_idx, blobs[i].section_blob_idx));

    let mut section_end = vec![0u64; sections.len()];
    let mut section_blobs: Vec<Vec<usize>> = vec![Vec::new(); sections.len()];
    for i in owned {
        let b = &blobs[i];
        let s = b.section_idx as usize;
        if b.section_blob_idx as usize != section_blobs[s].len() {
            return Err(ArchiveError::Format(
                "section_blob_idx values are not contiguous".into(),
            ));
        }
        section_blobs[s].push(i);
        offsets[i] = section_end[s];
        section_end[s] = section_end[s]
            .checked_add(b.raw_size)
            .ok_or_else(|| ArchiveError::Format("blob size overflow".into()))?;
    }

    for (s, section) in sections.iter().enumerate() {
        if section_end[s] != section.raw_size {
            return Err(ArchiveError::Format(format!(
                "data section {s} holds {} bytes but its blobs need {}",
                section.raw_size, section_end[s]
            )));
        }
    }
    Ok((offsets, section_blobs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpfs_core::types::enums::CompressionType;
    use bpfs_pack::encode::generation::{write_generation, GenerationInput, SectionSpec};
    use std::io::Cursor;

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

    const SECTIONS: [SectionSpec; 2] = [
        SectionSpec {
            name_stridx: 0,
            compression: CompressionType::None,
        },
        SectionSpec {
            name_stridx: 0,
            compression: CompressionType::Zstd,
        },
    ];

    fn write_sample(signing_key: Option<&ed25519_dalek::SigningKey>) -> (Vec<u8>, [u8; 32]) {
        let (blobs, blob_hashes, dirs, files) = sample_input();
        let strings = ["", "a.txt"];
        let input = GenerationInput {
            created_at: 42,
            strings: &strings,
            blobs: &blobs,
            blob_hashes: &blob_hashes,
            dirs: &dirs,
            files: &files,
            data_sections: &SECTIONS,
            block_size: 4,
            previous_integrity_hash: [0u8; 32],
            signing_key,
        };
        let mut buf = Vec::new();
        let mut contents = vec![b"hello".to_vec(), Vec::new()];
        let out = write_generation(&mut buf, &input, &mut contents).unwrap();
        assert_eq!(out.bytes_written, buf.len());
        (buf, out.integrity_hash)
    }

    #[test]
    fn roundtrip_unsigned_generation() {
        let (buf, integrity_hash) = write_sample(None);
        let (blobs, blob_hashes, dirs, files) = sample_input();

        let mut cursor = Cursor::new(buf);
        let gen = read_generation(&mut cursor, 0).unwrap();
        assert_eq!(cursor.position(), cursor.get_ref().len() as u64);
        assert_eq!(gen.created_at, 42);
        assert_eq!(gen.blobs, blobs);
        assert_eq!(gen.dirs, dirs);
        assert_eq!(gen.files, files);
        assert_eq!(gen.blob_hashes, blob_hashes);
        assert_eq!(gen.integrity_hash, integrity_hash);
        assert_eq!(gen.signature_type, 0);
        assert!(gen.signature.is_empty());
        assert_eq!(gen.data_sections[0].raw_size, 5);
        assert_eq!(gen.data_sections[0].blocks.len(), 2); // block_size 4
        assert_eq!(gen.blob_offsets, [0]);
        assert_eq!(gen.offset, 0);
    }

    #[test]
    fn roundtrip_signed_generation() {
        use ed25519_dalek::SigningKey;

        let key = SigningKey::generate(&mut rand::rng());
        let (buf, _) = write_sample(Some(&key));

        let gen = read_generation(&mut Cursor::new(buf), 0).unwrap();
        assert_eq!(gen.signature_type, 1);
        assert_eq!(gen.signature.len(), 64);
        assert!(bpfs_core::signing::verify_integrity_hash(
            &key.verifying_key(),
            &gen.integrity_hash,
            &gen.signature
        ));
    }

    #[test]
    fn section_bytes_without_owning_blobs_are_rejected() {
        let (buf, _) = write_sample(None);
        // Parsed as generation 1, the blob belongs to generation 0, so the 5
        // bytes in section 0 are unaccounted for.
        assert!(read_generation(&mut Cursor::new(buf), 1).is_err());
    }

    #[test]
    fn truncated_generation_errors() {
        let (mut buf, _) = write_sample(None);
        buf.truncate(buf.len() - 10);
        assert!(read_generation(&mut Cursor::new(buf), 0).is_err());
    }
}

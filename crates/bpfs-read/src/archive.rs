use crate::io::ReadLeExt;
use std::io::{Read, Seek, SeekFrom};

use bpfs_core::constants::MAGIC;
use bpfs_core::errors::{ArchiveError, Result};
use bpfs_core::types::packed::BlobEntry;

use crate::generation::{read_generation, Generation};

pub struct Archive {
    pub version: u16,
    pub flags: u16,
    pub generations: Vec<Generation>,
}

/// Parses a full BPFS archive: the header, then every `Generation` record up
/// to EOF.
pub fn read_archive<R: Read + Seek>(mut reader: R) -> Result<Archive> {
    let start = reader.stream_position()?;
    let total_len = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(start))?;

    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(ArchiveError::Format("invalid BPFS magic".into()));
    }
    let version = reader.read_u16_le()?;
    let flags = reader.read_u16_le()?;
    let mut reserved = [0u8; 8];
    reader.read_exact(&mut reserved)?;

    let mut generations = Vec::new();
    loop {
        let pos = reader.stream_position()?;
        if pos >= total_len {
            break;
        }
        generations.push(read_generation(&mut reader)?);
    }

    if generations.is_empty() {
        return Err(ArchiveError::Format(
            "archive contains no generations".into(),
        ));
    }

    for (idx, gen) in generations.iter().enumerate() {
        let expected_previous = if idx == 0 {
            [0u8; 32]
        } else {
            generations[idx - 1].integrity_hash
        };
        if gen.previous_integrity_hash != expected_previous {
            return Err(ArchiveError::HashMismatch("generation hash chain"));
        }
    }

    Ok(Archive {
        version,
        flags,
        generations,
    })
}

impl Archive {
    pub fn latest(&self) -> &Generation {
        self.generations
            .last()
            .expect("archives always have at least one generation")
    }

    /// Resolves a blob's raw bytes from wherever it actually lives (its own
    /// generation may not be the newest one, if the blob was deduplicated
    /// against an earlier snapshot).
    pub fn blob_bytes(&self, blob: &BlobEntry) -> Result<&[u8]> {
        let owner = self
            .generations
            .get(blob.generation_idx as usize)
            .ok_or(ArchiveError::IndexOutOfBounds("BlobEntry.generation_idx"))?;
        let section = owner
            .data_sections
            .get(blob.section_idx as usize)
            .ok_or(ArchiveError::IndexOutOfBounds("BlobEntry.section_idx"))?;

        let mut offset: u64 = 0;
        for b in &owner.blobs {
            if b.section_idx == blob.section_idx && b.section_blob_idx < blob.section_blob_idx {
                offset += b.raw_size;
            }
        }

        let start = offset as usize;
        let end = start
            .checked_add(blob.raw_size as usize)
            .ok_or(ArchiveError::Format("blob range overflow".into()))?;
        section.data.get(start..end).ok_or(ArchiveError::Format(
            "blob range exceeds data section bounds".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    use bpfs_pack::pack::{
        pack_directory, ExistingBlob, ExistingBlobLookup, NoExistingBlobs, PackOptions,
    };
    use bpfs_pack::policy::compression::DefaultCompressionPolicy;
    use std::fs;
    use tempfile::tempdir;

    fn policy() -> DefaultCompressionPolicy {
        DefaultCompressionPolicy {
            incompressible_entropy: 96.25,
        }
    }

    #[test]
    fn parses_single_generation_archive() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"hello world").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.bin"), b"\x00\x01binary").unwrap();

        let p = policy();
        let mut buf = Vec::new();
        pack_directory(
            &mut buf,
            dir.path(),
            true,
            PackOptions {
                generation_idx: 0,
                previous_integrity_hash: [0u8; 32],
                policy: &p,
                signing_key: None,
                existing_blobs: &NoExistingBlobs,
            },
        )
        .unwrap();

        let archive = read_archive(Cursor::new(buf)).unwrap();
        assert_eq!(archive.generations.len(), 1);
        assert_eq!(archive.latest().files.len(), 2);
    }

    #[test]
    fn rejects_bad_magic() {
        let buf = vec![0u8; 32];
        assert!(read_archive(Cursor::new(buf)).is_err());
    }

    #[test]
    fn blob_bytes_resolves_correct_slice() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"AAAA").unwrap();
        fs::write(dir.path().join("b.txt"), b"BBBBBB").unwrap();

        let p = policy();
        let mut buf = Vec::new();
        pack_directory(
            &mut buf,
            dir.path(),
            true,
            PackOptions {
                generation_idx: 0,
                previous_integrity_hash: [0u8; 32],
                policy: &p,
                signing_key: None,
                existing_blobs: &NoExistingBlobs,
            },
        )
        .unwrap();

        let archive = read_archive(Cursor::new(buf)).unwrap();
        let gen = archive.latest();
        let mut seen = std::collections::HashSet::new();
        for f in &gen.files {
            let blob = &gen.blobs[f.blob_idx as usize];
            let bytes = archive.blob_bytes(blob).unwrap();
            assert_eq!(bytes.len(), blob.raw_size as usize);
            seen.insert(bytes.to_vec());
        }
        assert!(seen.contains(b"AAAA".as_slice()));
        assert!(seen.contains(b"BBBBBB".as_slice()));
    }

    #[test]
    fn multi_generation_chain_verifies() {
        let dir1 = tempdir().unwrap();
        fs::write(dir1.path().join("a.txt"), b"generation zero").unwrap();

        let p = policy();
        let mut buf = Vec::new();
        let summary0 = pack_directory(
            &mut buf,
            dir1.path(),
            true,
            PackOptions {
                generation_idx: 0,
                previous_integrity_hash: [0u8; 32],
                policy: &p,
                signing_key: None,
                existing_blobs: &NoExistingBlobs,
            },
        )
        .unwrap();

        let dir2 = tempdir().unwrap();
        fs::write(dir2.path().join("b.txt"), b"generation one").unwrap();
        pack_directory(
            &mut buf,
            dir2.path(),
            false,
            PackOptions {
                generation_idx: 1,
                previous_integrity_hash: summary0.integrity_hash,
                policy: &p,
                signing_key: None,
                existing_blobs: &NoExistingBlobs,
            },
        )
        .unwrap();

        let archive = read_archive(Cursor::new(buf)).unwrap();
        assert_eq!(archive.generations.len(), 2);
        assert_eq!(
            archive.generations[1].previous_integrity_hash,
            archive.generations[0].integrity_hash
        );
    }

    #[test]
    fn cross_generation_blob_dedup_avoids_rewriting_bytes() {
        let dir1 = tempdir().unwrap();
        fs::write(dir1.path().join("a.txt"), b"shared content").unwrap();

        let p = policy();
        let mut buf = Vec::new();
        let summary0 = pack_directory(
            &mut buf,
            dir1.path(),
            true,
            PackOptions {
                generation_idx: 0,
                previous_integrity_hash: [0u8; 32],
                policy: &p,
                signing_key: None,
                existing_blobs: &NoExistingBlobs,
            },
        )
        .unwrap();

        let archive_gen0 = read_archive(Cursor::new(buf.clone())).unwrap();
        let hash_of = {
            let gen = archive_gen0.latest();
            gen.blob_hashes[0]
        };

        struct LookupGen0<'a> {
            hash: [u8; 32],
            blob: &'a BlobEntry,
        }
        impl<'a> ExistingBlobLookup for LookupGen0<'a> {
            fn find(&self, hash: &[u8; 32]) -> Option<ExistingBlob> {
                if *hash == self.hash {
                    Some(ExistingBlob {
                        generation_idx: self.blob.generation_idx,
                        section_idx: self.blob.section_idx,
                        section_blob_idx: self.blob.section_blob_idx,
                    })
                } else {
                    None
                }
            }
        }

        let dir2 = tempdir().unwrap();
        fs::write(dir2.path().join("a_again.txt"), b"shared content").unwrap();
        let lookup = LookupGen0 {
            hash: hash_of,
            blob: &archive_gen0.latest().blobs[0],
        };

        let summary1 = pack_directory(
            &mut buf,
            dir2.path(),
            false,
            PackOptions {
                generation_idx: 1,
                previous_integrity_hash: summary0.integrity_hash,
                policy: &p,
                signing_key: None,
                existing_blobs: &lookup,
            },
        )
        .unwrap();
        assert_eq!(summary1.new_blob_bytes, 0);

        let archive = read_archive(Cursor::new(buf)).unwrap();
        let gen1 = &archive.generations[1];
        let blob = &gen1.blobs[0];
        assert_eq!(blob.generation_idx, 0); // still points back at generation 0
        let bytes = archive.blob_bytes(blob).unwrap();
        assert_eq!(bytes, b"shared content");
    }
}

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use bpfs_core::errors::{ArchiveError, Result};
use bpfs_core::progress::{Item, Monitor, Phase, Progress};
use bpfs_core::signing::verify_integrity_hash;
use ed25519_dalek::VerifyingKey;

use crate::archive::Archive;
use crate::iter::current_files;

const CHUNK: usize = 1024 * 1024;

/// Re-hashes every generation's bytes on disk and compares them with the
/// stored `integrity_hash`. This covers all metadata and compressed data.
pub fn verify_integrity<R: Read + Seek>(archive: &Archive<R>, monitor: &dyn Monitor) -> Result<()> {
    let total: u64 = archive.generations.iter().map(|g| g.hashed_len).sum();
    let mut done = 0u64;
    let mut buf = vec![0u8; CHUNK];

    for (idx, gen) in archive.generations.iter().enumerate() {
        let actual: [u8; 32] = archive.with_reader(|reader| {
            reader.seek(SeekFrom::Start(gen.offset))?;
            let mut hasher = Sha256::new();
            let mut remaining = gen.hashed_len;
            while remaining > 0 {
                monitor.checkpoint()?;
                let want = remaining.min(CHUNK as u64) as usize;
                reader.read_exact(&mut buf[..want])?;
                hasher.update(&buf[..want]);
                remaining -= want as u64;
                done += want as u64;
                monitor.progress(Progress {
                    phase: Phase::Verifying,
                    done,
                    total,
                    item: None,
                });
            }
            Ok(hasher.finalize().into())
        })?;
        if actual != gen.integrity_hash {
            return Err(ArchiveError::Format(format!(
                "snapshot #{} is corrupted (integrity hash mismatch)",
                idx + 1
            )));
        }
    }
    Ok(())
}

/// Decompresses every stored blob and compares its SHA-256 with the hash
/// recorded when it was written.
pub fn verify_blob_hashes<R: Read + Seek>(
    archive: &Archive<R>,
    monitor: &dyn Monitor,
) -> Result<()> {
    let total: u64 = archive
        .generations
        .iter()
        .map(|g| g.owned_blob_bytes())
        .sum();
    let mut done = 0u64;
    let mut buf = vec![0u8; CHUNK];

    for gen in &archive.generations {
        // Name each blob after the first file that uses it, for messages.
        let mut names: HashMap<usize, PathBuf> = HashMap::new();
        for r in current_files(gen)? {
            names
                .entry(gen.files[r.file_idx].blob_idx as usize)
                .or_insert(r.path);
        }

        for section in &gen.section_blobs {
            for &i in section {
                let blob = &gen.blobs[i];
                let name = names.get(&i).cloned().unwrap_or_default();
                let mut reader = archive.blob_reader(blob)?;
                let mut hasher = Sha256::new();
                let mut blob_done = 0u64;
                loop {
                    monitor.checkpoint()?;
                    let n = reader.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                    blob_done += n as u64;
                    done += n as u64;
                    monitor.progress(Progress {
                        phase: Phase::VerifyingContent,
                        done,
                        total,
                        item: Some(Item {
                            path: &name,
                            done: blob_done,
                            total: blob.raw_size,
                        }),
                    });
                }
                let actual: [u8; 32] = hasher.finalize().into();
                if actual != gen.blob_hashes[i] {
                    return Err(ArchiveError::Format(format!(
                        "content of {} does not match its recorded hash",
                        name.display()
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Verifies every signed generation's signature against `key`. Unsigned
/// generations (`signature_type == 0`) are skipped.
pub fn verify_signatures<R>(archive: &Archive<R>, key: &VerifyingKey) -> Result<()> {
    for gen in &archive.generations {
        match gen.signature_type {
            0 => continue,
            1 => {
                if !verify_integrity_hash(key, &gen.integrity_hash, &gen.signature) {
                    return Err(ArchiveError::InvalidSignature);
                }
            }
            other => return Err(ArchiveError::UnsupportedSignature(other)),
        }
    }
    Ok(())
}

/// Runs every available check: on-disk integrity, per-blob content hashes,
/// and signatures if `key` is provided.
pub fn verify_deep<R: Read + Seek>(
    archive: &Archive<R>,
    key: Option<&VerifyingKey>,
    monitor: &dyn Monitor,
) -> Result<()> {
    verify_integrity(archive, monitor)?;
    verify_blob_hashes(archive, monitor)?;
    if let Some(key) = key {
        verify_signatures(archive, key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::read_archive;
    use bpfs_core::constants::DATA_BLOCK_SIZE;
    use bpfs_core::progress::NoMonitor;
    use bpfs_core::types::enums::CompressionType;
    use bpfs_pack::pack::{pack_directory, NoExistingBlobs, PackOptions};
    use bpfs_pack::policy::compression::DefaultCompressionPolicy;
    use ed25519_dalek::SigningKey;
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;
    use tempfile::tempdir;

    const POLICY: DefaultCompressionPolicy = DefaultCompressionPolicy {
        incompressible_entropy: 96.25,
    };

    fn pack(dir: &Path, key: Option<&SigningKey>) -> Vec<u8> {
        let mut buf = Vec::new();
        pack_directory(
            &mut buf,
            dir,
            true,
            PackOptions {
                generation_idx: 0,
                previous_integrity_hash: [0u8; 32],
                policy: &POLICY,
                compression: CompressionType::Zstd,
                block_size: DATA_BLOCK_SIZE,
                signing_key: key,
                existing_blobs: &NoExistingBlobs,
            },
        )
        .unwrap();
        buf
    }

    fn sample_dir() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("a.txt"),
            b"hello world, this is plaintext content",
        )
        .unwrap();
        fs::write(dir.path().join("b.bin"), b"\x00\x01\x02binarydata").unwrap();
        dir
    }

    #[test]
    fn valid_archive_passes_all_checks() {
        let dir = sample_dir();
        let archive = read_archive(Cursor::new(pack(dir.path(), None))).unwrap();
        verify_integrity(&archive, &NoMonitor).unwrap();
        verify_blob_hashes(&archive, &NoMonitor).unwrap();
    }

    #[test]
    fn tampered_data_fails_verification() {
        let dir = sample_dir();
        let buf = pack(dir.path(), None);
        let clean = read_archive(Cursor::new(buf.clone())).unwrap();
        let block = clean.latest().data_sections[1].blocks[0].clone();

        let mut tampered = buf;
        tampered[block.stored_offset as usize] ^= 0xFF;
        let archive = read_archive(Cursor::new(tampered)).unwrap();
        assert!(verify_integrity(&archive, &NoMonitor).is_err());
        assert!(verify_blob_hashes(&archive, &NoMonitor).is_err());
    }

    #[test]
    fn signed_archive_verifies_with_correct_key() {
        let dir = sample_dir();
        let key = SigningKey::generate(&mut rand::rng());
        let archive = read_archive(Cursor::new(pack(dir.path(), Some(&key)))).unwrap();
        assert!(verify_signatures(&archive, &key.verifying_key()).is_ok());
        assert!(verify_deep(&archive, Some(&key.verifying_key()), &NoMonitor).is_ok());
    }

    #[test]
    fn signed_archive_fails_with_wrong_key() {
        let dir = sample_dir();
        let key = SigningKey::generate(&mut rand::rng());
        let other = SigningKey::generate(&mut rand::rng());
        let archive = read_archive(Cursor::new(pack(dir.path(), Some(&key)))).unwrap();
        assert!(verify_signatures(&archive, &other.verifying_key()).is_err());
    }

    #[test]
    fn unsigned_archive_skips_signature_check() {
        let dir = sample_dir();
        let key = SigningKey::generate(&mut rand::rng());
        let archive = read_archive(Cursor::new(pack(dir.path(), None))).unwrap();
        assert!(verify_signatures(&archive, &key.verifying_key()).is_ok());
    }
}

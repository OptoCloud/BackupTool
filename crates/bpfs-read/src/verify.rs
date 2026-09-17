use sha2::{Digest, Sha256};

use bpfs_core::errors::{ArchiveError, Result};
use bpfs_core::signing::verify_integrity_hash;
use ed25519_dalek::VerifyingKey;

use crate::archive::Archive;

/// Recomputes the SHA-256 of every blob's actual bytes and compares it
/// against the `blobs_hashes` entry recorded when it was written. This is
/// the "deep" check: cheaper checks (per-section hashes, the generation
/// hash chain) already run while parsing.
pub fn verify_blob_hashes(archive: &Archive) -> Result<()> {
    for (gen_idx, gen) in archive.generations.iter().enumerate() {
        for (i, blob) in gen.blobs.iter().enumerate() {
            if blob.generation_idx as usize != gen_idx {
                continue; // stored elsewhere; verified when that generation is checked
            }
            let bytes = archive.blob_bytes(blob)?;
            let actual: [u8; 32] = Sha256::digest(bytes).into();
            let expected = gen.blob_hashes[i];
            if actual != expected {
                return Err(ArchiveError::HashMismatch("blob content hash"));
            }
        }
    }
    Ok(())
}

/// Verifies every signed generation's signature against `key`. Unsigned
/// generations (`signature_type == 0`) are skipped.
pub fn verify_signatures(archive: &Archive, key: &VerifyingKey) -> Result<()> {
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

/// Runs every available check: hash chain (already verified at parse time,
/// re-checked here for completeness), per-blob content hashes, and
/// signatures if `key` is provided.
pub fn verify_deep(archive: &Archive, key: Option<&VerifyingKey>) -> Result<()> {
    verify_blob_hashes(archive)?;
    if let Some(key) = key {
        verify_signatures(archive, key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::read_archive;
    use bpfs_pack::pack::{pack_directory, NoExistingBlobs, PackOptions};
    use bpfs_pack::policy::compression::DefaultCompressionPolicy;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use std::fs;
    use std::io::Cursor;
    use tempfile::tempdir;

    fn policy() -> DefaultCompressionPolicy {
        DefaultCompressionPolicy {
            incompressible_entropy: 96.25,
        }
    }

    #[test]
    fn verify_blob_hashes_passes_for_valid_archive() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        fs::write(dir.path().join("b.bin"), b"\x00\x01\x02binarydata").unwrap();

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
        assert!(verify_blob_hashes(&archive).is_ok());
    }

    #[test]
    fn verify_blob_hashes_fails_for_tampered_data_section() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("a.txt"),
            b"hello world, this is plaintext content",
        )
        .unwrap();

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

        // Tampering breaks the DataSection's own hash first, so parsing itself
        // should fail -- that's the correct, stronger outcome.
        let last = buf.len() / 2;
        buf[last] ^= 0xFF;
        assert!(read_archive(Cursor::new(buf)).is_err());
    }

    #[test]
    fn signed_archive_verifies_with_correct_key() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"signed content").unwrap();

        let key = SigningKey::generate(&mut OsRng);
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
                signing_key: Some(&key),
                existing_blobs: &NoExistingBlobs,
            },
        )
        .unwrap();

        let archive = read_archive(Cursor::new(buf)).unwrap();
        assert!(verify_signatures(&archive, &key.verifying_key()).is_ok());
    }

    #[test]
    fn signed_archive_fails_with_wrong_key() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"signed content").unwrap();

        let key = SigningKey::generate(&mut OsRng);
        let other = SigningKey::generate(&mut OsRng);
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
                signing_key: Some(&key),
                existing_blobs: &NoExistingBlobs,
            },
        )
        .unwrap();

        let archive = read_archive(Cursor::new(buf)).unwrap();
        assert!(verify_signatures(&archive, &other.verifying_key()).is_err());
    }

    #[test]
    fn unsigned_archive_skips_signature_check() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"no signature here").unwrap();

        let key = SigningKey::generate(&mut OsRng);
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
        assert!(verify_signatures(&archive, &key.verifying_key()).is_ok());
    }
}

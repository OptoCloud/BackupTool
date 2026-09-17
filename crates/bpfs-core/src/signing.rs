//! Thin wrapper around Ed25519 so `bpfs-pack` and `bpfs-read` agree on exactly
//! what gets signed/verified: the 32-byte generation `integrity_hash`.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

pub const SIGNATURE_SIZE: usize = 64;
pub const PUBLIC_KEY_SIZE: usize = 32;

pub fn sign_integrity_hash(key: &SigningKey, integrity_hash: &[u8; 32]) -> [u8; SIGNATURE_SIZE] {
    key.sign(integrity_hash).to_bytes()
}

pub fn verify_integrity_hash(
    key: &VerifyingKey,
    integrity_hash: &[u8; 32],
    signature: &[u8],
) -> bool {
    let Ok(sig_bytes) = <[u8; SIGNATURE_SIZE]>::try_from(signature) else {
        return false;
    };
    let signature = Signature::from_bytes(&sig_bytes);
    key.verify(integrity_hash, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn sign_then_verify_succeeds() {
        let key = SigningKey::generate(&mut OsRng);
        let hash = [7u8; 32];
        let sig = sign_integrity_hash(&key, &hash);
        assert!(verify_integrity_hash(&key.verifying_key(), &hash, &sig));
    }

    #[test]
    fn tampered_hash_fails_verification() {
        let key = SigningKey::generate(&mut OsRng);
        let hash = [7u8; 32];
        let sig = sign_integrity_hash(&key, &hash);
        let tampered = [8u8; 32];
        assert!(!verify_integrity_hash(
            &key.verifying_key(),
            &tampered,
            &sig
        ));
    }

    #[test]
    fn wrong_key_fails_verification() {
        let key = SigningKey::generate(&mut OsRng);
        let other = SigningKey::generate(&mut OsRng);
        let hash = [7u8; 32];
        let sig = sign_integrity_hash(&key, &hash);
        assert!(!verify_integrity_hash(&other.verifying_key(), &hash, &sig));
    }

    #[test]
    fn malformed_signature_bytes_rejected() {
        let key = SigningKey::generate(&mut OsRng);
        let hash = [7u8; 32];
        assert!(!verify_integrity_hash(
            &key.verifying_key(),
            &hash,
            &[0u8; 10]
        ));
    }
}

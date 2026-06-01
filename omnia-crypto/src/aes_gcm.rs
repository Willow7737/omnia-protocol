//! AES-256-GCM utility functions
//!
//! Shared encryption/decryption primitives used across the Omnia Protocol.
//! This module provides:
//!
//! - **`aes256gcm_encrypt_aad`** / **`aes256gcm_decrypt_aad`** — authenticated
//!   encryption with associated data (AAD), used for share encryption in the
//!   identity shard and anywhere else that needs AEAD.
//! - **`hkdf_aes_key`** — HKDF-SHA256 key derivation for AES-256 keys.
//! - **`generate_nonce`** — random 96-bit nonce generation.
//!
//! # Security
//!
//! - AES-256-GCM provides authenticated encryption: any tampering with the
//!   ciphertext or AAD is detected on decryption.
//! - Nonces must **never** be reused with the same key. Use [`generate_nonce`]
//!   to create a fresh random nonce for each encryption.
//! - Key derivation uses HKDF-SHA256 with a salt and info string for domain
//!   separation.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;

/// Error type for AES-GCM operations.
#[derive(Debug, thiserror::Error)]
pub enum AesGcmError {
    /// Encryption failed.
    #[error("AES-256-GCM encryption failed: {0}")]
    EncryptionFailed(String),
    /// Decryption failed (authentication error or corrupt ciphertext).
    #[error("AES-256-GCM decryption failed: authentication error")]
    DecryptionFailed,
    /// Invalid key length.
    #[error("Invalid AES key length")]
    InvalidKey,
}

/// AES-256-GCM encrypt with associated data (AAD).
///
/// Encrypts `plaintext` using the given `key` and `nonce`, binding the
/// `aad` (associated data) to the ciphertext. The AAD is authenticated
/// but not encrypted — it can be read without decrypting, but any
/// modification will cause decryption to fail.
///
/// # Security
///
/// - The nonce must be unique per encryption with the same key.
///   Use [`generate_nonce`] to create a random nonce.
/// - The key must be exactly 32 bytes (AES-256).
///
/// # Errors
///
/// Returns [`AesGcmError::InvalidKey`] if the key fails to initialize the cipher.
/// Returns [`AesGcmError::EncryptionFailed`] if the encryption operation fails.
pub fn aes256gcm_encrypt_aad(
    plaintext: &[u8],
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
) -> Result<Vec<u8>, AesGcmError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| AesGcmError::InvalidKey)?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .encrypt(nonce, aes_gcm::aead::Payload { msg: plaintext, aad })
        .map_err(|e| AesGcmError::EncryptionFailed(e.to_string()))
}

/// AES-256-GCM decrypt with associated data (AAD).
///
/// Decrypts `ciphertext` using the given `key` and `nonce`, verifying
/// that the `aad` matches what was used during encryption.
///
/// # Errors
///
/// Returns [`AesGcmError::DecryptionFailed`] if:
/// - The ciphertext has been tampered with
/// - The AAD doesn't match what was used during encryption
/// - The key or nonce is incorrect
pub fn aes256gcm_decrypt_aad(
    ciphertext: &[u8],
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
) -> Result<Vec<u8>, AesGcmError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| AesGcmError::InvalidKey)?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, aes_gcm::aead::Payload { msg: ciphertext, aad })
        .map_err(|_| AesGcmError::DecryptionFailed)
}

/// Derive an AES-256 key from key material using HKDF-SHA256.
///
/// Uses a fixed domain-separated salt (`OMNIA-AES-KEY-DERIVATION`) and the
/// full `key_material` as input keying material (IKM). The `info`
/// parameter provides domain separation.
///
/// # Security Note
///
/// This function uses a fixed, hardcoded salt (`b"OMNIA-AES-KEY-DERIVATION"`).
/// HKDF's security proof assumes the salt varies. A fixed salt means the same
/// `key_material` always produces the same derived key. This is acceptable for
/// single-derivation contexts (e.g., keystore encryption) but NOT for contexts
/// where multiple keys are derived from the same material with different salts.
/// For multi-key derivation, use a caller-provided random salt.
///
/// # Errors
///
/// Returns [`AesGcmError::InvalidKey`] if HKDF expand fails, which should
/// never happen for a 32-byte output (the maximum output of HKDF-SHA256
/// is 255 × 32 = 8160 bytes), but is handled for defensive programming.
pub fn hkdf_aes_key(key_material: &[u8; 32], info: &str) -> Result<[u8; 32], AesGcmError> {
    // Use a fixed domain-separated salt instead of splitting the key material.
    // Previously, the first 16 bytes were used as both salt and IKM (split),
    // which meant related key material was used for both HKDF inputs.
    // A fixed salt with domain separation ensures proper cryptographic hygiene.
    let salt = b"OMNIA-AES-KEY-DERIVATION";
    let hk = Hkdf::<Sha256>::new(Some(salt), key_material);
    let mut aes_key = [0u8; 32];
    hk.expand(info.as_bytes(), &mut aes_key)
        .map_err(|_| AesGcmError::InvalidKey)?;
    Ok(aes_key)
}

/// Generate a random 96-bit nonce for AES-256-GCM.
///
/// Uses the system's cryptographically secure random number generator.
/// The nonce must be unique for each encryption operation with the same key.
pub fn generate_nonce() -> [u8; 12] {
    use rand::RngCore;
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_aad_roundtrip() {
        let key = [0x42u8; 32];
        let nonce = generate_nonce();
        let plaintext = b"hello world";
        let aad = b"associated data";

        let ciphertext = aes256gcm_encrypt_aad(plaintext, &key, &nonce, aad).unwrap();
        let decrypted = aes256gcm_decrypt_aad(&ciphertext, &key, &nonce, aad).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_wrong_aad_fails() {
        let key = [0x42u8; 32];
        let nonce = generate_nonce();
        let plaintext = b"hello world";
        let aad = b"correct aad";

        let ciphertext = aes256gcm_encrypt_aad(plaintext, &key, &nonce, aad).unwrap();
        let result = aes256gcm_decrypt_aad(&ciphertext, &key, &nonce, b"wrong aad");
        assert!(result.is_err(), "Wrong AAD should fail decryption");
    }

    #[test]
    fn test_decrypt_tampered_ciphertext_fails() {
        let key = [0x42u8; 32];
        let nonce = generate_nonce();
        let plaintext = b"hello world";
        let aad = b"aad";

        let mut ciphertext = aes256gcm_encrypt_aad(plaintext, &key, &nonce, aad).unwrap();
        if !ciphertext.is_empty() {
            ciphertext[0] ^= 0xFF;
        }
        let result = aes256gcm_decrypt_aad(&ciphertext, &key, &nonce, aad);
        assert!(result.is_err(), "Tampered ciphertext should fail");
    }

    #[test]
    fn test_hkdf_aes_key_deterministic() {
        let key_material = [0xABu8; 32];
        let key1 = hkdf_aes_key(&key_material, "test-info").unwrap();
        let key2 = hkdf_aes_key(&key_material, "test-info").unwrap();
        assert_eq!(key1, key2, "Same inputs must produce same key");
    }

    #[test]
    fn test_hkdf_aes_key_different_info() {
        let key_material = [0xABu8; 32];
        let key1 = hkdf_aes_key(&key_material, "info-1").unwrap();
        let key2 = hkdf_aes_key(&key_material, "info-2").unwrap();
        assert_ne!(key1, key2, "Different info must produce different keys");
    }
}

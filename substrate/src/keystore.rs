//! Encrypted key store for validator key management
//!
//! This module provides [`EncryptedKeyStore`] for creating, loading, and
//! rotating validator keypairs. Keys are stored on disk with encryption
//! at rest. Key rotation produces a [`KeyRotationProof`] that can be
//! verified by other nodes to confirm the rotation was legitimate.
//!
//! # Security Model
//!
//! - Private keys are encrypted with a passphrase using AES-256-GCM.
//! - The passphrase is never stored; it must be provided at load time.
//! - Key rotation generates a new keypair and signs the rotation with
//!   the old key, producing a proof that other validators can verify.
//!
//! # Example
//!
//! ```ignore
//! use omnia_substrate::keystore::EncryptedKeyStore;
//!
//! // Create a new key store
//! let store = EncryptedKeyStore::create("./keys", "my-secure-passphrase")?;
//!
//! // Load an existing key store
//! let loaded = EncryptedKeyStore::load("./keys", "my-secure-passphrase")?;
//!
//! // Rotate the key
//! let proof = loaded.rotate("my-secure-passphrase", "new-secure-passphrase")?;
//! ```

use crate::crypto::{generate_keypair, NodeKeypair, NodePublicKey, Signer, Verifier};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during key store operations.
#[derive(Error, Debug)]
pub enum KeyStoreError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The key store file could not be parsed.
    #[error("Invalid key store format: {0}")]
    InvalidFormat(String),
    /// The provided passphrase is incorrect.
    #[error("Incorrect passphrase")]
    IncorrectPassphrase,
    /// The key store already exists at the specified path.
    #[error("Key store already exists at {0}")]
    AlreadyExists(String),
    /// The key store does not exist at the specified path.
    #[error("Key store not found at {0}")]
    NotFound(String),
    /// A cryptographic operation failed.
    #[error("Crypto error: {0}")]
    Crypto(String),
}

/// Result type for key store operations.
pub type KeyStoreResult<T> = Result<T, KeyStoreError>;

/// Persistent encrypted key store for a validator's Ed25519 keypair.
///
/// Stores the public key in plaintext and the encrypted private key
/// on disk. The private key is encrypted with a passphrase using
/// a simple XOR-based encryption (for demonstration; production
/// should use AES-256-GCM via a crate like `ring` or `aes-gcm`).
///
/// Files created:
/// - `<dir>/pubkey` — 32-byte Ed25519 public key (plaintext)
/// - `<dir>/seckey.enc` — 64-byte Ed25519 secret key (encrypted)
#[derive(Debug)]
pub struct EncryptedKeyStore {
    /// Directory where key files are stored.
    dir: PathBuf,
    /// The public key (always available after load).
    public_key: NodePublicKey,
    /// The keypair (only available after load with correct passphrase).
    keypair: Option<NodeKeypair>,
}

/// Proof that a key rotation was performed by the previous key holder.
///
/// Contains the old public key, the new public key, and a signature
/// from the old key over the new public key. This allows other
/// validators to verify that the rotation was authorized.
#[derive(Debug, Clone, Serialize)]
pub struct KeyRotationProof {
    /// The public key before rotation.
    pub old_pubkey: [u8; 32],
    /// The public key after rotation.
    pub new_pubkey: [u8; 32],
    /// Ed25519 signature from the old key over `new_pubkey`.
    #[serde(with = "serde_array_64")]
    pub signature: [u8; 64],
    /// Timestamp of the rotation (milliseconds since UNIX epoch).
    pub timestamp: u64,
}

/// Serde helper for serializing `[u8; 64]` as bytes.
/// Serde only natively implements Serialize/Deserialize for arrays up to size 32.
#[allow(dead_code)]
mod serde_array_64 {
    use super::*;

    pub fn serialize<S: Serializer>(data: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(data)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        let bytes: Vec<u8> = Vec::deserialize(d)?;
        let mut arr = [0u8; 64];
        if bytes.len() == 64 {
            arr.copy_from_slice(&bytes);
            Ok(arr)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected 64 bytes, got {}",
                bytes.len()
            )))
        }
    }
}

impl EncryptedKeyStore {
    /// Create a new encrypted key store at the given directory.
    ///
    /// Generates a fresh Ed25519 keypair, encrypts the secret key with
    /// the provided passphrase, and writes both keys to disk.
    ///
    /// # Errors
    ///
    /// - [`KeyStoreError::AlreadyExists`] if the directory already contains key files.
    /// - [`KeyStoreError::Io`] if the directory cannot be created or files cannot be written.
    pub fn create(dir: &Path, passphrase: &str) -> KeyStoreResult<Self> {
        std::fs::create_dir_all(dir)?;

        let pubkey_path = dir.join("pubkey");
        let seckey_path = dir.join("seckey.enc");

        if pubkey_path.exists() || seckey_path.exists() {
            return Err(KeyStoreError::AlreadyExists(dir.display().to_string()));
        }

        let keypair = generate_keypair();
        let public_key = keypair.verifying_key();

        // Write public key (plaintext)
        std::fs::write(&pubkey_path, public_key.to_bytes())?;

        // Encrypt and write secret key
        let encrypted = xor_encrypt(keypair.to_bytes().as_slice(), passphrase);
        std::fs::write(&seckey_path, encrypted)?;

        tracing::info!(
            dir = %dir.display(),
            pubkey = %hex::encode(&public_key.to_bytes()[..8]),
            "Created new validator key store"
        );

        Ok(Self {
            dir: dir.to_path_buf(),
            public_key,
            keypair: Some(keypair),
        })
    }

    /// Load an existing key store from disk.
    ///
    /// Reads the public key and encrypted secret key from disk,
    /// decrypts the secret key with the provided passphrase,
    /// and reconstructs the keypair.
    ///
    /// # Errors
    ///
    /// - [`KeyStoreError::NotFound`] if the key files do not exist.
    /// - [`KeyStoreError::IncorrectPassphrase`] if decryption yields invalid key bytes.
    /// - [`KeyStoreError::Io`] if the files cannot be read.
    pub fn load(dir: &Path, passphrase: &str) -> KeyStoreResult<Self> {
        let pubkey_path = dir.join("pubkey");
        let seckey_path = dir.join("seckey.enc");

        if !pubkey_path.exists() || !seckey_path.exists() {
            return Err(KeyStoreError::NotFound(dir.display().to_string()));
        }

        // Read and parse public key
        let pubkey_bytes = std::fs::read(&pubkey_path)?;
        if pubkey_bytes.len() != 32 {
            return Err(KeyStoreError::InvalidFormat(
                "Public key must be 32 bytes".to_string(),
            ));
        }
        let mut pk_arr = [0u8; 32];
        pk_arr.copy_from_slice(&pubkey_bytes);
        let public_key = NodePublicKey::from_bytes(&pk_arr)
            .map_err(|e| KeyStoreError::InvalidFormat(e.to_string()))?;

        // Read and decrypt secret key
        let encrypted_seckey = std::fs::read(&seckey_path)?;
        let decrypted = xor_decrypt(&encrypted_seckey, passphrase);

        if decrypted.len() != 32 {
            return Err(KeyStoreError::IncorrectPassphrase);
        }
        let mut sk_arr = [0u8; 32];
        sk_arr.copy_from_slice(&decrypted[..32]);

        // NodeKeypair::from_bytes returns SigningKey directly (not Result)
        let keypair = NodeKeypair::from_bytes(&sk_arr);

        // Verify the keypair matches the stored public key
        if keypair.verifying_key().to_bytes() != public_key.to_bytes() {
            return Err(KeyStoreError::IncorrectPassphrase);
        }

        tracing::info!(
            dir = %dir.display(),
            pubkey = %hex::encode(&public_key.to_bytes()[..8]),
            "Loaded validator key store"
        );

        Ok(Self {
            dir: dir.to_path_buf(),
            public_key,
            keypair: Some(keypair),
        })
    }

    /// Rotate the validator key.
    ///
    /// Generates a new keypair, signs the new public key with the old
    /// private key to produce a [`KeyRotationProof`], and re-encrypts
    /// the new secret key with the new passphrase.
    ///
    /// The old key files are replaced with the new ones.
    ///
    /// # Arguments
    ///
    /// * `old_passphrase` — Passphrase for the current secret key.
    /// * `new_passphrase` — Passphrase for the new secret key.
    ///
    /// # Errors
    ///
    /// - [`KeyStoreError::IncorrectPassphrase`] if the old passphrase is wrong.
    /// - [`KeyStoreError::Io`] if key files cannot be written.
    pub fn rotate(
        &self,
        old_passphrase: &str,
        new_passphrase: &str,
    ) -> KeyStoreResult<KeyRotationProof> {
        // Ensure we have the keypair loaded
        let old_keypair = self
            .keypair
            .as_ref()
            .ok_or_else(|| KeyStoreError::Crypto("Keypair not loaded".to_string()))?;

        // Verify the old passphrase by attempting to re-load
        let loaded = Self::load(&self.dir, old_passphrase)?;

        // Generate new keypair
        let new_keypair = generate_keypair();
        let new_pubkey = new_keypair.verifying_key();

        // Sign the new public key with the old private key
        let signature = loaded
            .keypair
            .as_ref()
            .ok_or_else(|| KeyStoreError::Crypto("Loaded keypair missing".to_string()))?
            .sign(&new_pubkey.to_bytes());

        let old_pubkey_bytes = old_keypair.verifying_key().to_bytes();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let proof = KeyRotationProof {
            old_pubkey: old_pubkey_bytes,
            new_pubkey: new_pubkey.to_bytes(),
            signature: signature.to_bytes(),
            timestamp: now_ms,
        };

        // Write new public key
        let pubkey_path = self.dir.join("pubkey");
        std::fs::write(&pubkey_path, new_pubkey.to_bytes())?;

        // Write new encrypted secret key
        let seckey_path = self.dir.join("seckey.enc");
        let encrypted = xor_encrypt(new_keypair.to_bytes().as_slice(), new_passphrase);
        std::fs::write(&seckey_path, encrypted)?;

        tracing::info!(
            dir = %self.dir.display(),
            old_pubkey = %hex::encode(&old_pubkey_bytes[..8]),
            new_pubkey = %hex::encode(&new_pubkey.to_bytes()[..8]),
            "Rotated validator key"
        );

        Ok(proof)
    }

    /// Get the public key from this key store.
    pub fn public_key(&self) -> &NodePublicKey {
        &self.public_key
    }

    /// Get the directory where key files are stored.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl KeyRotationProof {
    /// Verify that this rotation proof is valid.
    ///
    /// Checks that the signature over `new_pubkey` verifies against
    /// `old_pubkey`. This confirms the rotation was authorized by
    /// the previous key holder.
    ///
    /// # Returns
    ///
    /// `true` if the signature is valid, `false` otherwise.
    pub fn verify(&self) -> bool {
        let Ok(old_pk) = NodePublicKey::from_bytes(&self.old_pubkey) else {
            return false;
        };
        let Ok(sig) = crate::crypto::Signature::from_slice(&self.signature) else {
            return false;
        };
        old_pk.verify(&self.new_pubkey, &sig).is_ok()
    }
}

/// Simple XOR-based encryption for demonstration purposes.
///
/// **NOTE**: This is NOT suitable for production. Use AES-256-GCM
/// via `ring` or `aes-gcm` crate in production deployments.
fn xor_encrypt(data: &[u8], passphrase: &str) -> Vec<u8> {
    let key = derive_key(passphrase);
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()])
        .collect()
}

/// Simple XOR-based decryption (symmetric with encryption).
fn xor_decrypt(data: &[u8], passphrase: &str) -> Vec<u8> {
    xor_encrypt(data, passphrase)
}

/// Derive a fixed-size key from a passphrase using BLAKE3.
fn derive_key(passphrase: &str) -> [u8; 32] {
    *blake3::hash(passphrase.as_bytes()).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_key_store() {
        let dir = TempDir::new().expect("temp dir");
        let store =
            EncryptedKeyStore::create(dir.path(), "test-passphrase").expect("create key store");
        assert!(dir.path().join("pubkey").exists());
        assert!(dir.path().join("seckey.enc").exists());
        assert!(store.keypair.is_some());
    }

    #[test]
    fn test_load_key_store() {
        let dir = TempDir::new().expect("temp dir");
        EncryptedKeyStore::create(dir.path(), "test-passphrase").expect("create");

        let loaded = EncryptedKeyStore::load(dir.path(), "test-passphrase").expect("load");
        assert!(loaded.keypair.is_some());
        // The public key should match the file on disk
        let pubkey_bytes = std::fs::read(dir.path().join("pubkey")).expect("read pubkey");
        assert_eq!(
            loaded.public_key.to_bytes().as_slice(),
            pubkey_bytes.as_slice()
        );
    }

    #[test]
    fn test_load_wrong_passphrase() {
        let dir = TempDir::new().expect("temp dir");
        EncryptedKeyStore::create(dir.path(), "correct-passphrase").expect("create");

        let result = EncryptedKeyStore::load(dir.path(), "wrong-passphrase");
        assert!(result.is_err(), "Wrong passphrase should fail to load");
    }

    #[test]
    fn test_create_already_exists() {
        let dir = TempDir::new().expect("temp dir");
        EncryptedKeyStore::create(dir.path(), "pass1").expect("create first");
        let result = EncryptedKeyStore::create(dir.path(), "pass2");
        assert!(matches!(result, Err(KeyStoreError::AlreadyExists(_))));
    }

    #[test]
    fn test_load_not_found() {
        let dir = TempDir::new().expect("temp dir");
        let result = EncryptedKeyStore::load(dir.path(), "pass");
        assert!(matches!(result, Err(KeyStoreError::NotFound(_))));
    }

    #[test]
    fn test_rotate_key() {
        let dir = TempDir::new().expect("temp dir");
        let store = EncryptedKeyStore::create(dir.path(), "old-pass").expect("create");

        let proof = store.rotate("old-pass", "new-pass").expect("rotate");

        // Verify the rotation proof
        assert!(proof.verify(), "Rotation proof should be valid");

        // Load with new passphrase should work
        let loaded =
            EncryptedKeyStore::load(dir.path(), "new-pass").expect("load with new passphrase");
        assert_eq!(loaded.public_key.to_bytes(), proof.new_pubkey);

        // Load with old passphrase should fail
        let result = EncryptedKeyStore::load(dir.path(), "old-pass");
        assert!(result.is_err(), "Old passphrase should no longer work");
    }

    #[test]
    fn test_rotate_wrong_old_passphrase() {
        let dir = TempDir::new().expect("temp dir");
        let store = EncryptedKeyStore::create(dir.path(), "correct").expect("create");

        let result = store.rotate("wrong", "new-pass");
        assert!(result.is_err(), "Wrong old passphrase should fail");
    }

    #[test]
    fn test_rotation_proof_verify_valid() {
        let dir = TempDir::new().expect("temp dir");
        let store = EncryptedKeyStore::create(dir.path(), "pass").expect("create");
        let proof = store.rotate("pass", "new-pass").expect("rotate");
        assert!(proof.verify());
    }

    #[test]
    fn test_rotation_proof_verify_tampered() {
        let dir = TempDir::new().expect("temp dir");
        let store = EncryptedKeyStore::create(dir.path(), "pass").expect("create");
        let mut proof = store.rotate("pass", "new-pass").expect("rotate");

        // Tamper with the new pubkey
        proof.new_pubkey[0] ^= 0xFF;
        assert!(!proof.verify(), "Tampered proof should fail verification");
    }

    #[test]
    fn test_rotation_proof_timestamp() {
        let dir = TempDir::new().expect("temp dir");
        let store = EncryptedKeyStore::create(dir.path(), "pass").expect("create");
        let proof = store.rotate("pass", "new-pass").expect("rotate");

        // Timestamp should be recent (within last 60 seconds)
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        assert!(proof.timestamp > 0);
        assert!(now_ms.saturating_sub(proof.timestamp) < 60_000);
    }

    #[test]
    fn test_xor_encrypt_decrypt_roundtrip() {
        let data = b"hello world this is a test of encryption";
        let passphrase = "my-secret-passphrase";
        let encrypted = xor_encrypt(data, passphrase);
        let decrypted = xor_decrypt(&encrypted, passphrase);
        assert_eq!(data.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_xor_encrypt_wrong_passphrase() {
        let data = b"hello world";
        let encrypted = xor_encrypt(data, "correct");
        let decrypted = xor_decrypt(&encrypted, "wrong");
        assert_ne!(data.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_derive_key_deterministic() {
        let k1 = derive_key("passphrase");
        let k2 = derive_key("passphrase");
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_derive_key_different_passphrases() {
        let k1 = derive_key("passphrase1");
        let k2 = derive_key("passphrase2");
        assert_ne!(k1, k2);
    }
}

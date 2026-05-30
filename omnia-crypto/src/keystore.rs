//! Encrypted key store for validator key management
//!
//! This module provides [`EncryptedKeyStore`] for creating, loading, and
//! rotating validator keypairs. Keys are stored on disk with encryption
//! at rest. Key rotation produces a [`KeyRotationProof`] that can be
//! verified by other nodes to confirm the rotation was legitimate.
//!
//! # Security Model
//!
//! - Private keys are encrypted with a passphrase using **AES-256-GCM**
//!   with HKDF-SHA256 key derivation and per-encryption random salt + nonce.
//! - The passphrase is never stored; it must be provided at load time.
//! - Key rotation generates a new keypair and signs the rotation with
//!   the old key, producing a proof that other validators can verify.
//! - Backward compatibility: stores created with the legacy XOR encryption
//!   can still be loaded (they are automatically upgraded on next write).
//!
//! # Encryption Format
//!
//! Encrypted data layout: `salt(32 bytes) || nonce(12 bytes) || ciphertext+tag`
//!
//! - **salt**: 32 random bytes, fed into HKDF-SHA256 along with the passphrase
//!   to derive a unique 256-bit AES key per encryption.
//! - **nonce**: 12 random bytes, the AES-256-GCM IV.
//! - **ciphertext+tag**: AES-256-GCM ciphertext with the 16-byte authentication
//!   tag appended (ensures integrity and authenticity).
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
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use hkdf::Hkdf;
use omnia_primitives::blake3_hash_domain;
use rand::RngCore;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::Sha256;
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
    /// AES-256-GCM encryption failed.
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
}

/// Result type for key store operations.
pub type KeyStoreResult<T> = Result<T, KeyStoreError>;

/// Purpose for derived child keys (HKDF-based derivation).
///
/// Note: Derivation uses HKDF-SHA256, not SLIP-0010.
/// TODO: Implement real SLIP-0010 (HMAC-SHA512 based) key derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum KeyPurpose {
    /// Identity keys: m/44'/6061'/0'
    Identity = 0,
    /// Consensus keys: m/44'/6061'/1'
    Consensus = 1,
    /// Governance keys: m/44'/6061'/2'
    Governance = 2,
    /// Staking keys: m/44'/6061'/3'
    Staking = 3,
}

/// Persistent encrypted key store for a validator's Ed25519 keypair.
///
/// Stores the public key in plaintext and the encrypted private key
/// on disk. The private key is encrypted with a passphrase using
/// **AES-256-GCM** with HKDF-SHA256 key derivation.
///
/// Files created:
/// - `<dir>/pubkey` — 32-byte Ed25519 public key (plaintext)
/// - `<dir>/seckey.enc` — Encrypted secret key (AES-256-GCM: salt(32) + nonce(12) + ciphertext+tag)
#[derive(Debug)]
pub struct EncryptedKeyStore {
    /// Directory where key files are stored.
    dir: PathBuf,
    /// The public key (always available after load).
    public_key: NodePublicKey,
    /// The keypair (only available after load with correct passphrase).
    keypair: Option<NodeKeypair>,
    /// Whether the keystore was loaded from legacy XOR format and needs upgrade.
    needs_upgrade: bool,
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
    /// the provided passphrase using AES-256-GCM, and writes both keys
    /// to disk.
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

        // Encrypt and write secret key using AES-256-GCM
        let encrypted = aes_gcm_encrypt(keypair.to_bytes().as_slice(), passphrase)?;
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
            needs_upgrade: false,
        })
    }

    /// Load an existing key store from disk.
    ///
    /// Reads the public key and encrypted secret key from disk,
    /// decrypts the secret key with the provided passphrase,
    /// and reconstructs the keypair.
    ///
    /// Tries AES-256-GCM decryption first (new format). If that fails,
    /// falls back to legacy XOR decryption for backward compatibility
    /// with stores created before the migration.
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
            return Err(KeyStoreError::InvalidFormat("Public key must be 32 bytes".to_string()));
        }
        let mut pk_arr = [0u8; 32];
        pk_arr.copy_from_slice(&pubkey_bytes);
        let public_key = NodePublicKey::from_bytes(&pk_arr).map_err(|e| KeyStoreError::InvalidFormat(e.to_string()))?;

        // Read and decrypt secret key
        // Try AES-256-GCM first (new format), then fall back to legacy XOR
        let encrypted_seckey = std::fs::read(&seckey_path)?;

        let (decrypted, needs_upgrade) = if encrypted_seckey.len() >= 44 {
            // Could be AES-256-GCM format (salt(32) + nonce(12) + ciphertext+tag)
            match aes_gcm_decrypt(&encrypted_seckey, passphrase) {
                Ok(plain) => (plain, false),
                Err(_) => {
                    // XOR fallback removed for security (downgrade attack vector).
                    // If you need to migrate a legacy XOR keystore, enable the
                    // `legacy-xor` feature flag.
                    #[cfg(feature = "legacy-xor")]
                    {
                        tracing::warn!(
                            "⚠️  DEPRECATED: XOR keystore fallback used. \
                             This fallback will be removed in v0.4.0. \
                             Migrate by calling rotate() with the current passphrase to upgrade \
                             to AES-256-GCM."
                        );
                        #[allow(deprecated)]
                        let xor_decrypted = xor_decrypt(&encrypted_seckey, passphrase);
                        if xor_decrypted.len() == 32 {
                            tracing::warn!("⚠️  Loaded keystore with deprecated XOR encryption — will upgrade to AES-256-GCM on next write/rotate");
                            (xor_decrypted, true)
                        } else {
                            return Err(KeyStoreError::IncorrectPassphrase);
                        }
                    }
                    #[cfg(not(feature = "legacy-xor"))]
                    return Err(KeyStoreError::IncorrectPassphrase);
                }
            }
        } else {
            // Too short for AES-256-GCM — try legacy XOR only if feature enabled
            #[cfg(feature = "legacy-xor")]
            {
                tracing::warn!(
                    "⚠️  DEPRECATED: XOR keystore fallback used (short ciphertext). \
                     This fallback will be removed in v0.4.0. \
                     Migrate by calling rotate() with the current passphrase to upgrade \
                     to AES-256-GCM."
                );
                #[allow(deprecated)]
                let xor_decrypted = xor_decrypt(&encrypted_seckey, passphrase);
                if xor_decrypted.len() == 32 {
                    tracing::warn!("⚠️  Loaded keystore with deprecated XOR encryption — will upgrade to AES-256-GCM on next write/rotate");
                    (xor_decrypted, true)
                } else {
                    return Err(KeyStoreError::IncorrectPassphrase);
                }
            }
            #[cfg(not(feature = "legacy-xor"))]
            return Err(KeyStoreError::IncorrectPassphrase);
        };

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
            needs_upgrade,
        })
    }

    /// Rotate the validator key.
    ///
    /// Generates a new keypair, signs the new public key with the old
    /// private key to produce a [`KeyRotationProof`], and re-encrypts
    /// the new secret key with the new passphrase using AES-256-GCM.
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
    pub fn rotate(&self, old_passphrase: &str, new_passphrase: &str) -> KeyStoreResult<KeyRotationProof> {
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

        // Write new public key atomically via temp file
        let pubkey_path = self.dir.join("pubkey");
        let pubkey_temp = pubkey_path.with_extension("tmp");
        std::fs::write(&pubkey_temp, new_pubkey.to_bytes())?;
        std::fs::rename(&pubkey_temp, &pubkey_path)?;

        // Write new encrypted secret key atomically via temp file
        let seckey_path = self.dir.join("seckey.enc");
        let seckey_temp = seckey_path.with_extension("tmp");
        let encrypted = aes_gcm_encrypt(new_keypair.to_bytes().as_slice(), new_passphrase)?;
        std::fs::write(&seckey_temp, encrypted)?;
        std::fs::rename(&seckey_temp, &seckey_path)?;

        tracing::info!(
            dir = %self.dir.display(),
            old_pubkey = %hex::encode(&old_pubkey_bytes[..8]),
            new_pubkey = %hex::encode(&new_pubkey.to_bytes()[..8]),
            "Rotated validator key"
        );

        // If the keystore was loaded from legacy XOR format, the rotation
        // automatically re-encrypts with AES-256-GCM (above), completing
        // the upgrade. The caller should re-load the keystore to get an
        // instance with needs_upgrade = false.
        if self.needs_upgrade {
            tracing::info!(
                dir = %self.dir.display(),
                "Legacy XOR keystore automatically upgraded to AES-256-GCM via rotation"
            );
        }

        Ok(proof)
    }

    /// Get the public key from this key store.
    pub fn public_key(&self) -> &NodePublicKey {
        &self.public_key
    }

    /// Check whether this keystore was loaded from a legacy XOR format
    /// and needs to be upgraded to AES-256-GCM.
    ///
    /// After calling [`rotate()`](Self::rotate), the on-disk format is
    /// automatically upgraded. Re-loading the keystore will produce an
    /// instance with `needs_upgrade == false`.
    pub fn needs_upgrade(&self) -> bool {
        self.needs_upgrade
    }

    /// Get the directory where key files are stored.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Get a reference to the loaded keypair, if available.
    ///
    /// The keypair is always present after a successful [`create`](Self::create)
    /// or [`load`](Self::load) call, but may be `None` if the keystore
    /// was constructed in a non-standard way.
    pub fn keypair(&self) -> Option<&NodeKeypair> {
        self.keypair.as_ref()
    }

    /// Create keystore from a BIP-39 mnemonic phrase.
    ///
    /// The Ed25519 keypair is derived **deterministically** from the mnemonic
    /// using BLAKE3 with a fixed domain separator (`Omnia-Ed25519-KeyDerivation-V1`)
    /// as the salt, then using the 32-byte output as the Ed25519 secret key seed.
    /// The same mnemonic always produces the same keypair.
    ///
    /// The keystore encryption key is derived separately via HKDF-SHA256 from
    /// the BIP-39 seed.
    ///
    /// # Arguments
    ///
    /// * `mnemonic` — The BIP-39 mnemonic
    /// * `passphrase` — Optional BIP-39 passphrase (different from keystore passphrase)
    /// * `data_dir` — Directory for keystore files
    ///
    /// # Errors
    ///
    /// Returns [`KeyStoreError`] if the mnemonic is invalid or keystore creation fails.
    pub fn from_mnemonic(
        mnemonic: &bip39::Mnemonic,
        passphrase: Option<&str>,
        data_dir: &Path,
    ) -> KeyStoreResult<Self> {
        let seed = mnemonic.to_seed(passphrase.unwrap_or(""));
        // Use first 32 bytes of 64-byte seed for Ed25519 key derivation
        // and last 32 bytes for encryption key derivation.
        // Previously both used the same first 32 bytes, which meant the
        // encryption key was deterministically derivable from the Ed25519 key.
        let keypair = Self::derive_ed25519_keypair_from_seed(&seed[..32]);
        let encryption_key = Self::derive_key_from_seed(&seed[32..64])?;
        Self::initialize_with_deterministic_key(data_dir, &encryption_key, keypair)
    }

    /// Generate a random 24-word mnemonic and create keystore from it.
    ///
    /// # Arguments
    ///
    /// * `passphrase` — Optional BIP-39 passphrase
    /// * `data_dir` — Directory for keystore files
    ///
    /// # Returns
    ///
    /// A tuple of (keystore, mnemonic) on success.
    pub fn generate_with_mnemonic(
        passphrase: Option<&str>,
        data_dir: &Path,
    ) -> KeyStoreResult<(Self, bip39::Mnemonic)> {
        let mnemonic = bip39::Mnemonic::generate(24)
            .map_err(|e| KeyStoreError::Crypto(format!("Mnemonic generation failed: {e}")))?;
        let keystore = Self::from_mnemonic(&mnemonic, passphrase, data_dir)?;
        Ok((keystore, mnemonic))
    }

    /// Derive an Ed25519 child key for a specific purpose using HKDF.
    ///
    /// This function uses HKDF-SHA256 to derive child keys from the master
    /// keypair. It is **not** SLIP-0010 compliant — SLIP-0010 requires
    /// HMAC-SHA512 based derivation with hardened key paths.
    ///
    /// Derivation info: `OMNIA-HKDF-{purpose}-{index}-V1`
    ///
    /// # Arguments
    ///
    /// * `purpose` — The key purpose (Identity, Consensus, Governance, Staking)
    /// * `index` — The child key index
    ///
    /// # Errors
    ///
    /// Returns [`KeyStoreError`] if key derivation fails.
    ///
    /// # Deprecation
    ///
    /// This function was previously named `derive_child_key` and claimed to
    /// implement SLIP-0010, but it actually uses HKDF-SHA256 instead of the
    /// correct HMAC-SHA512-based derivation specified in SLIP-0010.
    // TODO: Implement real SLIP-0010 (HMAC-SHA512 based) key derivation.
    //       The current HKDF approach is not compatible with standard HD wallets.
    #[deprecated(
        since = "0.3.0",
        note = "This function uses HKDF, not SLIP-0010. Use derive_key_hkdf() explicitly or implement real SLIP-0010."
    )]
    #[allow(deprecated)]
    pub fn derive_child_key(&self, purpose: KeyPurpose, index: u32) -> KeyStoreResult<ed25519_dalek::SigningKey> {
        self.derive_key_hkdf(purpose, index)
    }

    /// Derive an Ed25519 child key for a specific purpose using HKDF-SHA256.
    ///
    /// This function uses HKDF-SHA256 to derive child keys from the master
    /// keypair. Note: this is **not** SLIP-0010 compliant. SLIP-0010 requires
    /// HMAC-SHA512 based derivation with hardened key paths.
    ///
    /// Derivation info: `OMNIA-HKDF-{purpose}-{index}-V1`
    ///
    /// # Arguments
    ///
    /// * `purpose` — The key purpose (Identity, Consensus, Governance, Staking)
    /// * `index` — The child key index
    ///
    /// # Errors
    ///
    /// Returns [`KeyStoreError`] if key derivation fails.
    // TODO: Implement real SLIP-0010 (HMAC-SHA512 based) key derivation.
    //       The current HKDF approach is not compatible with standard HD wallets.
    pub fn derive_key_hkdf(&self, purpose: KeyPurpose, index: u32) -> KeyStoreResult<ed25519_dalek::SigningKey> {
        let secret_bytes = self
            .keypair()
            .ok_or_else(|| KeyStoreError::Crypto("No keypair loaded".into()))?
            .to_bytes();

        let purpose_index = purpose as u32;
        let derivation_info = format!("OMNIA-HKDF-{purpose_index}-{index}-V1");

        let hkdf = Hkdf::<Sha256>::new(Some(&purpose_index.to_be_bytes()), &secret_bytes);
        let mut derived_key = [0u8; 32];
        hkdf.expand(derivation_info.as_bytes(), &mut derived_key)
            .map_err(|e| KeyStoreError::Crypto(format!("HKDF expand failed: {e}")))?;

        Ok(ed25519_dalek::SigningKey::from_bytes(&derived_key))
    }

    /// Derive an Ed25519 child key using SLIP-0010-compliant HMAC-SHA512 derivation.
    ///
    /// This function implements the SLIP-0010 standard for Ed25519 key derivation,
    /// which uses HMAC-SHA512 with hardened-only key paths. This is compatible with
    /// standard HD wallets (e.g., hardware wallets, Ledger, Trezor) and follows
    /// the derivation path `m/44'/6061'/{purpose}'/{index}'`.
    ///
    /// # SLIP-0010 Derivation
    ///
    /// Each child key is derived as:
    /// 1. `I = HMAC-SHA512(Key = "ed25519 seed", Data = 0x00 || parent_key || 0x01 || parent_chain_code)`
    ///    — for the master key from the seed
    /// 2. Subsequent levels use:
    ///    `I = HMAC-SHA512(Key = chain_code, Data = 0x00 || ser_256(k_par) || i + 2^31)`
    ///    where `i + 2^31` is the hardened index in big-endian 4 bytes.
    ///
    /// # Arguments
    ///
    /// * `purpose` — The key purpose (Identity, Consensus, Governance, Staking)
    /// * `index` — The child key index (automatically treated as hardened)
    ///
    /// # Errors
    ///
    /// Returns [`KeyStoreError`] if key derivation fails or the keypair is not loaded.
    ///
    /// # References
    ///
    /// - SLIP-0010: <https://github.com/satoshilabs/slips/blob/master/slip-0010.md>
    /// - BIP-44: <https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki>
    pub fn derive_child_key_slip0010(
        &self,
        purpose: KeyPurpose,
        index: u32,
    ) -> KeyStoreResult<ed25519_dalek::SigningKey> {
        let secret_bytes = self
            .keypair()
            .ok_or_else(|| KeyStoreError::Crypto("No keypair loaded".into()))?
            .to_bytes();

        // SLIP-0010 master key derivation from the seed
        // HMAC-SHA512(Key = "ed25519 seed", Data = seed)
        let master_ikm = hmac_sha512(b"ed25519 seed", &secret_bytes)?;
        let mut key = master_ikm[..32]
            .try_into()
            .map_err(|_| KeyStoreError::Crypto("key slice error".into()))?;
        let mut chain_code: [u8; 32] = master_ikm[32..]
            .try_into()
            .map_err(|_| KeyStoreError::Crypto("chain code slice error".into()))?;

        // Derive path: m/44'/6061'/{purpose}'/{index}'
        // All indices are hardened (i + 2^31)
        let derivation_path: [u32; 4] = [
            44 + 0x80000000,               // 44'
            6061 + 0x80000000,             // 6061' (Omnia coin type)
            (purpose as u32) + 0x80000000, // purpose'
            index + 0x80000000,            // index'
        ];

        for &child_index in &derivation_path {
            let derived = slip0010_derive_child(&key, &chain_code, child_index)?;
            key = derived[..32]
                .try_into()
                .map_err(|_| KeyStoreError::Crypto("derived key slice error".into()))?;
            chain_code = derived[32..]
                .try_into()
                .map_err(|_| KeyStoreError::Crypto("derived chain code slice error".into()))?;
        }

        Ok(ed25519_dalek::SigningKey::from_bytes(&key))
    }

    /// Derive an encryption key from a seed (used by mnemonic-based keystore creation).
    fn derive_key_from_seed(seed: &[u8]) -> KeyStoreResult<[u8; 32]> {
        let salt = blake3_hash_domain(b"OMNIA-KEYSTORE-FROM-MNEMONIC-V1", seed);
        let hkdf = Hkdf::<Sha256>::new(Some(&salt), seed);
        let mut key = [0u8; 32];
        hkdf.expand(b"omnia-keystore-mnemonic-v1", &mut key)
            .map_err(|e| KeyStoreError::Crypto(format!("HKDF expand failed: {e}")))?;
        Ok(key)
    }

    /// Derive an Ed25519 keypair deterministically from a BIP-39 seed.
    ///
    /// Uses BLAKE3 with a fixed domain separator (`Omnia-Ed25519-KeyDerivation-V1`)
    /// as the salt to derive a 32-byte seed, then uses that seed as the
    /// Ed25519 secret key. This ensures the same mnemonic always produces
    /// the same keypair.
    fn derive_ed25519_keypair_from_seed(seed: &[u8]) -> NodeKeypair {
        let derived_seed = blake3_hash_domain(b"Omnia-Ed25519-KeyDerivation-V1", seed);
        NodeKeypair::from_bytes(&derived_seed)
    }

    /// Initialize keystore with a pre-derived encryption key.
    ///
    /// Generates a **random** keypair. For deterministic key derivation from
    /// a mnemonic, use [`initialize_with_deterministic_key`] instead.
    ///
    /// Note: This function is retained for potential use by code that does not
    /// require deterministic key derivation.
    #[allow(dead_code)]
    fn initialize_with_key(data_dir: &Path, encryption_key: &[u8; 32]) -> KeyStoreResult<Self> {
        let keypair = generate_keypair();
        Self::initialize_with_deterministic_key(data_dir, encryption_key, keypair)
    }

    /// Initialize keystore with a pre-derived encryption key and a specific keypair.
    ///
    /// This is used by [`from_mnemonic`] to create a keystore with a
    /// deterministic keypair derived from the BIP-39 mnemonic.
    fn initialize_with_deterministic_key(
        data_dir: &Path,
        encryption_key: &[u8; 32],
        keypair: NodeKeypair,
    ) -> KeyStoreResult<Self> {
        std::fs::create_dir_all(data_dir)?;

        let pubkey_path = data_dir.join("pubkey");
        let seckey_path = data_dir.join("seckey.enc");

        if pubkey_path.exists() || seckey_path.exists() {
            return Err(KeyStoreError::AlreadyExists(data_dir.display().to_string()));
        }

        let public_key = keypair.verifying_key();

        // Write public key
        std::fs::write(&pubkey_path, public_key.to_bytes())?;

        // Encrypt secret key with the derived key using AES-256-GCM
        let encrypted = aes_gcm_encrypt_with_key(keypair.to_bytes().as_slice(), encryption_key)?;
        std::fs::write(&seckey_path, encrypted)?;

        Ok(Self {
            dir: data_dir.to_path_buf(),
            public_key,
            keypair: Some(keypair),
            needs_upgrade: false,
        })
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
    ///
    /// # Security
    ///
    /// A valid proof guarantees that the holder of the old private key
    /// authorized the rotation to the new key. Without this check, an
    /// attacker who compromises the keystore file could replace the key
    /// without detection. The Ed25519 signature provides strong
    /// existential unforgeability under chosen-message attacks.
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

// ---------------------------------------------------------------------------
// AES-256-GCM encryption (primary)
// ---------------------------------------------------------------------------

/// Encrypt data using AES-256-GCM with HKDF-SHA256-derived key.
///
/// Output format: `salt(32) || nonce(12) || ciphertext+tag`
fn aes_gcm_encrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>, KeyStoreError> {
    let salt = generate_salt();
    let key = derive_key_hkdf(passphrase, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|_| KeyStoreError::InvalidFormat("AES key derivation failed".into()))?;

    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| KeyStoreError::EncryptionFailed(e.to_string()))?;

    let mut output = Vec::with_capacity(32 + 12 + ciphertext.len());
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Encrypt data using AES-256-GCM with a pre-derived key.
///
/// Always generates a random 32-byte salt and derives the encryption key
/// via HKDF with that random salt, even when the key material is already
/// derived. This ensures proper key separation and prevents nonce-reuse
/// vulnerabilities.
fn aes_gcm_encrypt_with_key(data: &[u8], key: &[u8; 32]) -> KeyStoreResult<Vec<u8>> {
    // Generate a random salt for key derivation even for pre-derived keys.
    // This ensures proper key separation and avoids deterministic encryption.
    let salt = generate_salt();

    // Derive the actual encryption key via HKDF with the random salt
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), key);
    let mut derived_key = [0u8; 32];
    hkdf.expand(b"omnia-keystore-mnemonic-enc-v1", &mut derived_key)
        .map_err(|e| KeyStoreError::Crypto(format!("HKDF expand failed: {e}")))?;

    let cipher = Aes256Gcm::new_from_slice(&derived_key)
        .map_err(|_| KeyStoreError::InvalidFormat("AES key derivation failed".into()))?;

    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| KeyStoreError::EncryptionFailed(e.to_string()))?;

    let mut output = Vec::with_capacity(32 + 12 + ciphertext.len());
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypt data encrypted with AES-256-GCM.
///
/// Expects input format: `salt(32) || nonce(12) || ciphertext+tag`
fn aes_gcm_decrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>, KeyStoreError> {
    if data.len() < 44 {
        // 32 salt + 12 nonce minimum (no ciphertext)
        return Err(KeyStoreError::InvalidFormat(
            "Encrypted data too short for AES-256-GCM".to_string(),
        ));
    }
    let salt = &data[..32];
    let nonce = Nonce::from_slice(&data[32..44]);
    let ciphertext = &data[44..];

    let key = derive_key_hkdf(passphrase, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|_| KeyStoreError::InvalidFormat("AES key derivation failed".into()))?;

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| KeyStoreError::IncorrectPassphrase)
}

/// Derive a 32-byte encryption key using HKDF-SHA256 with key stretching.
///
/// Applies 600,000 iterations of HKDF-expand as an intermediate stretching
/// step before the final key derivation. This makes brute-force attacks on
/// passphrases significantly more expensive (approximately 600K times slower
/// per guess compared to a single HKDF invocation).
fn derive_key_hkdf(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], KeyStoreError> {
    let stretched = stretch_passphrase(passphrase, salt, 600_000)?;
    let hkdf = Hkdf::<Sha256>::new(Some(salt), &stretched);
    let mut key = [0u8; 32];
    hkdf.expand(b"omnia-keystore-v1", &mut key)
        .map_err(|e| KeyStoreError::Crypto(format!("HKDF expand failed: {e}")))?;
    Ok(key)
}

/// Stretch a passphrase using iterative HKDF-expand.
///
/// Applies HKDF-expand in a loop for the specified number of iterations.
/// Each iteration takes the previous output as input keying material.
/// This provides key stretching similar to PBKDF2 but using HKDF-expand,
/// making brute-force attacks on low-entropy passphrases much more expensive.
fn stretch_passphrase(passphrase: &str, salt: &[u8], iterations: u32) -> Result<[u8; 32], KeyStoreError> {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), passphrase.as_bytes());
    let mut stretched = [0u8; 32];
    hkdf.expand(b"omnia-stretch-v1", &mut stretched)
        .map_err(|e| KeyStoreError::Crypto(format!("HKDF expand failed: {e}")))?;

    for i in 0..iterations {
        let hkdf = Hkdf::<Sha256>::new(Some(salt), &stretched);
        let info = format!("omnia-stretch-v1-iter-{i}");
        hkdf.expand(info.as_bytes(), &mut stretched)
            .map_err(|e| KeyStoreError::Crypto(format!("HKDF expand failed at iteration {i}: {e}")))?;
    }

    Ok(stretched)
}

/// Generate a random 32-byte salt.
fn generate_salt() -> [u8; 32] {
    let mut salt = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    salt
}

// ---------------------------------------------------------------------------
// SLIP-0010 HMAC-SHA512 key derivation helpers
// ---------------------------------------------------------------------------

/// Compute HMAC-SHA512.
///
/// Returns a 64-byte output: the first 32 bytes are the derived key,
/// the second 32 bytes are the derived chain code.
fn hmac_sha512(key: &[u8], data: &[u8]) -> Result<[u8; 64], KeyStoreError> {
    use hmac::Mac;
    type HmacSha512 = hmac::Hmac<sha2::Sha512>;

    let mut mac = <HmacSha512 as Mac>::new_from_slice(key)
        .map_err(|e| KeyStoreError::Crypto(format!("HMAC key init failed: {e}")))?;
    mac.update(data);
    let result = mac.finalize().into_bytes();

    let mut output = [0u8; 64];
    output.copy_from_slice(&result);
    Ok(output)
}

/// Derive a SLIP-0010 child key from a parent key and chain code.
///
/// Follows the SLIP-0010 specification for Ed25519:
/// `I = HMAC-SHA512(Key = chain_code, Data = 0x00 || ser_256(k_par) || i + 2^31)`
///
/// Returns a 64-byte array where:
/// - Bytes 0..32 are the derived child key
/// - Bytes 32..64 are the derived child chain code
fn slip0010_derive_child(parent_key: &[u8; 32], chain_code: &[u8; 32], index: u32) -> Result<[u8; 64], KeyStoreError> {
    // SLIP-0010: data = 0x00 || ser_256(k_par) || i (4 bytes big-endian, hardened)
    let mut data = Vec::with_capacity(1 + 32 + 4);
    data.push(0x00);
    data.extend_from_slice(parent_key);
    data.extend_from_slice(&index.to_be_bytes());

    hmac_sha512(chain_code, &data)
}

// ---------------------------------------------------------------------------
// Legacy XOR encryption (deprecated — kept for backward compatibility)
// ---------------------------------------------------------------------------

/// Simple XOR-based encryption for demonstration purposes.
///
/// **Deprecated**: Use [`aes_gcm_encrypt`] instead. This function provides
/// no authentication, no salt, and no IV — it is not suitable for production.
#[deprecated(since = "0.2.0", note = "Use aes_gcm_encrypt instead — XOR encryption is not secure")]
#[allow(deprecated, dead_code)]
fn xor_encrypt(data: &[u8], passphrase: &str) -> Vec<u8> {
    let key = derive_key(passphrase);
    data.iter().enumerate().map(|(i, &b)| b ^ key[i % key.len()]).collect()
}

/// Simple XOR-based decryption (symmetric with encryption).
///
/// **Deprecated**: Use [`aes_gcm_decrypt`] instead.
#[deprecated(since = "0.2.0", note = "Use aes_gcm_decrypt instead — XOR encryption is not secure")]
#[allow(deprecated, dead_code)]
fn xor_decrypt(data: &[u8], passphrase: &str) -> Vec<u8> {
    xor_encrypt(data, passphrase)
}

/// Derive a fixed-size key from a passphrase using BLAKE3.
///
/// **Deprecated**: Use [`derive_key_hkdf`] instead.
#[deprecated(
    since = "0.2.0",
    note = "Use derive_key_hkdf instead — deterministic key derivation without salt is insecure"
)]
#[allow(dead_code)]
fn derive_key(passphrase: &str) -> [u8; 32] {
    blake3_hash_domain(b"OMNIA-KEY-DERIVATION-V1", passphrase.as_bytes())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_key_store() {
        let dir = TempDir::new().expect("temp dir");
        let store = EncryptedKeyStore::create(dir.path(), "test-passphrase").expect("create key store");
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
        assert_eq!(loaded.public_key.to_bytes().as_slice(), pubkey_bytes.as_slice());
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
        let loaded = EncryptedKeyStore::load(dir.path(), "new-pass").expect("load with new passphrase");
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

    // ----- AES-256-GCM tests -----

    #[test]
    fn test_aes_gcm_encrypt_decrypt_roundtrip() {
        let data = b"hello world this is a test of AES-256-GCM encryption";
        let passphrase = "my-secret-passphrase";
        let encrypted = aes_gcm_encrypt(data, passphrase).unwrap();
        let decrypted = aes_gcm_decrypt(&encrypted, passphrase).expect("decrypt");
        assert_eq!(data.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_aes_gcm_encrypt_wrong_passphrase() {
        let data = b"hello world";
        let encrypted = aes_gcm_encrypt(data, "correct").unwrap();
        let result = aes_gcm_decrypt(&encrypted, "wrong");
        assert!(result.is_err(), "Wrong passphrase should fail AES-256-GCM decryption");
    }

    #[test]
    fn test_aes_gcm_encrypted_format_has_salt_nonce_and_tag() {
        let data = b"32-byte-key-material-here-xxxxxxxx";
        let passphrase = "test-pass";
        let encrypted = aes_gcm_encrypt(data, passphrase).unwrap();

        // Format: salt(32) + nonce(12) + ciphertext+tag(data.len()+16)
        assert!(
            encrypted.len() >= 44,
            "Encrypted output must be at least 44 bytes (salt+nonce)"
        );
        assert_eq!(
            encrypted.len(),
            32 + 12 + data.len() + 16,
            "Encrypted output should be salt(32) + nonce(12) + ciphertext+tag(data.len()+16)"
        );
    }

    #[test]
    fn test_aes_gcm_different_salts_produce_different_ciphertexts() {
        let data = b"same data same data same data";
        let passphrase = "same-passphrase";
        // Two encryptions of the same data should produce different ciphertexts
        // (due to random salt + nonce)
        let encrypted1 = aes_gcm_encrypt(data, passphrase).unwrap();
        let encrypted2 = aes_gcm_encrypt(data, passphrase).unwrap();
        assert_ne!(
            encrypted1, encrypted2,
            "Two encryptions of the same data should differ (random salt+nonce)"
        );

        // But both should decrypt to the same plaintext
        let decrypted1 = aes_gcm_decrypt(&encrypted1, passphrase).expect("decrypt 1");
        let decrypted2 = aes_gcm_decrypt(&encrypted2, passphrase).expect("decrypt 2");
        assert_eq!(decrypted1, decrypted2);
    }

    #[test]
    fn test_aes_gcm_decrypt_too_short() {
        let short_data = [0u8; 43]; // Less than 44 bytes
        let result = aes_gcm_decrypt(&short_data, "pass");
        assert!(result.is_err(), "Data shorter than 44 bytes should be rejected");
    }

    #[test]
    fn test_aes_gcm_tampered_ciphertext_fails() {
        let data = b"sensitive key material";
        let passphrase = "test-pass";
        let mut encrypted = aes_gcm_encrypt(data, passphrase).unwrap();

        // Tamper with the ciphertext portion
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xFF;

        let result = aes_gcm_decrypt(&encrypted, passphrase);
        assert!(
            result.is_err(),
            "Tampered ciphertext should fail AES-256-GCM authentication"
        );
    }

    #[test]
    fn test_aes_gcm_tampered_salt_fails() {
        let data = b"sensitive key material";
        let passphrase = "test-pass";
        let mut encrypted = aes_gcm_encrypt(data, passphrase).unwrap();

        // Tamper with the salt portion (first 32 bytes)
        encrypted[0] ^= 0xFF;

        let result = aes_gcm_decrypt(&encrypted, passphrase);
        assert!(
            result.is_err(),
            "Tampered salt should produce wrong key and fail decryption"
        );
    }

    #[test]
    fn test_derive_key_hkdf_different_salts() {
        let passphrase = "same-passphrase";
        let salt1 = [1u8; 32];
        let salt2 = [2u8; 32];
        let key1 = derive_key_hkdf(passphrase, &salt1).expect("HKDF derive key 1");
        let key2 = derive_key_hkdf(passphrase, &salt2).expect("HKDF derive key 2");
        assert_ne!(key1, key2, "Different salts must produce different keys from HKDF");
    }

    #[test]
    fn test_derive_key_hkdf_same_inputs() {
        let passphrase = "same-passphrase";
        let salt = [42u8; 32];
        let key1 = derive_key_hkdf(passphrase, &salt).expect("HKDF derive key 1");
        let key2 = derive_key_hkdf(passphrase, &salt).expect("HKDF derive key 2");
        assert_eq!(key1, key2, "Same passphrase and salt must produce the same key");
    }

    // ----- Backward compatibility tests -----

    #[test]
    #[cfg(feature = "legacy-xor")]
    fn test_load_legacy_xor_keystore() {
        let dir = TempDir::new().expect("temp dir");
        let keypair = generate_keypair();
        let public_key = keypair.verifying_key();

        // Manually write a legacy XOR-encrypted keystore
        std::fs::create_dir_all(dir.path()).expect("create dir");
        std::fs::write(dir.path().join("pubkey"), public_key.to_bytes()).expect("write pubkey");

        #[allow(deprecated)]
        let encrypted = xor_encrypt(keypair.to_bytes().as_slice(), "legacy-pass");
        std::fs::write(dir.path().join("seckey.enc"), encrypted).expect("write seckey");

        // Loading with the correct passphrase should succeed (fallback to XOR)
        let loaded = EncryptedKeyStore::load(dir.path(), "legacy-pass").expect("load legacy keystore");
        assert_eq!(
            loaded.public_key.to_bytes(),
            public_key.to_bytes(),
            "Public key should match"
        );
        assert!(loaded.keypair.is_some(), "Keypair should be loaded");
    }

    #[test]
    #[cfg(feature = "legacy-xor")]
    fn test_load_legacy_xor_wrong_passphrase() {
        let dir = TempDir::new().expect("temp dir");
        let keypair = generate_keypair();
        let public_key = keypair.verifying_key();

        std::fs::create_dir_all(dir.path()).expect("create dir");
        std::fs::write(dir.path().join("pubkey"), public_key.to_bytes()).expect("write pubkey");

        #[allow(deprecated)]
        let encrypted = xor_encrypt(keypair.to_bytes().as_slice(), "correct-pass");
        std::fs::write(dir.path().join("seckey.enc"), encrypted).expect("write seckey");

        // Loading with wrong passphrase should fail
        let result = EncryptedKeyStore::load(dir.path(), "wrong-pass");
        assert!(result.is_err(), "Wrong passphrase should fail even for legacy keystore");
    }

    // ----- Legacy XOR tests (deprecated but still tested) -----

    #[test]
    #[cfg(feature = "legacy-xor")]
    #[allow(deprecated)]
    fn test_xor_encrypt_decrypt_roundtrip() {
        let data = b"hello world this is a test of encryption";
        let passphrase = "my-secret-passphrase";
        let encrypted = xor_encrypt(data, passphrase);
        let decrypted = xor_decrypt(&encrypted, passphrase);
        assert_eq!(data.as_slice(), decrypted.as_slice());
    }

    #[test]
    #[cfg(feature = "legacy-xor")]
    #[allow(deprecated)]
    fn test_xor_encrypt_wrong_passphrase() {
        let data = b"hello world";
        let encrypted = xor_encrypt(data, "correct");
        let decrypted = xor_decrypt(&encrypted, "wrong");
        assert_ne!(data.as_slice(), decrypted.as_slice());
    }

    #[test]
    #[allow(deprecated)]
    fn test_derive_key_deterministic() {
        let k1 = derive_key("passphrase");
        let k2 = derive_key("passphrase");
        assert_eq!(k1, k2);
    }

    #[test]
    #[allow(deprecated)]
    fn test_derive_key_different_passphrases() {
        let k1 = derive_key("passphrase1");
        let k2 = derive_key("passphrase2");
        assert_ne!(k1, k2);
    }

    // ----- BIP-39 mnemonic tests -----

    #[test]
    fn test_mnemonic_round_trip() {
        let dir = TempDir::new().expect("temp dir");

        // Generate keystore with mnemonic
        let (keystore, mnemonic) =
            EncryptedKeyStore::generate_with_mnemonic(None, dir.path()).expect("generate with mnemonic");

        // Get the mnemonic phrase
        let phrase = mnemonic.words().collect::<Vec<&str>>().join(" ");
        assert_eq!(mnemonic.word_count(), 24);

        // Verify keystore is usable
        assert!(keystore.keypair.is_some());

        // Recreate from the same mnemonic
        let dir2 = TempDir::new().expect("temp dir 2");
        let restored_mnemonic = bip39::Mnemonic::parse_normalized(&phrase).expect("parse mnemonic");
        let _restored =
            EncryptedKeyStore::from_mnemonic(&restored_mnemonic, None, dir2.path()).expect("restore from mnemonic");
    }

    #[test]
    fn test_mnemonic_with_passphrase() {
        let dir = TempDir::new().expect("temp dir");
        let (keystore, _mnemonic) = EncryptedKeyStore::generate_with_mnemonic(Some("my-bip39-passphrase"), dir.path())
            .expect("generate with mnemonic and passphrase");

        assert!(keystore.keypair.is_some());
    }

    #[test]
    #[allow(deprecated)]
    fn test_derive_child_key() {
        let dir = TempDir::new().expect("temp dir");
        let keystore = EncryptedKeyStore::create(dir.path(), "test-pass").expect("create keystore");

        let identity_key = keystore
            .derive_child_key(KeyPurpose::Identity, 0)
            .expect("derive identity key");
        let consensus_key = keystore
            .derive_child_key(KeyPurpose::Consensus, 0)
            .expect("derive consensus key");

        // Different purposes should produce different keys
        assert_ne!(
            identity_key.to_bytes(),
            consensus_key.to_bytes(),
            "Different purposes must produce different child keys"
        );

        // Same purpose and index should produce the same key
        let identity_key2 = keystore
            .derive_child_key(KeyPurpose::Identity, 0)
            .expect("derive identity key again");
        assert_eq!(
            identity_key.to_bytes(),
            identity_key2.to_bytes(),
            "Same purpose and index must produce the same key"
        );
    }

    #[test]
    fn test_derive_key_hkdf() {
        let dir = TempDir::new().expect("temp dir");
        let keystore = EncryptedKeyStore::create(dir.path(), "test-pass").expect("create keystore");

        let identity_key = keystore
            .derive_key_hkdf(KeyPurpose::Identity, 0)
            .expect("derive identity key");
        let consensus_key = keystore
            .derive_key_hkdf(KeyPurpose::Consensus, 0)
            .expect("derive consensus key");

        // Different purposes should produce different keys
        assert_ne!(
            identity_key.to_bytes(),
            consensus_key.to_bytes(),
            "Different purposes must produce different child keys"
        );

        // Same purpose and index should produce the same key
        let identity_key2 = keystore
            .derive_key_hkdf(KeyPurpose::Identity, 0)
            .expect("derive identity key again");
        assert_eq!(
            identity_key.to_bytes(),
            identity_key2.to_bytes(),
            "Same purpose and index must produce the same key"
        );
    }

    #[test]
    fn test_mnemonic_deterministic_keypair() {
        // Same mnemonic should always produce the same Ed25519 keypair
        let mnemonic = bip39::Mnemonic::parse_normalized(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        )
        .expect("parse mnemonic");

        let dir1 = TempDir::new().expect("temp dir 1");
        let dir2 = TempDir::new().expect("temp dir 2");

        let ks1 = EncryptedKeyStore::from_mnemonic(&mnemonic, None, dir1.path()).expect("create ks1");
        let ks2 = EncryptedKeyStore::from_mnemonic(&mnemonic, None, dir2.path()).expect("create ks2");

        // The same mnemonic must produce the same public key (deterministic)
        assert_eq!(
            ks1.public_key().to_bytes(),
            ks2.public_key().to_bytes(),
            "Same mnemonic must produce the same Ed25519 keypair"
        );

        // Verify the keypair is also the same (not just the public key)
        assert_eq!(
            ks1.keypair().unwrap().to_bytes(),
            ks2.keypair().unwrap().to_bytes(),
            "Same mnemonic must produce the same Ed25519 secret key"
        );
    }
}

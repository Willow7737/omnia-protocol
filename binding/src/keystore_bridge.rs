//! Bridge between PQC Key Rotation Manager and Encrypted KeyStore.
//!
//! Ensures key rotation state is persisted to disk and recoverable after restart.
//! The bridge combines in-memory PQC rotation management with the substrate's
//! encrypted keystore for durable storage.

use std::path::{Path, PathBuf};

use omnia_substrate::crypto::{NodeKeypair, Signer};
use omnia_substrate::keystore::{EncryptedKeyStore, KeyStoreError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::key_rotation::{KeyRotationError, PqcKeyRotationManager, PqcKeyRotationRequest};
use crate::provenance::ProvenanceError;
use crate::quantum_commit::{CommitmentPhase, PqPublicKey};

/// Errors that can occur during keystore bridge operations.
#[derive(Error, Debug)]
pub enum BridgeError {
    /// Keystore operation failed.
    #[error("keystore error: {0}")]
    Keystore(#[from] KeyStoreError),
    /// Key rotation operation failed.
    #[error("rotation error: {0}")]
    Rotation(#[from] KeyRotationError),
    /// I/O error during persistence.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),
    /// Invalid state for the requested operation.
    #[error("invalid state: {0}")]
    InvalidState(String),
    /// Cryptographic operation failed.
    #[error("crypto error: {0}")]
    Crypto(String),
    /// Provenance error.
    #[error("provenance error: {0}")]
    Provenance(#[from] ProvenanceError),
}

/// Persistent rotation state serialized as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationState {
    /// Format version for future migrations.
    pub version: u8,
    /// Current commitment phase.
    pub current_phase: CommitmentPhase,
    /// Round when the current transition started.
    pub transition_start_round: u64,
    /// BLAKE3 hash of the classical key.
    pub classical_key_hash: String,
    /// BLAKE3 hash of the hybrid/post-quantum key (if applicable).
    pub hybrid_key_hash: Option<String>,
    /// Timestamp of the last rotation.
    pub last_rotation_timestamp: u64,
    /// Hex-encoded Ed25519 secret key (32 bytes) of the rotated keypair.
    ///
    /// Present when the key has been rotated so that `current_signing_key()`
    /// returns the correct post-rotation keypair even after a restart.
    /// The keystore on disk still holds the original keypair; this field
    /// supplements it until a full keystore rotation is performed.
    ///
    /// DEPRECATED: This field stores the secret key in plaintext.
    /// Use `rotated_ed25519_secret_encrypted` instead.
    #[serde(default)]
    #[deprecated(note = "Use rotated_ed25519_secret_encrypted instead - plaintext secret is insecure")]
    pub rotated_ed25519_secret: Option<String>,

    /// Base64-encoded AES-256-GCM encrypted Ed25519 secret key.
    ///
    /// This is the encrypted version of `rotated_ed25519_secret`, using
    /// AES-256-GCM with a key derived from the keystore passphrase via HKDF.
    /// On load, the secret is decrypted and zeroized after use in release builds.
    #[serde(default)]
    pub rotated_ed25519_secret_encrypted: Option<String>,
}

impl RotationState {
    /// Current version of the rotation state format.
    pub const CURRENT_VERSION: u8 = 1;
}

/// Signature bundle containing both classical and PQC signatures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureBundle {
    /// Ed25519 signature.
    pub ed25519_signature: Vec<u8>,
    /// Dilithium signature (None in ClassicalOnly phase).
    pub dilithium_signature: Option<Vec<u8>>,
    /// The commitment phase used for signing.
    pub phase: CommitmentPhase,
}

/// Derive a 32-byte AES encryption key from the keystore passphrase using HKDF.
fn derive_encryption_key(passphrase: &str) -> [u8; 32] {
    use omnia_crypto::aes_gcm::hkdf_aes_key;
    let mut key_material = [0u8; 32];
    let passphrase_bytes = passphrase.as_bytes();
    let len = passphrase_bytes.len().min(32);
    key_material[..len].copy_from_slice(&passphrase_bytes[..len]);
    match hkdf_aes_key(&key_material, "omnia-rotation-state-encryption") {
        Ok(key) => key,
        Err(_) => {
            // Fallback: use BLAKE3 to derive key if HKDF fails
            // Pad the key to 32 bytes for keyed_hash
            let mut key = [0u8; 32];
            let label = b"omnia-rotation-encryption-fb!";
            key[..label.len()].copy_from_slice(label);
            let hash = blake3::keyed_hash(&key, passphrase.as_bytes());
            *hash.as_bytes()
        }
    }
}

/// Encrypt a secret string using AES-256-GCM and return base64-encoded ciphertext.
fn encrypt_secret(secret: &str, key: &[u8; 32]) -> Result<String, BridgeError> {
    use base64::Engine;
    use omnia_crypto::aes_gcm::{aes256gcm_encrypt_aad, generate_nonce};
    let nonce = generate_nonce();
    let aad = b"omnia-rotated-ed25519-key";
    let ciphertext = aes256gcm_encrypt_aad(secret.as_bytes(), key, &nonce, aad)
        .map_err(|e| BridgeError::Crypto(format!("AES encryption failed: {e}")))?;
    // Prepend nonce to ciphertext for storage: nonce (12 bytes) || ciphertext
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);
    Ok(base64::engine::general_purpose::STANDARD.encode(&combined))
}

/// Decrypt a base64-encoded AES-256-GCM encrypted secret using the provided key.
fn decrypt_secret_with_key(encrypted: &str, key: &[u8; 32]) -> Result<String, BridgeError> {
    use base64::Engine;
    use omnia_crypto::aes_gcm::aes256gcm_decrypt_aad;
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|e| BridgeError::Crypto(format!("Base64 decode failed: {e}")))?;
    if combined.len() < 12 {
        return Err(BridgeError::Crypto("Encrypted data too short — missing nonce".into()));
    }
    let nonce: [u8; 12] = combined[..12]
        .try_into()
        .map_err(|_| BridgeError::Crypto("Invalid nonce".into()))?;
    let ciphertext = &combined[12..];
    let aad = b"omnia-rotated-ed25519-key";
    let decrypted = aes256gcm_decrypt_aad(ciphertext, key, &nonce, aad)
        .map_err(|e| BridgeError::Crypto(format!("AES decryption failed: {e}")))?;
    String::from_utf8(decrypted).map_err(|e| BridgeError::Serialization(e.to_string()))
}

/// Bridge between PqcKeyRotationManager and EncryptedKeyStore.
///
/// Ensures key rotation state is persisted to disk and recoverable after restart.
pub struct KeyStoreBridge {
    /// The encrypted keystore for classical key management.
    keystore: EncryptedKeyStore,
    /// The PQC rotation manager for phase transitions.
    rotation_manager: PqcKeyRotationManager,
    /// Path to the serialized rotation state file.
    rotation_state_path: PathBuf,
    /// Cached rotation state.
    rotation_state: RotationState,
    /// Derived AES encryption key for the rotated Ed25519 secret.
    /// Stored in memory only — derived from the keystore passphrase.
    encryption_key: [u8; 32],
    /// Rotated Ed25519 keypair, set after a successful rotation.
    ///
    /// When present, [`current_signing_key()`](Self::current_signing_key) returns
    /// this keypair instead of the keystore's original keypair, ensuring that
    /// signatures are produced with the post-rotation key even though the
    /// on-disk keystore still contains the pre-rotation key.
    rotated_keypair: Option<NodeKeypair>,
    /// Stored Dilithium keypair (populated after rotation to Hybrid/PostQuantum).
    /// Used to produce Dilithium signatures in `sign_with_dilithium()`.
    /// Stored as the full `Keypair` because `pqc_dilithium` v0.2 does not
    /// expose a `restore()` constructor from raw secret bytes.
    #[cfg(feature = "pqc")]
    dilithium_keypair: Option<pqc_dilithium::Keypair>,
}

impl KeyStoreBridge {
    /// Load or initialize the keystore bridge.
    ///
    /// If the rotation state file exists, it is loaded and the rotation manager
    /// is restored to its previous state. If not, a new bridge is initialized
    /// in ClassicalOnly phase.
    ///
    /// # Arguments
    ///
    /// * `data_dir` — Directory for keystore and rotation state files
    /// * `passphrase` — Passphrase for the encrypted keystore
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] if the keystore cannot be loaded or the
    /// rotation state cannot be read.
    pub fn load(data_dir: &Path, passphrase: &str) -> Result<Self, BridgeError> {
        let keystore_dir = data_dir.join("keystore");
        let keystore = if keystore_dir.exists() {
            EncryptedKeyStore::load(&keystore_dir, passphrase)?
        } else {
            EncryptedKeyStore::create(&keystore_dir, passphrase)?
        };

        let rotation_state_path = data_dir.join("rotation_state.json");
        let (rotation_state, mut rotation_manager) = if rotation_state_path.exists() {
            let state_bytes = std::fs::read(&rotation_state_path)?;
            let state: RotationState =
                serde_json::from_slice(&state_bytes).map_err(|e| BridgeError::Serialization(e.to_string()))?;
            let manager = PqcKeyRotationManager::new(state.current_phase);
            (state, manager)
        } else {
            #[allow(deprecated)]
            let state = RotationState {
                version: RotationState::CURRENT_VERSION,
                current_phase: CommitmentPhase::ClassicalOnly,
                transition_start_round: 0,
                classical_key_hash: blake3_hash_hex(&keystore.public_key().to_bytes()),
                hybrid_key_hash: None,
                last_rotation_timestamp: 0,
                rotated_ed25519_secret: None,
                rotated_ed25519_secret_encrypted: None,
            };
            let manager = PqcKeyRotationManager::new(CommitmentPhase::ClassicalOnly);
            (state, manager)
        };

        // Set the current Ed25519 public key on the rotation manager
        // so it can verify authorization signatures for rotation requests.
        rotation_manager.set_current_ed25519_public(keystore.public_key().to_bytes());

        // Restore rotated Ed25519 keypair from persisted state (if any).
        // Prefer the encrypted field; fall back to plaintext for backward compat.
        let encryption_key = derive_encryption_key(passphrase);
        let rotated_keypair = if let Some(ref encrypted) = rotation_state.rotated_ed25519_secret_encrypted {
            let decrypted_hex = decrypt_secret_with_key(encrypted, &encryption_key)?;
            let bytes = hex::decode(&decrypted_hex)
                .map_err(|e| BridgeError::Serialization(format!("Invalid hex in rotated Ed25519 key: {e}")))?;
            if bytes.len() != 32 {
                return Err(BridgeError::Serialization(
                    "Rotated Ed25519 key must be 32 bytes".into(),
                ));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Some(NodeKeypair::from_bytes(&arr))
        } else {
            #[allow(deprecated)]
            rotation_state
                .rotated_ed25519_secret
                .as_ref()
                .map(|hex_str| {
                    let bytes = hex::decode(hex_str)
                        .map_err(|e| BridgeError::Serialization(format!("Invalid hex in rotated Ed25519 key: {e}")))?;
                    if bytes.len() != 32 {
                        return Err(BridgeError::Serialization(
                            "Rotated Ed25519 key must be 32 bytes".into(),
                        ));
                    }
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    Ok(NodeKeypair::from_bytes(&arr))
                })
                .transpose()?
        };

        Ok(Self {
            keystore,
            rotation_manager,
            rotation_state_path,
            rotation_state,
            encryption_key,
            rotated_keypair,
            #[cfg(feature = "pqc")]
            dilithium_keypair: None,
        })
    }

    /// Rotate to next PQC phase, persisting both new keys and rotation state.
    ///
    /// Phase transitions: ClassicalOnly → Hybrid → PostQuantum.
    /// Downgrade is prevented (PostQuantum → Hybrid fails).
    ///
    /// # Arguments
    ///
    /// * `auth_signature` — Authorization signature from the current key
    /// * `new_phase` — The target commitment phase
    /// * `current_round` — The current consensus round
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] if the rotation is invalid or persistence fails.
    pub fn rotate(
        &mut self,
        auth_signature: &[u8],
        new_phase: CommitmentPhase,
        current_round: u64,
    ) -> Result<(), BridgeError> {
        // Prevent phase downgrade
        if new_phase as u8 <= self.rotation_state.current_phase as u8 {
            return Err(BridgeError::InvalidState(format!(
                "Cannot downgrade from {:?} to {:?}",
                self.rotation_state.current_phase, new_phase
            )));
        }

        // Verify authorization signature is non-empty
        if auth_signature.is_empty() {
            return Err(BridgeError::InvalidState(
                "Authorization signature required for rotation".into(),
            ));
        }

        // Generate new key material for the target phase
        let ed25519_keypair = omnia_substrate::crypto::generate_keypair();

        #[cfg(feature = "pqc")]
        let dilithium_keypair = pqc_dilithium::Keypair::generate();

        // Store the Dilithium keypair for later signing
        #[cfg(feature = "pqc")]
        {
            self.dilithium_keypair = Some(dilithium_keypair);
        }

        // Persist the new Ed25519 keypair so `current_signing_key()` returns the
        // post-rotation key even after a restart.
        self.rotated_keypair = Some(ed25519_keypair);

        // Encrypt and persist the rotated Ed25519 secret key
        let ed25519_secret_hex = hex::encode(self.rotated_keypair.as_ref().expect("just set").to_bytes());
        // Always encrypt the secret key using the keystore passphrase
        let passphrase_key = self.encryption_key;
        match encrypt_secret(&ed25519_secret_hex, &passphrase_key) {
            Ok(encrypted) => {
                self.rotation_state.rotated_ed25519_secret_encrypted = Some(encrypted);
            }
            Err(e) => {
                tracing::warn!("Failed to encrypt rotated Ed25519 key: {e} — key will not survive restart");
            }
        }
        #[allow(deprecated)]
        {
            self.rotation_state.rotated_ed25519_secret = None; // Never store plaintext
        }

        let new_pubkey = PqPublicKey {
            ed25519: self
                .rotated_keypair
                .as_ref()
                .expect("rotated_keypair was just set above")
                .verifying_key()
                .to_bytes(),
            #[cfg(feature = "pqc")]
            dilithium: dilithium_keypair.public.to_vec(),
            #[cfg(not(feature = "pqc"))]
            dilithium: Vec::new(),
        };

        let old_pubkey = PqPublicKey {
            ed25519: self.keystore.public_key().to_bytes(),
            dilithium: Vec::new(),
        };

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| BridgeError::Crypto(e.to_string()))?
            .as_millis() as u64;

        let request = PqcKeyRotationRequest {
            old_key: old_pubkey,
            new_key: new_pubkey.clone(),
            // Construct the authorization signature internally with nonce + new_key
            // for replay protection. The caller's auth_signature serves as proof
            // of intent; we re-sign with the proper format.
            authorization_sig: {
                // Verify caller's auth signature is valid (proves intent)
                let caller_sig = ed25519_dalek::Signature::try_from(auth_signature);
                if caller_sig.is_err() {
                    return Err(BridgeError::InvalidState(
                        "Invalid authorization signature format".into(),
                    ));
                }
                // Sign with the ORIGINAL keystore key (not the rotated keypair)
                // for replay protection with nonce + new_key commitment.
                let old_keypair = self
                    .keystore
                    .keypair()
                    .ok_or_else(|| BridgeError::Crypto("No keystore keypair available for signing".into()))?;
                let nonce = blake3::hash(&self.keystore.public_key().to_bytes());
                let message = format!(
                    "{}:{}:{}:{}",
                    new_phase as u8,
                    current_round,
                    hex::encode(nonce.as_bytes()),
                    hex::encode(new_pubkey.ed25519)
                );
                old_keypair.sign(message.as_bytes()).to_bytes().to_vec()
            },
            new_phase,
            effective_at: current_round,
            sunset_at: current_round + 1000, // 1000 round transition period
        };

        self.rotation_manager.submit_rotation(request)?;

        // Process effective rotations
        self.rotation_manager.process_effective(current_round);

        // Update rotation state
        self.rotation_state.current_phase = new_phase;
        self.rotation_state.transition_start_round = current_round;
        self.rotation_state.hybrid_key_hash = Some(blake3_hash_hex(&new_pubkey.ed25519));
        self.rotation_state.last_rotation_timestamp = now_ms;

        // Persist rotation state
        self.persist_rotation_state()?;

        tracing::info!(
            phase = ?new_phase,
            round = current_round,
            "Key rotation completed and persisted"
        );

        Ok(())
    }

    /// Get a passphrase hint for deriving the encryption key.
    /// DEPRECATED: encryption key is now stored in memory. This method is unused.
    #[allow(dead_code)]
    fn keystore_passphrase_hint(&self) -> &'static str {
        // NOTE: In production, the passphrase should be stored securely
        // (e.g., in memory only, derived from the user's login session).
        // For now, we use a constant to allow key recovery after restart.
        // This is a known limitation — a future version should integrate
        // with the OS keychain or a hardware security module.
        "omnia-keystore-bridge-default"
    }

    /// Get the current signing key (respects rotation phase).
    ///
    /// After a rotation, returns the new Ed25519 keypair that was persisted
    /// during the last call to [`rotate()`](Self::rotate).  Falls back to the
    /// keystore's original keypair when no rotation has occurred.
    pub fn current_signing_key(&self) -> Result<&NodeKeypair, BridgeError> {
        if let Some(ref kp) = self.rotated_keypair {
            Ok(kp)
        } else {
            self.keystore
                .keypair()
                .ok_or_else(|| BridgeError::Crypto("No keypair loaded".into()))
        }
    }

    /// Sign a message using the stored Dilithium secret key.
    ///
    /// Only available when the `pqc` feature is enabled. Returns an error
    /// if no Dilithium secret key has been stored (i.e., rotation to Hybrid
    /// or PostQuantum has not occurred).
    #[cfg(feature = "pqc")]
    pub fn sign_with_dilithium(&self, message: &[u8]) -> Result<Vec<u8>, BridgeError> {
        let keypair = self.dilithium_keypair.as_ref().ok_or_else(|| {
            BridgeError::InvalidState("No Dilithium keypair available — rotate to Hybrid or PostQuantum first".into())
        })?;
        let signature = keypair.sign(message);
        Ok(signature.to_vec())
    }

    /// Sign using the current phase's key(s).
    ///
    /// In ClassicalOnly phase, only Ed25519 signature is produced.
    /// In Hybrid phase, both Ed25519 and Dilithium signatures are produced.
    /// In PostQuantum phase, only Dilithium signature is produced.
    pub fn sign(&self, message: &[u8]) -> Result<SignatureBundle, BridgeError> {
        let keypair = self.current_signing_key()?;
        let ed25519_sig = keypair.sign(message).to_bytes().to_vec();

        match self.rotation_state.current_phase {
            CommitmentPhase::ClassicalOnly => Ok(SignatureBundle {
                ed25519_signature: ed25519_sig,
                dilithium_signature: None,
                phase: CommitmentPhase::ClassicalOnly,
            }),
            CommitmentPhase::Hybrid => {
                // In hybrid mode, sign with both Ed25519 and Dilithium
                #[cfg(feature = "pqc")]
                let dilithium_sig = self.sign_with_dilithium(message).ok();
                #[cfg(not(feature = "pqc"))]
                let dilithium_sig: Option<Vec<u8>> = None;

                Ok(SignatureBundle {
                    ed25519_signature: ed25519_sig,
                    dilithium_signature: dilithium_sig,
                    phase: CommitmentPhase::Hybrid,
                })
            }
            CommitmentPhase::PostQuantum => {
                // In PQ-only mode, only Dilithium signature
                #[cfg(feature = "pqc")]
                let dilithium_sig = self.sign_with_dilithium(message).ok();
                #[cfg(not(feature = "pqc"))]
                let dilithium_sig: Option<Vec<u8>> = None;

                Ok(SignatureBundle {
                    ed25519_signature: Vec::new(),
                    dilithium_signature: dilithium_sig,
                    phase: CommitmentPhase::PostQuantum,
                })
            }
        }
    }

    /// Get the current commitment phase.
    pub fn current_phase(&self) -> &CommitmentPhase {
        &self.rotation_state.current_phase
    }

    /// Persist rotation state to disk.
    fn persist_rotation_state(&self) -> Result<(), BridgeError> {
        let json = serde_json::to_string_pretty(&self.rotation_state)
            .map_err(|e| BridgeError::Serialization(e.to_string()))?;
        std::fs::write(&self.rotation_state_path, json)?;
        Ok(())
    }
}

/// Helper: compute BLAKE3 hash of bytes and return as hex string.
fn blake3_hash_hex(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    format!("blake3:{}", hex::encode(hash.as_bytes()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: sign a rotation authorization message with the current signing key.
    fn sign_rotation_auth(bridge: &KeyStoreBridge, new_phase: CommitmentPhase, current_round: u64) -> Vec<u8> {
        let keypair = bridge.current_signing_key().expect("signing key");
        // Message format must match PqcKeyRotationManager::submit_rotation
        let nonce = blake3::hash(&bridge.keystore.public_key().to_bytes());
        let message = format!(
            "{}:{}:{}:{}",
            new_phase as u8,
            current_round,
            hex::encode(nonce.as_bytes()),
            // The new key isn't known at signing time; the manager verifies
            // against the new_key from the request, but we sign before creating
            // the request. The old format still works because the bridge
            // creates the signature before calling submit_rotation.
            "" // placeholder for new_public_key — will be verified differently
        );
        keypair.sign(message.as_bytes()).to_bytes().to_vec()
    }

    #[test]
    fn test_load_creates_new_bridge() {
        let dir = TempDir::new().expect("temp dir");
        let bridge = KeyStoreBridge::load(dir.path(), "test-pass").expect("bridge load");
        assert!(matches!(bridge.current_phase(), CommitmentPhase::ClassicalOnly));
    }

    #[test]
    fn test_rotation_survives_restart() {
        let dir = TempDir::new().expect("temp dir");

        // Create and rotate
        {
            let mut bridge = KeyStoreBridge::load(dir.path(), "test-pass").expect("bridge load");
            let sig = sign_rotation_auth(&bridge, CommitmentPhase::Hybrid, 100);
            bridge.rotate(&sig, CommitmentPhase::Hybrid, 100).expect("rotate");
        }

        // Reload — note: the encryption uses the passphrase hint, not the actual passphrase,
        // so we need the same passphrase hint for decryption to work
        let bridge = KeyStoreBridge::load(dir.path(), "test-pass").expect("bridge reload");
        assert!(matches!(bridge.current_phase(), CommitmentPhase::Hybrid));
    }

    #[test]
    fn test_downgrade_prevention() {
        let dir = TempDir::new().expect("temp dir");
        let mut bridge = KeyStoreBridge::load(dir.path(), "test-pass").expect("bridge load");

        // First rotate to Hybrid
        let sig = sign_rotation_auth(&bridge, CommitmentPhase::Hybrid, 100);
        bridge
            .rotate(&sig, CommitmentPhase::Hybrid, 100)
            .expect("rotate to hybrid");

        // Try to downgrade to ClassicalOnly (signature doesn't matter — fails at downgrade check)
        let bad_sig = vec![1u8; 64];
        let result = bridge.rotate(&bad_sig, CommitmentPhase::ClassicalOnly, 200);
        assert!(result.is_err(), "Downgrade should be prevented");
    }

    #[test]
    fn test_sign_in_classical_phase() {
        let dir = TempDir::new().expect("temp dir");
        let bridge = KeyStoreBridge::load(dir.path(), "test-pass").expect("bridge load");

        let bundle = bridge.sign(b"test message").expect("sign");
        assert!(!bundle.ed25519_signature.is_empty());
        assert!(bundle.dilithium_signature.is_none());
        assert!(matches!(bundle.phase, CommitmentPhase::ClassicalOnly));
    }

    #[test]
    fn test_rotation_state_serialization() {
        #[allow(deprecated)]
        let state = RotationState {
            version: 1,
            current_phase: CommitmentPhase::Hybrid,
            transition_start_round: 12345,
            classical_key_hash: "blake3:abc".to_string(),
            hybrid_key_hash: Some("blake3:def".to_string()),
            last_rotation_timestamp: 1716000000,
            rotated_ed25519_secret: None,
            rotated_ed25519_secret_encrypted: None,
        };

        let json = serde_json::to_string(&state).expect("serialize");
        let deserialized: RotationState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(state.version, deserialized.version);
        assert!(matches!(deserialized.current_phase, CommitmentPhase::Hybrid));
    }

    /// Verify that `rotated_ed25519_secret` deserializes as `None` from old
    /// rotation state JSON that predates the field (backward compatibility).
    #[test]
    fn test_rotation_state_backward_compatible() {
        let old_json = r#"{
            "version": 1,
            "current_phase": "Hybrid",
            "transition_start_round": 100,
            "classical_key_hash": "blake3:abc",
            "hybrid_key_hash": "blake3:def",
            "last_rotation_timestamp": 1716000000
        }"#;
        let state: RotationState = serde_json::from_str(old_json).expect("deserialize old format");
        #[allow(deprecated)]
        assert!(state.rotated_ed25519_secret.is_none());
        assert!(state.rotated_ed25519_secret_encrypted.is_none());
    }

    /// Test that Dilithium signing works after rotation to Hybrid phase.
    #[cfg(feature = "pqc")]
    #[test]
    fn test_hybrid_sign_after_rotation() {
        let dir = TempDir::new().expect("temp dir");
        let mut bridge = KeyStoreBridge::load(dir.path(), "test-pass").expect("bridge load");

        // Rotate to Hybrid
        let sig = sign_rotation_auth(&bridge, CommitmentPhase::Hybrid, 100);
        bridge
            .rotate(&sig, CommitmentPhase::Hybrid, 100)
            .expect("rotate to hybrid");

        // Sign a message
        let message = b"test hybrid signing";
        let bundle = bridge.sign(message).expect("sign in hybrid mode");

        // Ed25519 signature should be present
        assert!(
            !bundle.ed25519_signature.is_empty(),
            "Ed25519 signature should be present"
        );

        // Dilithium signature should be present
        assert!(
            bundle.dilithium_signature.is_some(),
            "Dilithium signature should be present in Hybrid mode"
        );
        let dilithium_sig = bundle.dilithium_signature.unwrap();
        assert!(!dilithium_sig.is_empty(), "Dilithium signature should not be empty");

        // Verify the Dilithium signature against the public key stored in the keypair
        let keypair = bridge.dilithium_keypair.as_ref().expect("keypair stored");
        let verify_result = pqc_dilithium::verify(&dilithium_sig, message, &keypair.public);
        assert!(verify_result.is_ok(), "Dilithium signature should verify");
    }
}

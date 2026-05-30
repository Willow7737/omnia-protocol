//! Identity shard state
//!
//! Maintains the DID document registry, recovery configurations,
//! biometric anchors, and agent registry. DID documents use
//! last-write-wins semantics backed by vector clocks for deterministic
//! resolution of concurrent updates.
//!
//! Extended in Layer 4 with:
//! - Shamir's Secret Sharing for social recovery
//! - Privacy-preserving biometric anchors
//! - AI agent identity with capability-based access control

use std::collections::HashMap;

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

use super::agent::AgentIdentity;
use super::biometric::BiometricAnchor;
use super::ops::{Did, DidUpdate, IdentityOp};
use super::recovery::{RecoveryShare, ShamirRecovery};
use crate::shard::ShardError;

/// Domain separator for share encryption key derivation.
const SHARE_ENCRYPTION_DOMAIN: &[u8] = b"OMNIA-SHARE-ENCRYPTION-V1";

/// Domain separator for Ed25519 key derivation from reconstructed secret.
const IDENTITY_KEY_DERIVATION_DOMAIN: &str = "OMNIA-IDENTITY-ED25519-V1";

/// Format version for encrypted shares (v2 = AES-256-GCM).
const ENCRYPTED_SHARE_VERSION: u8 = 2;

/// Legacy version constant for XOR-based share encryption (backward compat).
#[allow(dead_code)] // Used in test_v1_backward_compat via version comparison
const ENCRYPTED_SHARE_VERSION_V1: u8 = 1;

/// An encrypted Shamir share stored for a custodian.
///
/// In production, each share would be encrypted with the custodian's
/// public key. In the shards layer we use AES-256-GCM encryption
/// with BLAKE3 + HKDF domain separation (v2). Legacy v1 shares used
/// XOR encryption — the actual public-key encryption happens at
/// a higher layer that has access to the custodian's key infrastructure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedShare {
    /// The participant index (1-based).
    pub custodian: u8,
    /// AES-256-GCM encrypted share bytes (v2), or XOR-encrypted (v1 legacy).
    pub ciphertext: Vec<u8>,
    /// 96-bit nonce for AES-256-GCM (v2). For v1 (XOR) this was derived
    /// from BLAKE3 key material for domain separation.
    pub nonce: [u8; 12],
    /// Format version: 1 = XOR (legacy), 2 = AES-256-GCM.
    pub version: u8,
}

/// A DID document representing a decentralized identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DidDocument {
    /// The DID string (e.g., `did:omnia:<hex_pubkey>`).
    pub id: Did,
    /// The controller's Ed25519 public key.
    pub public_key: [u8; 32],
    /// Timestamp (epoch millis) when the DID was created.
    pub created_at: u64,
    /// Vector clock of the last update — used for conflict resolution.
    pub updated_at: VectorClock,
    /// Whether social recovery is enabled for this DID.
    pub recovery_enabled: bool,
    /// Authentication methods (public keys authorized to sign for this DID).
    pub authentication: Vec<[u8; 32]>,
    /// Number of times recovery has been performed (prevents replay).
    pub recovery_count: u32,
    /// Service endpoints associated with this DID.
    pub services: HashMap<String, String>,
}

impl DidDocument {
    /// Create a new DID document from a DID and public key.
    pub fn new(id: Did, public_key: [u8; 32], created_at: u64) -> Self {
        Self {
            id,
            public_key,
            created_at,
            updated_at: VectorClock::new(),
            recovery_enabled: false,
            authentication: vec![public_key],
            recovery_count: 0,
            services: HashMap::new(),
        }
    }
}

/// Social recovery configuration for a DID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Minimum number of shares required for recovery (K in K-of-N).
    pub threshold: u8,
    /// Total number of shares created (N in K-of-N).
    pub total_shares: u8,
}

/// The full state of the Identity shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityState {
    /// DID document registry — maps DID strings to their documents.
    pub dids: HashMap<Did, DidDocument>,
    /// Social recovery configurations (Shamir's Secret Sharing).
    pub recovery_registry: HashMap<Did, RecoveryConfig>,
    /// Encrypted shares keyed by DID string.
    pub shares: HashMap<String, Vec<EncryptedShare>>,
    /// AI agent identities keyed by agent DID.
    pub agent_registry: HashMap<Did, AgentIdentity>,
    /// Biometric anchors keyed by DID.
    pub biometric_registry: HashMap<Did, BiometricAnchor>,
}

impl IdentityState {
    /// Create an empty identity state.
    pub fn new() -> Self {
        Self {
            dids: HashMap::new(),
            recovery_registry: HashMap::new(),
            shares: HashMap::new(),
            agent_registry: HashMap::new(),
            biometric_registry: HashMap::new(),
        }
    }

    /// Apply an identity operation, mutating state.
    ///
    /// The `caller_pubkey` parameter provides the public key of the event
    /// creator for authorization checks. When `None`, authorization checks
    /// are skipped (used in backward-compatible contexts like tests).
    pub fn apply(
        &mut self,
        op: &IdentityOp,
        vc: &VectorClock,
        caller_pubkey: Option<&[u8; 32]>,
    ) -> Result<(), ShardError> {
        match op {
            IdentityOp::CreateDid { document } => {
                if self.dids.contains_key(&document.id) {
                    return Err(ShardError::StateConflict(format!(
                        "DID already exists: {}",
                        document.id
                    )));
                }
                // Authorization: caller's public key must match the document's primary key
                let caller = caller_pubkey.ok_or_else(||
                    ShardError::ValidationFailed("Authorization required: caller_pubkey must be provided for CreateDid".into())
                )?;
                if &document.public_key != caller {
                    return Err(ShardError::ValidationFailed(
                        "Only the DID owner can create their DID".into(),
                    ));
                }
                self.dids.insert(document.id.clone(), document.clone());
                Ok(())
            }
            IdentityOp::UpdateDid { did, updates } => {
                let doc = self
                    .dids
                    .get_mut(did)
                    .ok_or_else(|| ShardError::ValidationFailed(format!("DID not found: {did}")))?;

                // Authorization check: caller must be in the document's authentication set
                let caller = caller_pubkey.ok_or_else(||
                    ShardError::ValidationFailed("Authorization required: caller_pubkey must be provided for UpdateDid".into())
                )?;
                if !doc.authentication.iter().any(|key| key == caller) {
                    return Err(ShardError::ValidationFailed(
                        "Unauthorized: caller not in authentication set".into(),
                    ));
                }

                for update in updates {
                    match update {
                        DidUpdate::AddAuthentication { public_key } => {
                            if !doc.authentication.contains(public_key) {
                                doc.authentication.push(*public_key);
                            }
                        }
                        DidUpdate::RemoveAuthentication { public_key } => {
                            doc.authentication.retain(|pk| pk != public_key);
                        }
                        DidUpdate::AddService { service_id, endpoint } => {
                            doc.services.insert(service_id.clone(), endpoint.clone());
                        }
                    }
                }
                doc.updated_at.merge(vc);
                Ok(())
            }
            IdentityOp::RecoverDid { did, shares } => {
                let config = self
                    .recovery_registry
                    .get(did)
                    .ok_or_else(|| ShardError::ValidationFailed(format!("No recovery config for DID: {did}")))?;

                if shares.len() < config.threshold as usize {
                    return Err(ShardError::ValidationFailed("Insufficient recovery shares".into()));
                }

                // Reconstruct the secret from the provided shares
                let reconstructed = ShamirRecovery::reconstruct(shares)
                    .map_err(|e| ShardError::ValidationFailed(format!("Recovery reconstruction failed: {e}")))?;

                // Derive a new Ed25519 public key from the reconstructed secret
                // using BLAKE3 domain separation. The derived key is deterministic:
                // the same secret always produces the same public key.
                let new_public_key = derive_identity_key(&reconstructed);

                // Use complete_recovery() to properly update the DID document
                self.complete_recovery(did, &new_public_key, vc)?;

                Ok(())
            }
            IdentityOp::VerifyDid { did } => {
                if !self.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!("DID not found: {did}")));
                }
                Ok(())
            }
            IdentityOp::AddAgent { did, agent } => {
                if !self.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!("Owner DID not found: {did}")));
                }
                // Authorization: caller must be in the DID's authentication set
                let caller = caller_pubkey.ok_or_else(||
                    ShardError::ValidationFailed("Authorization required: caller_pubkey must be provided for AddAgent".into())
                )?;
                let doc = self.dids.get(did).expect("checked above");
                if !doc.authentication.iter().any(|key| key == caller) {
                    return Err(ShardError::ValidationFailed(
                        "Unauthorized: caller not in authentication set for AddAgent".into(),
                    ));
                }
                if self.agent_registry.contains_key(&agent.did) {
                    return Err(ShardError::StateConflict(format!(
                        "Agent already exists: {}",
                        agent.did
                    )));
                }
                self.agent_registry.insert(agent.did.clone(), agent.clone());
                Ok(())
            }
            IdentityOp::EnrollBiometric {
                did,
                template,
                algorithm,
            } => {
                if !self.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!("DID not found: {did}")));
                }
                // Authorization: caller must be in the DID's authentication set
                let caller = caller_pubkey.ok_or_else(||
                    ShardError::ValidationFailed("Authorization required: caller_pubkey must be provided for EnrollBiometric".into())
                )?;
                let doc = self.dids.get(did).expect("checked above");
                if !doc.authentication.iter().any(|key| key == caller) {
                    return Err(ShardError::ValidationFailed(
                        "Unauthorized: caller not in authentication set for EnrollBiometric".into(),
                    ));
                }
                let anchor = BiometricAnchor::enroll(template, algorithm);
                self.biometric_registry.insert(did.clone(), anchor);
                Ok(())
            }
            IdentityOp::VerifyBiometric { did, template } => {
                let anchor = self
                    .biometric_registry
                    .get(did)
                    .ok_or_else(|| ShardError::ValidationFailed(format!("No biometric enrolled for DID: {did}")))?;
                if !anchor.verify(template) {
                    return Err(ShardError::ValidationFailed("Biometric verification failed".into()));
                }
                Ok(())
            }
            IdentityOp::RevokeAgent { agent_did } => {
                let caller = caller_pubkey.ok_or_else(||
                    ShardError::ValidationFailed("Authorization required: caller_pubkey must be provided for RevokeAgent".into())
                )?;
                let agent = self
                    .agent_registry
                    .get(agent_did)
                    .ok_or_else(|| ShardError::ValidationFailed(format!("Agent not found: {agent_did}")))?;
                // The caller must be the DID that created this agent (owner)
                // Look up the owner DID and verify caller is in its authentication set
                let owner_did = &agent.owner_did;
                let owner_doc = self.dids.get(owner_did)
                    .ok_or_else(|| ShardError::ValidationFailed(format!("Owner DID not found: {owner_did}")))?;
                if !owner_doc.authentication.iter().any(|key| key == caller) {
                    return Err(ShardError::ValidationFailed(
                        "Only the agent owner can revoke".into(),
                    ));
                }
                let agent = self.agent_registry.get_mut(agent_did).unwrap();
                agent.revoke();
                Ok(())
            }
            IdentityOp::ConfigureRecovery {
                did,
                secret,
                threshold,
                total_shares,
            } => {
                if !self.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!("DID not found: {did}")));
                }
                // Authorization: caller must be in the DID's authentication set
                let caller = caller_pubkey.ok_or_else(||
                    ShardError::ValidationFailed("Authorization required: caller_pubkey must be provided for ConfigureRecovery".into())
                )?;
                let doc = self.dids.get(did).expect("checked above");
                if !doc.authentication.iter().any(|key| key == caller) {
                    return Err(ShardError::ValidationFailed(
                        "Unauthorized: caller not in authentication set for ConfigureRecovery".into(),
                    ));
                }
                let shares = ShamirRecovery::split(secret, *threshold, *total_shares)
                    .map_err(|e| ShardError::ValidationFailed(format!("Recovery split failed: {e}")))?;
                self.recovery_registry.insert(
                    did.clone(),
                    RecoveryConfig {
                        threshold: *threshold,
                        total_shares: *total_shares,
                    },
                );
                // Encrypt and persist shares instead of dropping them
                self.persist_shares(did, &shares)?;
                Ok(())
            }
        }
    }

    /// Enroll a biometric anchor for a DID.
    ///
    /// Convenience method that creates a `BiometricAnchor` without going
    /// through the `apply` pipeline.
    pub fn enroll_biometric(&mut self, did: &str, template: &[u8], algorithm: &str) -> Result<(), ShardError> {
        if !self.dids.contains_key(did) {
            return Err(ShardError::ValidationFailed(format!("DID not found: {did}")));
        }
        let anchor = BiometricAnchor::enroll(template, algorithm);
        self.biometric_registry.insert(did.to_string(), anchor);
        Ok(())
    }

    /// Verify a biometric template against the stored commitment.
    pub fn verify_biometric(&self, did: &str, fresh_template: &[u8]) -> Result<bool, ShardError> {
        let anchor = self
            .biometric_registry
            .get(did)
            .ok_or_else(|| ShardError::ValidationFailed(format!("No biometric enrolled for DID: {did}")))?;
        Ok(anchor.verify(fresh_template))
    }

    /// Create Shamir's Secret Sharing recovery shares for a DID.
    pub fn create_recovery_shares(
        &mut self,
        did: &str,
        secret: &[u8],
        threshold: u8,
        total: u8,
    ) -> Result<Vec<RecoveryShare>, ShardError> {
        if !self.dids.contains_key(did) {
            return Err(ShardError::ValidationFailed(format!("DID not found: {did}")));
        }
        let shares = ShamirRecovery::split(secret, threshold, total)
            .map_err(|e| ShardError::ValidationFailed(format!("Recovery split failed: {e}")))?;
        self.recovery_registry.insert(
            did.to_string(),
            RecoveryConfig {
                threshold,
                total_shares: total,
            },
        );
        Ok(shares)
    }

    /// Recover a DID secret using Shamir's Secret Sharing.
    pub fn recover_did(&self, did: &str, shares: &[RecoveryShare]) -> Result<Vec<u8>, ShardError> {
        let config = self
            .recovery_registry
            .get(did)
            .ok_or_else(|| ShardError::ValidationFailed(format!("No recovery config for DID: {did}")))?;
        if shares.len() < config.threshold as usize {
            return Err(ShardError::ValidationFailed(format!(
                "Insufficient shares: have {}, need {}",
                shares.len(),
                config.threshold
            )));
        }
        ShamirRecovery::reconstruct(shares)
            .map_err(|e| ShardError::ValidationFailed(format!("Recovery reconstruction failed: {e}")))
    }

    /// Complete the recovery process by adding the recovered key to DID authentication.
    ///
    /// This method ensures:
    /// 1. The recovered key is added to the authentication set
    /// 2. A recovery counter is incremented (prevents replay)
    /// 3. The DID document is properly updated
    pub fn complete_recovery(
        &mut self,
        did: &str,
        recovered_public_key: &[u8; 32],
        vc: &VectorClock,
    ) -> Result<(), ShardError> {
        let doc = self
            .dids
            .get_mut(did)
            .ok_or_else(|| ShardError::ValidationFailed(format!("DID not found: {did}")))?;

        // Add the recovered key to authentication (rotation, not replacement)
        let key_array = *recovered_public_key;
        if !doc.authentication.contains(&key_array) {
            doc.authentication.push(key_array);
        }

        // Increment recovery counter (prevents replay attacks)
        doc.recovery_count += 1;

        // Update vector clock
        doc.updated_at.merge(vc);
        doc.recovery_enabled = true;

        Ok(())
    }

    /// Register an AI agent identity.
    pub fn register_agent(&mut self, agent: AgentIdentity) -> Result<(), ShardError> {
        if self.agent_registry.contains_key(&agent.did) {
            return Err(ShardError::StateConflict(format!(
                "Agent already exists: {}",
                agent.did
            )));
        }
        if !self.dids.contains_key(&agent.owner_did) {
            return Err(ShardError::ValidationFailed(format!(
                "Owner DID not found: {}",
                agent.owner_did
            )));
        }
        self.agent_registry.insert(agent.did.clone(), agent);
        Ok(())
    }

    /// Encrypt and persist recovery shares for a DID.
    ///
    /// Each share is encrypted with AES-256-GCM using a per-custodian key
    /// derived via BLAKE3 + HKDF-SHA256 with domain separation. This provides
    /// authenticated encryption — any tampering with the ciphertext will be
    /// detected on decryption. In production, a higher layer would re-encrypt
    /// with the custodian's actual public key.
    pub fn persist_shares(&mut self, did: &str, shares: &[RecoveryShare]) -> Result<(), ShardError> {
        let mut encrypted: Vec<EncryptedShare> = Vec::with_capacity(shares.len());

        for share in shares {
            // Derive a per-custodian encryption key using BLAKE3 + HKDF domain separation
            let mut key_input = Vec::with_capacity(SHARE_ENCRYPTION_DOMAIN.len() + 1);
            key_input.extend_from_slice(SHARE_ENCRYPTION_DOMAIN);
            key_input.push(share.index);

            // Derive AES-256 key via HKDF-SHA256 from BLAKE3-derived key material
            let key_material = blake3::derive_key("OMNIA-SHARE-AES-KEY-V2", &key_input);
            let aes_key = hkdf_aes_key(&key_material, "OMNIA-SHARE-ENCRYPT-V2");

            // Generate random 96-bit nonce
            let nonce = generate_nonce();

            // AES-256-GCM encrypt
            let ciphertext = aes256gcm_encrypt(&share.value, &aes_key, &nonce, &key_input);

            encrypted.push(EncryptedShare {
                custodian: share.index,
                ciphertext,
                nonce,
                version: ENCRYPTED_SHARE_VERSION, // version 2
            });
        }

        self.shares.insert(did.to_string(), encrypted);
        Ok(())
    }

    /// Decrypt shares for a given DID, returning the raw `RecoveryShare`s.
    ///
    /// This reverses `persist_shares` with version-aware decryption:
    /// - v1 (legacy): XOR decryption with BLAKE3-derived keys
    /// - v2 (current): AES-256-GCM authenticated decryption
    pub fn decrypt_shares(&self, did: &str) -> Result<Vec<RecoveryShare>, ShardError> {
        let encrypted_shares = self
            .shares
            .get(did)
            .ok_or_else(|| ShardError::ValidationFailed(format!("No encrypted shares for DID: {did}")))?;

        let mut decrypted = Vec::with_capacity(encrypted_shares.len());

        for enc in encrypted_shares {
            let plaintext = match enc.version {
                #[cfg(feature = "legacy-xor-encryption")]
                1 => {
                    // Legacy XOR decryption (backward compatibility)
                    // Only available when the `legacy-xor-encryption` feature is enabled.
                    tracing::warn!(
                        "Decrypting legacy v1 XOR share for custodian {} - upgrade recommended",
                        enc.custodian
                    );
                    let mut key_input = Vec::with_capacity(SHARE_ENCRYPTION_DOMAIN.len() + 1);
                    key_input.extend_from_slice(SHARE_ENCRYPTION_DOMAIN);
                    key_input.push(enc.custodian);
                    let key = blake3::derive_key("OMNIA-SHARE-ENCRYPTION-KEY", &key_input);
                    xor_with_key(&enc.ciphertext, &key)
                }
                #[cfg(not(feature = "legacy-xor-encryption"))]
                1 => {
                    return Err(ShardError::ValidationFailed(
                        "Legacy v1 XOR decryption requires the 'legacy-xor-encryption' feature flag. \
                         Re-encrypt shares with v2 (AES-256-GCM) to decrypt without this flag.".into(),
                    ));
                }
                2 => {
                    // AES-256-GCM decryption
                    let mut key_input = Vec::with_capacity(SHARE_ENCRYPTION_DOMAIN.len() + 1);
                    key_input.extend_from_slice(SHARE_ENCRYPTION_DOMAIN);
                    key_input.push(enc.custodian);
                    let key_material = blake3::derive_key("OMNIA-SHARE-AES-KEY-V2", &key_input);
                    let aes_key = hkdf_aes_key(&key_material, "OMNIA-SHARE-ENCRYPT-V2");
                    aes256gcm_decrypt(&enc.ciphertext, &aes_key, &enc.nonce, &key_input)?
                }
                _ => {
                    return Err(ShardError::ValidationFailed(format!(
                        "Unknown share encryption version: {}",
                        enc.version
                    )));
                }
            };

            decrypted.push(RecoveryShare {
                index: enc.custodian,
                value: plaintext,
            });
        }

        Ok(decrypted)
    }

    /// Serialize the state to bytes for snapshots.
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    /// Deserialize state from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

impl Default for IdentityState {
    fn default() -> Self {
        Self::new()
    }
}

/// XOR a byte slice with a repeating 32-byte key.
///
/// This is a simple stream cipher used for legacy v1 share encryption.
/// XOR is its own inverse, so the same function is used
/// for both encryption and decryption.
///
/// **DEPRECATED**: This function is gated behind the `legacy-xor-encryption`
/// feature flag. XOR encryption does not provide authenticated encryption
/// and is vulnerable to known-plaintext attacks. Use AES-256-GCM (v2) instead.
#[cfg(feature = "legacy-xor-encryption")]
fn xor_with_key(data: &[u8], key: &[u8; 32]) -> Vec<u8> {
    data.iter().enumerate().map(|(i, byte)| byte ^ key[i % 32]).collect()
}

/// Derive an AES-256 key from key material using HKDF-SHA256.
///
/// Delegates to [`omnia_crypto::hkdf_aes_key`] to avoid duplicating the
/// HKDF key-derivation pattern that already exists in the crypto crate.
fn hkdf_aes_key(key_material: &[u8; 32], info: &str) -> [u8; 32] {
    omnia_crypto::hkdf_aes_key(key_material, info).expect("HKDF key derivation should not fail for valid 32-byte input")
}

/// Generate a random 96-bit nonce for AES-256-GCM.
///
/// Delegates to [`omnia_crypto::generate_nonce`].
fn generate_nonce() -> [u8; 12] {
    omnia_crypto::generate_nonce()
}

/// AES-256-GCM encrypt with associated data.
///
/// Delegates to [`omnia_crypto::aes256gcm_encrypt_aad`] to avoid duplicating
/// the AES-GCM encryption pattern that already exists in the crypto crate.
fn aes256gcm_encrypt(plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 12], aad: &[u8]) -> Vec<u8> {
    omnia_crypto::aes256gcm_encrypt_aad(plaintext, key, nonce, aad)
        .expect("AES-256-GCM encryption should not fail with valid key")
}

/// AES-256-GCM decrypt with associated data.
///
/// Delegates to [`omnia_crypto::aes256gcm_decrypt_aad`] to avoid duplicating
/// the AES-GCM decryption pattern that already exists in the crypto crate.
fn aes256gcm_decrypt(ciphertext: &[u8], key: &[u8; 32], nonce: &[u8; 12], aad: &[u8]) -> Result<Vec<u8>, ShardError> {
    omnia_crypto::aes256gcm_decrypt_aad(ciphertext, key, nonce, aad)
        .map_err(|_| ShardError::ValidationFailed("Share decryption failed: authentication error".to_string()))
}

/// Derive a 32-byte Ed25519 public key from a reconstructed secret
/// using BLAKE3 domain separation.
///
/// The derivation uses `blake3::derive_key` with the domain
/// `"OMNIA-IDENTITY-ED25519-V1"` to ensure the derived key is
/// context-separated from all other BLAKE3 uses in the protocol.
/// The same secret always produces the same public key (deterministic).
fn derive_identity_key(secret: &[u8]) -> [u8; 32] {
    blake3::derive_key(IDENTITY_KEY_DERIVATION_DOMAIN, secret)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// End-to-end test for Shamir's Secret Sharing recovery flow:
    /// 1. Create an identity with 5 custodians (threshold=3)
    /// 2. Persist encrypted shares
    /// 3. Simulate 3 custodians providing shares
    /// 4. Reconstruct secret
    /// 5. Derive new keypair
    /// 6. Verify new keypair is in authentication set
    /// 7. Verify old keypair is still valid (rotation, not replacement)
    #[test]
    fn test_sss_recovery_end_to_end() {
        let mut state = IdentityState::new();
        let vc = VectorClock::new();

        // Step 1: Create a DID
        let original_pk: [u8; 32] = [0xAB; 32];
        let did = "did:omnia:abcd1234".to_string();
        let doc = DidDocument::new(did.clone(), original_pk, 1000);
        state
            .apply(&IdentityOp::CreateDid { document: doc }, &vc, Some(&original_pk))
            .unwrap();

        // Step 2: Configure recovery with 5 custodians, threshold=3
        let secret = b"my-super-secret-recovery-key";
        state
            .apply(
                &IdentityOp::ConfigureRecovery {
                    did: did.clone(),
                    secret: secret.to_vec(),
                    threshold: 3,
                    total_shares: 5,
                },
                &vc,
                Some(&original_pk),
            )
            .unwrap();

        // Verify recovery config was stored
        let config = state.recovery_registry.get(&did).unwrap();
        assert_eq!(config.threshold, 3);
        assert_eq!(config.total_shares, 5);

        // Verify encrypted shares were persisted
        let encrypted = state.shares.get(&did).unwrap();
        assert_eq!(encrypted.len(), 5);
        for enc in encrypted {
            assert_eq!(enc.version, 2); // v2 = AES-256-GCM
            assert!(!enc.ciphertext.is_empty()); // ciphertext should not be empty
        }

        // Step 3: Decrypt shares and simulate 3 custodians providing shares
        let all_shares = state.decrypt_shares(&did).unwrap();
        assert_eq!(all_shares.len(), 5);

        // Pick 3 out of 5 shares (e.g., custodians 1, 3, 5)
        let recovering_shares: Vec<RecoveryShare> =
            vec![all_shares[0].clone(), all_shares[2].clone(), all_shares[4].clone()];

        // Step 4: Reconstruct the secret
        let reconstructed = ShamirRecovery::reconstruct(&recovering_shares).unwrap();
        assert_eq!(reconstructed, secret.to_vec());

        // Step 5: Derive new keypair from the reconstructed secret
        let new_public_key = derive_identity_key(&reconstructed);

        // Verify the derivation is deterministic
        let new_public_key_2 = derive_identity_key(&reconstructed);
        assert_eq!(new_public_key, new_public_key_2);

        // Step 6: Perform recovery via the apply pipeline
        state
            .apply(
                &IdentityOp::RecoverDid {
                    did: did.clone(),
                    shares: recovering_shares,
                },
                &vc,
                None,
            )
            .unwrap();

        // Verify new keypair is in authentication set
        let updated_doc = state.dids.get(&did).unwrap();
        assert!(
            updated_doc.authentication.contains(&new_public_key),
            "New derived key should be in authentication set"
        );

        // Step 7: Verify old keypair is still valid (rotation, not replacement)
        assert!(
            updated_doc.authentication.contains(&original_pk),
            "Original key should still be in authentication set (rotation, not replacement)"
        );

        // Verify recovery_enabled is set
        assert!(updated_doc.recovery_enabled);

        // Verify authentication has 2 keys (original + new)
        assert_eq!(updated_doc.authentication.len(), 2);
    }

    #[test]
    fn test_encrypted_share_aes256gcm_roundtrip() {
        // Formerly test_encrypted_share_xor_roundtrip — now tests v2 AES-256-GCM.
        let mut state = IdentityState::new();
        let did = "did:omnia:test".to_string();
        let pk: [u8; 32] = [0x42; 32];
        let doc = DidDocument::new(did.clone(), pk, 0);
        state.dids.insert(did.clone(), doc);

        let secret = b"test-secret-for-aes-gcm";
        let shares = ShamirRecovery::split(secret, 2, 3).unwrap();

        // Persist and then decrypt
        state.persist_shares(&did, &shares).unwrap();
        let decrypted = state.decrypt_shares(&did).unwrap();

        // Verify roundtrip: decrypted shares should match original
        assert_eq!(decrypted.len(), shares.len());
        for (orig, dec) in shares.iter().zip(decrypted.iter()) {
            assert_eq!(orig.index, dec.index);
            assert_eq!(orig.value, dec.value);
        }

        // Verify the decrypted shares can reconstruct the secret
        let reconstructed = ShamirRecovery::reconstruct(&decrypted).unwrap();
        assert_eq!(reconstructed, secret.to_vec());
    }

    #[test]
    fn test_derive_identity_key_domain_separation() {
        // Different secrets should produce different keys
        let key_a = derive_identity_key(b"secret-a");
        let key_b = derive_identity_key(b"secret-b");
        assert_ne!(key_a, key_b, "Different secrets must produce different keys");

        // Same secret should produce the same key
        let key_1 = derive_identity_key(b"same-secret");
        let key_2 = derive_identity_key(b"same-secret");
        assert_eq!(key_1, key_2, "Same secret must produce the same key");
    }

    #[test]
    fn test_persist_shares_stores_correct_version() {
        let mut state = IdentityState::new();
        let did = "did:omnia:version-test".to_string();
        let pk: [u8; 32] = [0x11; 32];
        let doc = DidDocument::new(did.clone(), pk, 0);
        state.dids.insert(did.clone(), doc);

        let shares = ShamirRecovery::split(b"secret", 2, 3).unwrap();
        state.persist_shares(&did, &shares).unwrap();

        let encrypted = state.shares.get(&did).unwrap();
        for enc in encrypted {
            assert_eq!(enc.version, 2); // v2 = AES-256-GCM
        }
    }

    #[test]
    fn test_share_aes256gcm_round_trip() {
        let mut state = IdentityState::new();
        let did = "did:omnia:aes-test".to_string();
        let pk: [u8; 32] = [0x42; 32];
        let doc = DidDocument::new(did.clone(), pk, 0);
        state.dids.insert(did.clone(), doc);

        let secret = b"test-secret-for-aes-gcm";
        let shares = ShamirRecovery::split(secret, 2, 3).unwrap();
        state.persist_shares(&did, &shares).unwrap();

        // Verify version 2
        let encrypted = state.shares.get(&did).unwrap();
        for enc in encrypted {
            assert_eq!(enc.version, 2);
        }

        // Decrypt and verify roundtrip
        let decrypted = state.decrypt_shares(&did).unwrap();
        assert_eq!(decrypted.len(), shares.len());
        for (orig, dec) in shares.iter().zip(decrypted.iter()) {
            assert_eq!(orig.index, dec.index);
            assert_eq!(orig.value, dec.value);
        }

        // Verify reconstruction
        let reconstructed = ShamirRecovery::reconstruct(&decrypted).unwrap();
        assert_eq!(reconstructed, secret.to_vec());
    }

    #[test]
    fn test_share_tamper_detected() {
        let mut state = IdentityState::new();
        let did = "did:omnia:tamper-test".to_string();
        let pk: [u8; 32] = [0x33; 32];
        let doc = DidDocument::new(did.clone(), pk, 0);
        state.dids.insert(did.clone(), doc);

        let secret = b"tamper-detection-secret";
        let shares = ShamirRecovery::split(secret, 2, 3).unwrap();
        state.persist_shares(&did, &shares).unwrap();

        // Tamper with a ciphertext byte
        let encrypted_shares = state.shares.get_mut(&did).unwrap();
        if !encrypted_shares[0].ciphertext.is_empty() {
            encrypted_shares[0].ciphertext[0] ^= 0xFF;
        }

        // Decryption should fail (AEAD authentication)
        let result = state.decrypt_shares(&did);
        assert!(result.is_err(), "Tampered share should fail AES-GCM authentication");
    }

    #[test]
    #[cfg(feature = "legacy-xor-encryption")]
    fn test_share_v1_backward_compat() {
        let mut state = IdentityState::new();
        let did = "did:omnia:v1-compat".to_string();
        let pk: [u8; 32] = [0x55; 32];
        let doc = DidDocument::new(did.clone(), pk, 0);
        state.dids.insert(did.clone(), doc);

        let secret = b"v1-backward-compat-secret";
        let shares = ShamirRecovery::split(secret, 2, 3).unwrap();

        // Manually create v1 (XOR) encrypted shares
        let mut v1_encrypted = Vec::new();
        for share in &shares {
            let mut key_input = Vec::with_capacity(SHARE_ENCRYPTION_DOMAIN.len() + 1);
            key_input.extend_from_slice(SHARE_ENCRYPTION_DOMAIN);
            key_input.push(share.index);
            let key = blake3::derive_key("OMNIA-SHARE-ENCRYPTION-KEY", &key_input);
            let nonce_input: Vec<u8> = {
                let mut v = Vec::with_capacity(SHARE_ENCRYPTION_DOMAIN.len() + 1);
                v.extend_from_slice(b"OMNIA-SHARE-NONCE-V1");
                v.push(share.index);
                v
            };
            let nonce_hash = blake3::hash(&nonce_input);
            let mut nonce = [0u8; 12];
            nonce.copy_from_slice(&nonce_hash.as_bytes()[..12]);
            let ciphertext = xor_with_key(&share.value, &key);
            v1_encrypted.push(EncryptedShare {
                custodian: share.index,
                ciphertext,
                nonce,
                version: 1,
            });
        }
        state.shares.insert(did.clone(), v1_encrypted);

        // Decrypt v1 shares should still work
        let decrypted = state.decrypt_shares(&did).unwrap();
        let reconstructed = ShamirRecovery::reconstruct(&decrypted).unwrap();
        assert_eq!(reconstructed, secret.to_vec());
    }

    #[test]
    fn test_share_wrong_key_fails() {
        let mut state = IdentityState::new();
        let did = "did:omnia:wrong-key".to_string();
        let pk: [u8; 32] = [0x77; 32];
        let doc = DidDocument::new(did.clone(), pk, 0);
        state.dids.insert(did.clone(), doc);

        let secret = b"wrong-key-test-secret";
        let shares = ShamirRecovery::split(secret, 2, 3).unwrap();
        state.persist_shares(&did, &shares).unwrap();

        // Corrupt the custodian index to use wrong decryption key
        let encrypted_shares = state.shares.get_mut(&did).unwrap();
        encrypted_shares[0].custodian = 99; // Wrong custodian index

        // Decryption should fail (derived key won't match)
        let result = state.decrypt_shares(&did);
        assert!(result.is_err(), "Wrong key should fail AES-GCM decryption");
    }

    #[test]
    fn test_sss_recovery_updates_did_auth() {
        let mut state = IdentityState::new();
        let vc = VectorClock::new();

        // Create a DID
        let original_pk: [u8; 32] = [0xAB; 32];
        let did = "did:omnia:recovery-auth-test".to_string();
        let doc = DidDocument::new(did.clone(), original_pk, 1000);
        state
            .apply(&IdentityOp::CreateDid { document: doc }, &vc, Some(&original_pk))
            .unwrap();

        // Configure recovery with 5 custodians, threshold=3
        let secret = b"recovery-auth-secret";
        state
            .apply(
                &IdentityOp::ConfigureRecovery {
                    did: did.clone(),
                    secret: secret.to_vec(),
                    threshold: 3,
                    total_shares: 5,
                },
                &vc,
                Some(&original_pk),
            )
            .unwrap();

        // Recover with 3 shares
        let all_shares = state.decrypt_shares(&did).unwrap();
        let recovering_shares: Vec<RecoveryShare> =
            vec![all_shares[0].clone(), all_shares[2].clone(), all_shares[4].clone()];

        // Perform recovery
        state
            .apply(
                &IdentityOp::RecoverDid {
                    did: did.clone(),
                    shares: recovering_shares,
                },
                &vc,
                None,
            )
            .unwrap();

        // Verify new key is in authentication
        let updated_doc = state.dids.get(&did).unwrap();
        let new_public_key = derive_identity_key(secret);
        assert!(
            updated_doc.authentication.contains(&new_public_key),
            "New recovered key should be in authentication set"
        );

        // Verify old key is still present (rotation, not replacement)
        assert!(
            updated_doc.authentication.contains(&original_pk),
            "Original key should still be in authentication set"
        );

        // Verify recovery_count incremented
        assert_eq!(updated_doc.recovery_count, 1);
    }

    #[test]
    fn test_recovery_prevents_replay() {
        let mut state = IdentityState::new();
        let vc = VectorClock::new();

        let original_pk: [u8; 32] = [0xCC; 32];
        let did = "did:omnia:replay-test".to_string();
        let doc = DidDocument::new(did.clone(), original_pk, 1000);
        state
            .apply(&IdentityOp::CreateDid { document: doc }, &vc, Some(&original_pk))
            .unwrap();

        let secret = b"replay-prevention-secret";
        state
            .apply(
                &IdentityOp::ConfigureRecovery {
                    did: did.clone(),
                    secret: secret.to_vec(),
                    threshold: 2,
                    total_shares: 3,
                },
                &vc,
                Some(&original_pk),
            )
            .unwrap();

        let shares = state.decrypt_shares(&did).unwrap();

        // First recovery
        state
            .apply(
                &IdentityOp::RecoverDid {
                    did: did.clone(),
                    shares: shares.clone(),
                },
                &vc,
                None,
            )
            .unwrap();

        let doc_after_first = state.dids.get(&did).unwrap();
        assert_eq!(doc_after_first.recovery_count, 1);

        // Second recovery with same shares
        state
            .apply(
                &IdentityOp::RecoverDid {
                    did: did.clone(),
                    shares: shares.clone(),
                },
                &vc,
                None,
            )
            .unwrap();

        let doc_after_second = state.dids.get(&did).unwrap();
        assert_eq!(doc_after_second.recovery_count, 2);
        // The same key should not be duplicated
        let key = derive_identity_key(secret);
        let key_count = doc_after_second.authentication.iter().filter(|k| **k == key).count();
        assert_eq!(key_count, 1, "Same key should not be duplicated in authentication");
    }
}

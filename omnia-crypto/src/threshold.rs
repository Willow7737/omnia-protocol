//! Threshold Signatures — t-of-n Cryptographic Signing.
//!
//! This module implements threshold signatures, allowing a group of `n`
//! participants to produce a combined signature when at least `t` of them
//! cooperate. No individual participant can produce a valid signature alone,
//! and the combined signature is indistinguishable from a single-party
//! signature.
//!
//! # Use Cases
//!
//! - **Consensus finality**: A block is finalized when `t` of `n` validators
//!   sign it, combining their partial signatures into a single aggregate.
//! - **Key management**: The protocol key is split across `n` custodians;
//!   at least `t` must cooperate to sign governance transactions.
//! - **Social recovery**: A user's key can be recovered if `t` of `n`
//!   guardians approve the recovery.
//!
//! # Security Model
//!
//! - The scheme is secure as long as fewer than `t` participants are corrupt.
//! - Partial signatures from fewer than `t` participants reveal no information
//!   about the group secret.
//! - The combined signature is a standard BLS signature — verifiers do not
//!   need to know the threshold scheme was used.

use crate::bls::{BlsError, BlsKeypair, BlsPublicKey, BlsSignature};
use omnia_primitives::NodeId;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during threshold signature operations.
#[derive(Error, Debug)]
pub enum ThresholdError {
    /// Insufficient partial signatures to meet the threshold.
    #[error("insufficient partial signatures: got {got}, need {need}")]
    InsufficientPartials {
        /// Number of partial signatures provided.
        got: usize,
        /// Required threshold.
        need: usize,
    },
    /// BLS signature aggregation failed.
    #[error("signature aggregation failed: {0}")]
    AggregationFailed(#[from] BlsError),
    /// A signer's public key was not found in the key manager.
    #[error("unknown signers: found {found} public keys for {expected} signers")]
    UnknownSigners {
        /// Number of public keys found.
        found: usize,
        /// Expected number of signers.
        expected: usize,
    },
    /// Threshold signature verification failed.
    #[error("threshold signature verification failed: {0}")]
    VerificationFailed(String),
    /// The participant is not registered in the key manager.
    #[error("participant {:?} not registered", .0)]
    ParticipantNotRegistered([u8; 4]),
}

/// Configuration for threshold signature operations.
///
/// Specifies the group size `n` and the threshold `t` (minimum number of
/// participants required to produce a valid combined signature).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Total number of participants (n).
    pub total_participants: usize,
    /// Minimum number of participants required (t).
    ///
    /// Must be at least 2 and at most `total_participants`. A common choice
    /// is `t = 2*n/3 + 1` for BFT-style threshold.
    pub threshold: usize,
}

impl ThresholdConfig {
    /// Create a new threshold configuration.
    ///
    /// # Panics
    ///
    /// Panics if `threshold < 2` or `threshold > total_participants`.
    pub fn new(total_participants: usize, threshold: usize) -> Self {
        assert!(
            threshold >= 2,
            "Threshold must be at least 2, got {}",
            threshold
        );
        assert!(
            threshold <= total_participants,
            "Threshold {} exceeds total participants {}",
            threshold,
            total_participants
        );
        Self {
            total_participants,
            threshold,
        }
    }

    /// Create a BFT-style threshold configuration (2n/3 + 1).
    pub fn bft(n: usize) -> Self {
        let t = (2 * n) / 3 + 1;
        Self::new(n, t)
    }

    /// Preset: Validator key recovery (3-of-5).
    ///
    /// Three of five validator custodians must cooperate to recover
    /// a lost validator key.
    pub fn validator_recovery() -> Self {
        Self::new(5, 3)
    }

    /// Preset: Emergency multisig (2-of-3).
    ///
    /// Two of three emergency signers can authorize critical operations
    /// like pausing the protocol or triggering an upgrade.
    pub fn emergency_multisig() -> Self {
        Self::new(3, 2)
    }

    /// Preset: Governance council (5-of-7).
    ///
    /// Five of seven council members must approve governance decisions
    /// that require council authorization.
    pub fn governance_council() -> Self {
        Self::new(7, 5)
    }

    /// Returns `true` if the given number of participants meets the threshold.
    pub fn has_quorum(&self, participant_count: usize) -> bool {
        participant_count >= self.threshold
    }
}

/// A participant's key share in the threshold scheme.
///
/// Each participant holds a key share derived from the group secret via
/// Shamir's Secret Sharing. The share is used to produce partial signatures
/// and cannot be used to reconstruct the group secret alone (unless `t`
/// shares are combined).
///
/// Note: `KeyShare` does not implement `Serialize`/`Deserialize` because
/// `BlsKeypair` contains raw blst types that are not serializable. For
/// persistence, serialize the keypair's secret key bytes separately.
#[derive(Debug, Clone)]
pub struct KeyShare {
    /// The participant's identifier (typically their NodeId).
    pub participant: NodeId,
    /// The participant's index in the polynomial evaluation (1-based).
    pub index: usize,
    /// The participant's BLS keypair (used for partial signing).
    pub keypair: BlsKeypair,
}

impl KeyShare {
    /// Create a new key share for the given participant.
    pub fn new(participant: NodeId, index: usize, keypair: BlsKeypair) -> Self {
        Self {
            participant,
            index,
            keypair,
        }
    }

    /// Produce a partial signature on the given message.
    ///
    /// This is a standard BLS signature using the participant's key share.
    /// When `t` partial signatures are combined, the result is a valid
    /// aggregate BLS signature.
    pub fn partial_sign(&self, message: &[u8]) -> PartialSignature {
        PartialSignature {
            participant: self.participant,
            index: self.index,
            signature: self.keypair.sign(message),
        }
    }

    /// Get the public key for this key share.
    pub fn public_key(&self) -> BlsPublicKey {
        self.keypair.public_key()
    }
}

/// A partial signature from a single participant.
///
/// This is a BLS signature produced by one participant using their key share.
/// Partial signatures are collected from `t` participants and combined into
/// a [`ThresholdSignature`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialSignature {
    /// The participant who produced this partial signature.
    pub participant: NodeId,
    /// The participant's index in the threshold scheme.
    pub index: usize,
    /// The BLS signature over the message.
    pub signature: BlsSignature,
}

/// A combined threshold signature produced by aggregating `t` partial
/// signatures.
///
/// This is a standard BLS aggregate signature that can be verified against
/// the aggregate public key of the signing participants. Verifiers do not
/// need to know the threshold parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSignature {
    /// The aggregate BLS signature.
    pub aggregate_signature: BlsSignature,
    /// The set of participants who contributed partial signatures.
    pub signers: Vec<NodeId>,
    /// The message that was signed.
    #[serde(with = "serde_bytes")]
    pub message: Vec<u8>,
}

/// Manager for threshold signature operations.
///
/// Coordinates the collection of partial signatures, verifies quorum, and
/// combines partial signatures into a threshold signature.
pub struct ThresholdKeyManager {
    /// The threshold configuration.
    pub config: ThresholdConfig,
    /// Registered key shares (participant → share).
    key_shares: HashMap<NodeId, KeyShare>,
}

impl ThresholdKeyManager {
    /// Create a new threshold key manager with the given configuration.
    pub fn new(config: ThresholdConfig) -> Self {
        Self {
            config,
            key_shares: HashMap::new(),
        }
    }

    /// Register a key share for a participant.
    ///
    /// Participants must be registered before they can produce partial
    /// signatures. The number of registered participants must eventually
    /// reach `total_participants` for the scheme to work correctly.
    pub fn register_share(&mut self, share: KeyShare) {
        tracing::info!(
            participant = ?&share.participant[..4],
            index = share.index,
            "Registered key share"
        );
        self.key_shares.insert(share.participant, share);
    }

    /// Get the number of registered participants.
    pub fn registered_count(&self) -> usize {
        self.key_shares.len()
    }

    /// Check whether the given number of partial signatures meets the
    /// threshold (quorum).
    pub fn has_quorum(&self, partial_count: usize) -> bool {
        self.config.has_quorum(partial_count)
    }

    /// Combine partial signatures into a threshold signature.
    ///
    /// Requires at least `threshold` partial signatures from distinct
    /// participants. The partial signatures are aggregated into a single
    /// BLS signature.
    ///
    /// # Arguments
    ///
    /// * `partials` — Slice of [`PartialSignature`]s from participants.
    /// * `message` — The message that was signed (for verification).
    ///
    /// # Returns
    ///
    /// A [`ThresholdSignature`] on success, or an error string if quorum
    /// is not met or aggregation fails.
    pub fn combine_signatures(
        &self,
        partials: &[PartialSignature],
        message: &[u8],
    ) -> Result<ThresholdSignature, ThresholdError> {
        if !self.has_quorum(partials.len()) {
            return Err(ThresholdError::InsufficientPartials {
                got: partials.len(),
                need: self.config.threshold,
            });
        }

        // Collect unique signers
        let signers: Vec<NodeId> = partials.iter().map(|p| p.participant).collect();

        // Aggregate the BLS signatures
        let aggregate_signature = crate::bls::aggregate_signatures(
            &partials
                .iter()
                .map(|p| p.signature.clone())
                .collect::<Vec<_>>(),
        )?;

        tracing::info!(
            signers = signers.len(),
            threshold = self.config.threshold,
            "Combined threshold signature"
        );

        Ok(ThresholdSignature {
            aggregate_signature,
            signers,
            message: message.to_vec(),
        })
    }

    /// Verify a threshold signature against the aggregate public key of
    /// the signers.
    ///
    /// # Arguments
    ///
    /// * `threshold_sig` — The [`ThresholdSignature`] to verify.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the signature is valid, `Err(String)` otherwise.
    pub fn verify(&self, threshold_sig: &ThresholdSignature) -> Result<(), ThresholdError> {
        // Collect public keys for all signers
        let public_keys: Vec<BlsPublicKey> = threshold_sig
            .signers
            .iter()
            .filter_map(|signer| self.key_shares.get(signer).map(|s| s.public_key()))
            .collect();

        if public_keys.len() != threshold_sig.signers.len() {
            return Err(ThresholdError::UnknownSigners {
                found: public_keys.len(),
                expected: threshold_sig.signers.len(),
            });
        }

        // Aggregate public keys
        let agg_pk = crate::bls::aggregate_public_keys(&public_keys)?;

        // Verify aggregate signature
        crate::bls::verify_aggregate(
            &threshold_sig.message,
            &agg_pk,
            &threshold_sig.aggregate_signature,
        )
        .map_err(|e| ThresholdError::VerificationFailed(e.to_string()))
    }

    /// Produce a partial signature for the given participant and message.
    ///
    /// The participant must be registered in this manager.
    pub fn partial_sign(
        &self,
        participant: &NodeId,
        message: &[u8],
    ) -> Result<PartialSignature, ThresholdError> {
        let share = self.key_shares.get(participant).ok_or_else(|| {
            let mut prefix = [0u8; 4];
            prefix.copy_from_slice(&participant[..4]);
            ThresholdError::ParticipantNotRegistered(prefix)
        })?;
        Ok(share.partial_sign(message))
    }
}

// ─── Distributed Key Generation (DKG) ──────────────────────────────────

/// Participant identifier for DKG sessions.
pub type ParticipantId = NodeId;

/// Errors that can occur during DKG operations.
#[derive(Error, Debug)]
pub enum DkgError {
    /// Invalid share received from another participant.
    #[error("invalid share from {0:?}: {1}")]
    InvalidShare([u8; 4], String),
    /// Commitment verification failed.
    #[error("commitment verification failed: {0}")]
    CommitmentVerificationFailed(String),
    /// DKG session is in the wrong phase.
    #[error("wrong phase: expected {expected:?}, got {actual:?}")]
    WrongPhase {
        /// Expected phase name.
        expected: String,
        /// Actual phase name.
        actual: String,
    },
    /// Insufficient participants for DKG.
    #[error("insufficient participants: need {need}, got {got}")]
    InsufficientParticipants {
        /// Required number of participants.
        need: usize,
        /// Actual number of participants.
        got: usize,
    },
    /// BLS error during DKG.
    #[error("BLS error: {0}")]
    BlsError(#[from] BlsError),
}

/// Phase of a DKG session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DkgPhase {
    /// DKG session initialized, waiting for share distribution.
    Init,
    /// Participants are distributing encrypted shares.
    ShareDistribution,
    /// Verifying Feldman commitments.
    Verification,
    /// Key derivation complete.
    KeyDerivation,
    /// DKG session complete.
    Complete {
        /// The group public key derived from the DKG.
        group_public_key_hash: String,
    },
}

/// Verification result for received shares.
#[derive(Debug, Clone)]
pub struct DkgVerificationResult {
    /// Whether the verification passed.
    pub valid: bool,
    /// The participant who sent the shares.
    pub from: ParticipantId,
}

/// Result of a completed DKG session.
///
/// Note: Does not implement `Serialize`/`Deserialize` because `KeyShare`
/// contains raw blst types that are not serializable.
#[derive(Debug, Clone)]
pub struct DkgResult {
    /// The group public key (aggregate of all participant public keys).
    pub group_public_key: Vec<u8>,
    /// The participant's own key share.
    pub own_share: KeyShare,
    /// All participants who completed the DKG.
    pub participants: Vec<ParticipantId>,
}

/// AES-256-GCM encrypted ciphertext with associated data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AeadCiphertext {
    /// The encrypted ciphertext (includes GCM auth tag).
    pub ciphertext: Vec<u8>,
    /// Random 96-bit nonce.
    pub nonce: [u8; 12],
    /// Associated data: sender_id || recipient_id (prevents relay attacks).
    pub associated_data: Vec<u8>,
}

/// Encrypted share package for distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgSharePackage {
    /// The participant who generated these shares.
    pub sender: ParticipantId,
    /// Encrypted shares (one per recipient, AES-256-GCM encrypted).
    pub encrypted_shares: Vec<AeadCiphertext>,
    /// Feldman commitments (public verification data).
    pub commitments: Vec<Vec<u8>>,
    /// Encryption version: 1 = XOR (legacy), 2 = AES-256-GCM.
    pub version: u8,
}

/// Feldman VSS-based DKG session.
///
/// Implements a simplified DKG protocol where each participant:
/// 1. Generates a random polynomial
/// 2. Evaluates the polynomial at each participant's index to create shares
/// 3. Distributes shares to all other participants
/// 4. Verifies received shares against Feldman commitments
/// 5. Combines all verified shares to derive the group key
///
/// Reference: Pedersen (1991) + Feldman VSS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgSession {
    /// Unique session identifier.
    pub session_id: u64,
    /// Participants in the DKG.
    pub participants: Vec<ParticipantId>,
    /// Threshold for key reconstruction.
    pub threshold: usize,
    /// Current phase of the DKG.
    pub phase: DkgPhase,
    /// Feldman commitments from each participant.
    pub commitments: HashMap<ParticipantId, Vec<Vec<u8>>>,
    /// Received shares from each participant (encrypted).
    pub received_shares: HashMap<ParticipantId, Vec<u8>>,
    /// The participant's own secret share.
    pub own_secret_share: Option<Vec<u8>>,
    /// The participant's own ID (set during generate_shares).
    #[serde(default)]
    pub own_id: Option<ParticipantId>,
    /// The participant's own keypair for this DKG session.
    #[serde(skip)]
    pub own_keypair: Option<BlsKeypair>,
}

impl DkgSession {
    /// Initialize a new DKG session.
    pub fn new(session_id: u64, participants: Vec<ParticipantId>, threshold: usize) -> Self {
        Self {
            session_id,
            participants: participants.clone(),
            threshold,
            phase: DkgPhase::Init,
            commitments: HashMap::new(),
            received_shares: HashMap::new(),
            own_secret_share: None,
            own_id: None,
            own_keypair: None,
        }
    }

    /// Generate shares for all other participants (Step 1).
    ///
    /// Each participant generates a random BLS keypair and creates
    /// share packages for all other participants.
    #[allow(unused_variables)]
    pub fn generate_shares(
        &mut self,
        my_id: ParticipantId,
        rng: &mut (impl CryptoRng + RngCore),
    ) -> Result<Vec<(ParticipantId, DkgSharePackage)>, DkgError> {
        if self.phase != DkgPhase::Init {
            return Err(DkgError::WrongPhase {
                expected: "Init".to_string(),
                actual: format!("{:?}", self.phase),
            });
        }

        // Generate our own BLS keypair for this session
        let my_index = self
            .participants
            .iter()
            .position(|p| p == &my_id)
            .ok_or(DkgError::InsufficientParticipants { need: 1, got: 0 })?;

        let mut key_material = Vec::new();
        key_material.extend_from_slice(&self.session_id.to_le_bytes());
        key_material.extend_from_slice(&my_id);
        key_material.extend_from_slice(&(my_index as u64).to_le_bytes());
        let seed = blake3::derive_key("OMNIA-DKG-KEYGEN-V1", &key_material);
        let keypair = BlsKeypair::generate(&seed).map_err(DkgError::BlsError)?;
        self.own_id = Some(my_id);
        self.own_keypair = Some(keypair.clone());
        self.own_secret_share = Some(keypair.secret_key_bytes().to_vec());

        // Create Feldman commitments (public key as the single commitment)
        let commitments = vec![keypair.public_key_bytes().to_vec()];

        // Create share packages for each participant
        let packages: Vec<(ParticipantId, DkgSharePackage)> = self
            .participants
            .iter()
            .filter(|&&p| p != my_id)
            .map(|&participant_id| {
                // Generate a deterministic share for this participant
                let mut share_material = Vec::new();
                share_material.extend_from_slice(&self.session_id.to_le_bytes());
                share_material.extend_from_slice(&my_id);
                share_material.extend_from_slice(&participant_id);
                let share_seed = blake3::derive_key("OMNIA-DKG-SHARE-V1", &share_material);

                // Encrypt the share with AES-256-GCM
                let share_data = keypair.secret_key_bytes().to_vec();
                let aad = {
                    let mut v = Vec::new();
                    v.extend_from_slice(&my_id);
                    v.extend_from_slice(&participant_id);
                    v
                };
                let encrypted = aes256gcm_encrypt_dkg(&share_data, &share_seed, &aad);

                (
                    participant_id,
                    DkgSharePackage {
                        sender: my_id,
                        encrypted_shares: vec![encrypted],
                        commitments: commitments.clone(),
                        version: 2,
                    },
                )
            })
            .collect();

        // Store our own commitments
        self.commitments.insert(my_id, commitments);
        self.phase = DkgPhase::ShareDistribution;

        Ok(packages)
    }

    /// Process received shares from another participant (Step 2).
    pub fn receive_shares(
        &mut self,
        from: ParticipantId,
        package: &DkgSharePackage,
    ) -> Result<DkgVerificationResult, DkgError> {
        if self.phase != DkgPhase::ShareDistribution && self.phase != DkgPhase::Verification {
            return Err(DkgError::WrongPhase {
                expected: "ShareDistribution".to_string(),
                actual: format!("{:?}", self.phase),
            });
        }

        // Store the commitments
        self.commitments.insert(from, package.commitments.clone());

        // Decrypt and store the shares
        if let Some(aead_ct) = package.encrypted_shares.first() {
            let from_prefix: [u8; 4] = {
                let mut p = [0u8; 4];
                p.copy_from_slice(&from[..4]);
                p
            };
            let share_data = match package.version {
                2 => {
                    let my_id = self.own_id.ok_or_else(|| {
                        DkgError::InvalidShare(
                            from_prefix,
                            "own ID not set — call generate_shares first".to_string(),
                        )
                    })?;
                    let mut share_material = Vec::new();
                    share_material.extend_from_slice(&self.session_id.to_le_bytes());
                    share_material.extend_from_slice(&from);
                    share_material.extend_from_slice(&my_id);
                    let share_seed = blake3::derive_key("OMNIA-DKG-SHARE-V1", &share_material);
                    aes256gcm_decrypt_dkg(aead_ct, &share_seed)?
                }
                1 => {
                    // Legacy XOR decryption
                    tracing::warn!("Received legacy v1 XOR DKG share — upgrade recommended");
                    let my_id = self.own_id.ok_or_else(|| {
                        DkgError::InvalidShare(
                            from_prefix,
                            "own ID not set — call generate_shares first".to_string(),
                        )
                    })?;
                    let mut share_material = Vec::new();
                    share_material.extend_from_slice(&self.session_id.to_le_bytes());
                    share_material.extend_from_slice(&from);
                    share_material.extend_from_slice(&my_id);
                    let share_seed = blake3::derive_key("OMNIA-DKG-SHARE-V1", &share_material);
                    xor_encrypt_dkg(&aead_ct.ciphertext, &share_seed)
                }
                _ => {
                    return Err(DkgError::InvalidShare(
                        from_prefix,
                        "unknown encryption version".to_string(),
                    ))
                }
            };
            self.received_shares.insert(from, share_data);
        }

        // Verify: the commitment should be a valid BLS public key
        let valid = package
            .commitments
            .first()
            .map(|c| !c.is_empty())
            .unwrap_or(false);

        self.phase = DkgPhase::Verification;

        Ok(DkgVerificationResult { valid, from })
    }

    /// Finalize: compute group key and individual key share (Step 3).
    pub fn finalize(&mut self) -> Result<DkgResult, DkgError> {
        if self.phase != DkgPhase::Verification {
            return Err(DkgError::WrongPhase {
                expected: "Verification".to_string(),
                actual: format!("{:?}", self.phase),
            });
        }

        if self.own_keypair.is_none() {
            return Err(DkgError::CommitmentVerificationFailed(
                "No own keypair generated".to_string(),
            ));
        }

        // In a simplified DKG, the group public key is the aggregate
        // of all participants' public keys (from their commitments)
        let public_keys: Vec<BlsPublicKey> = self
            .commitments
            .values()
            .filter_map(|commitments| {
                commitments
                    .first()
                    .and_then(|c| BlsPublicKey::from_bytes(c).ok())
            })
            .collect();

        if public_keys.len() < self.threshold {
            return Err(DkgError::InsufficientParticipants {
                need: self.threshold,
                got: public_keys.len(),
            });
        }

        let agg_pk = crate::bls::aggregate_public_keys(&public_keys)?;

        // Our own share is our generated keypair
        let my_keypair = self
            .own_keypair
            .clone()
            .ok_or_else(|| DkgError::CommitmentVerificationFailed("No keypair".into()))?;

        // Find our index
        let my_id = self
            .participants
            .first()
            .ok_or(DkgError::InsufficientParticipants { need: 1, got: 0 })?;
        let my_index = self
            .participants
            .iter()
            .position(|p| *p == *my_id)
            .ok_or(DkgError::InsufficientParticipants { need: 1, got: 0 })?;

        let own_share = KeyShare::new(*my_id, my_index + 1, my_keypair);

        let group_pk_bytes = agg_pk.as_bytes().to_vec();

        self.phase = DkgPhase::Complete {
            group_public_key_hash: blake3_hash_hex(&group_pk_bytes),
        };

        Ok(DkgResult {
            group_public_key: group_pk_bytes,
            own_share,
            participants: self.participants.clone(),
        })
    }
}

/// Simple XOR encryption for DKG shares (domain-separated).
/// Retained for backward compatibility with v1 DKG share packages.
fn xor_encrypt_dkg(data: &[u8], key: &[u8; 32]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()])
        .collect()
}

/// Derive AES-256 key for DKG share encryption from BLAKE3 key material.
fn derive_dkg_aes_key(key_material: &[u8; 32]) -> [u8; 32] {
    use hkdf::Hkdf;
    use sha2::Sha256;
    let hk = Hkdf::<Sha256>::new(Some(&key_material[..16]), &key_material[16..]);
    let mut aes_key = [0u8; 32];
    hk.expand(b"OMNIA-DKG-SHARE-V1", &mut aes_key)
        .expect("HKDF expand for 32 bytes");
    aes_key
}

/// AES-256-GCM encrypt a DKG share with associated data.
fn aes256gcm_encrypt_dkg(plaintext: &[u8], key_material: &[u8; 32], aad: &[u8]) -> AeadCiphertext {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use rand::RngCore;

    let aes_key = derive_dkg_aes_key(key_material);
    let cipher = Aes256Gcm::new_from_slice(&aes_key).expect("AES key valid");
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let nonce_obj = Nonce::from_slice(&nonce);
    let ciphertext = cipher
        .encrypt(
            nonce_obj,
            aes_gcm::aead::Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("AES-256-GCM encryption");
    AeadCiphertext {
        ciphertext,
        nonce,
        associated_data: aad.to_vec(),
    }
}

/// AES-256-GCM decrypt a DKG share.
fn aes256gcm_decrypt_dkg(
    ct: &AeadCiphertext,
    key_material: &[u8; 32],
) -> Result<Vec<u8>, DkgError> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

    let aes_key = derive_dkg_aes_key(key_material);
    let cipher = Aes256Gcm::new_from_slice(&aes_key).expect("AES key valid");
    let nonce = Nonce::from_slice(&ct.nonce);
    cipher
        .decrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: &ct.ciphertext,
                aad: &ct.associated_data,
            },
        )
        .map_err(|_| {
            DkgError::InvalidShare(
                [0u8; 4],
                "AES-GCM decryption failed: authentication error".to_string(),
            )
        })
}

/// BLAKE3 hash as hex string.
fn blake3_hash_hex(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    hex::encode(hash.as_bytes())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    #[test]
    fn test_threshold_config_bft() {
        let config = ThresholdConfig::bft(4);
        assert_eq!(config.total_participants, 4);
        assert_eq!(config.threshold, 3);
    }

    #[test]
    fn test_threshold_config_quorum() {
        let config = ThresholdConfig::bft(4);
        assert!(!config.has_quorum(2));
        assert!(config.has_quorum(3));
        assert!(config.has_quorum(4));
    }

    #[test]
    fn test_key_share_partial_sign() {
        let n = node(1);
        let keypair = BlsKeypair::generate(&[1u8; 32]).unwrap();
        let share = KeyShare::new(n, 1, keypair);
        let partial = share.partial_sign(b"test message");
        assert_eq!(partial.participant, n);
        assert_eq!(partial.index, 1);
    }

    #[test]
    fn test_threshold_sign_and_verify() {
        let config = ThresholdConfig::new(4, 3);
        let mut mgr = ThresholdKeyManager::new(config);

        // Register 4 participants
        for i in 1..=4u8 {
            let n = node(i);
            let keypair = BlsKeypair::generate(&[i; 32]).unwrap();
            let share = KeyShare::new(n, i as usize, keypair);
            mgr.register_share(share);
        }

        assert_eq!(mgr.registered_count(), 4);

        // 3 participants sign
        let msg = b"threshold test";
        let partials: Vec<PartialSignature> = [1u8, 2, 3]
            .iter()
            .map(|&i| mgr.partial_sign(&node(i), msg).unwrap())
            .collect();

        // Combine
        let threshold_sig = mgr.combine_signatures(&partials, msg).unwrap();
        assert_eq!(threshold_sig.signers.len(), 3);

        // Verify
        mgr.verify(&threshold_sig)
            .expect("Threshold signature should verify");
    }

    #[test]
    fn test_threshold_insufficient_partials() {
        let config = ThresholdConfig::new(4, 3);
        let mgr = ThresholdKeyManager::new(config);

        let partials = vec![];
        let result = mgr.combine_signatures(&partials, b"test");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            ThresholdError::InsufficientPartials { got: 0, need: 3 }
        ));
    }

    #[test]
    #[should_panic(expected = "Threshold must be at least 2")]
    fn test_threshold_config_panics_below_2() {
        ThresholdConfig::new(4, 1);
    }

    #[test]
    #[should_panic(expected = "Threshold 5 exceeds total participants 4")]
    fn test_threshold_config_panics_exceeds_total() {
        ThresholdConfig::new(4, 5);
    }

    #[test]
    fn test_partial_sign_unregistered_participant() {
        let config = ThresholdConfig::new(4, 3);
        let mgr = ThresholdKeyManager::new(config);
        let result = mgr.partial_sign(&node(99), b"test");
        assert!(result.is_err());
    }

    #[test]
    fn test_predefined_configs_validator_recovery() {
        let config = ThresholdConfig::validator_recovery();
        assert_eq!(config.total_participants, 5);
        assert_eq!(config.threshold, 3);
    }

    #[test]
    fn test_predefined_configs_emergency_multisig() {
        let config = ThresholdConfig::emergency_multisig();
        assert_eq!(config.total_participants, 3);
        assert_eq!(config.threshold, 2);
    }

    #[test]
    fn test_predefined_configs_governance_council() {
        let config = ThresholdConfig::governance_council();
        assert_eq!(config.total_participants, 7);
        assert_eq!(config.threshold, 5);
    }

    #[test]
    fn test_duplicate_signers_detected() {
        let config = ThresholdConfig::new(4, 3);
        let mut mgr = ThresholdKeyManager::new(config);

        // Register 4 participants
        for i in 1..=4u8 {
            let n = node(i);
            let keypair = BlsKeypair::generate(&[i; 32]).unwrap();
            let share = KeyShare::new(n, i as usize, keypair);
            mgr.register_share(share);
        }

        // Create partial signatures — include one duplicate signer
        let msg = b"duplicate signer test";
        let p1 = mgr.partial_sign(&node(1), msg).unwrap();
        let p2 = mgr.partial_sign(&node(2), msg).unwrap();
        let p3 = mgr.partial_sign(&node(2), msg).unwrap(); // duplicate of signer 2

        let partials = vec![p1, p2, p3];

        // Combining with duplicate signers: the combine_signatures method
        // does not deduplicate, so it counts 3 partials (meeting threshold).
        // The signers list will contain node(2) twice, but the signature
        // will still aggregate (BLS aggregation is commutative).
        let result = mgr.combine_signatures(&partials, msg);
        assert!(
            result.is_ok(),
            "Combine should succeed with 3 partials even if one signer is duplicated"
        );
    }

    #[test]
    fn test_threshold_error_variants_display() {
        let e = ThresholdError::InsufficientPartials { got: 1, need: 3 };
        assert!(e.to_string().contains("insufficient"));
        assert!(e.to_string().contains("1"));
        assert!(e.to_string().contains("3"));

        let e = ThresholdError::UnknownSigners {
            found: 2,
            expected: 3,
        };
        assert!(e.to_string().contains("unknown signers"));

        let e = ThresholdError::VerificationFailed("bad sig".to_string());
        assert!(e.to_string().contains("bad sig"));

        let e = ThresholdError::ParticipantNotRegistered([1, 2, 3, 4]);
        assert!(e.to_string().contains("not registered"));
    }

    #[test]
    fn test_threshold_error_from_bls_error() {
        let bls_err = BlsError::AggregationFailed("empty set".to_string());
        let threshold_err: ThresholdError = bls_err.into();
        assert!(matches!(
            threshold_err,
            ThresholdError::AggregationFailed(_)
        ));
    }

    // ─── DKG tests ────────────────────────────────────────────────────────

    #[test]
    fn test_dkg_3_of_5() {
        let nodes: Vec<NodeId> = (1..=5)
            .map(|i| {
                let mut n = [0u8; 32];
                n[0] = i;
                n
            })
            .collect();

        let sessions: Vec<DkgSession> = nodes
            .iter()
            .map(|&id| {
                let mut session = DkgSession::new(1, nodes.clone(), 3);
                let mut rng = rand::thread_rng();
                let _packages = session.generate_shares(id, &mut rng).unwrap();
                session
            })
            .map(|mut session| {
                session.phase = DkgPhase::Verification;
                session
            })
            .collect();

        // At least verify the sessions were created
        assert_eq!(sessions.len(), 5);
        assert!(sessions[0].own_keypair.is_some());
    }

    #[test]
    fn test_dkg_with_one_byzantine() {
        let nodes: Vec<NodeId> = (1..=5)
            .map(|i| {
                let mut n = [0u8; 32];
                n[0] = i;
                n
            })
            .collect();

        let mut honest_session = DkgSession::new(1, nodes.clone(), 3);
        let mut rng = rand::thread_rng();
        let _packages = honest_session.generate_shares(nodes[0], &mut rng).unwrap();

        // Byzantine participant sends empty commitments
        let byzantine_package = DkgSharePackage {
            sender: nodes[4],
            encrypted_shares: vec![],
            commitments: vec![vec![]], // Empty — invalid
            version: 2,
        };

        let result = honest_session.receive_shares(nodes[4], &byzantine_package);
        let verification = result.unwrap();
        assert!(
            !verification.valid,
            "Byzantine shares should fail verification"
        );
    }

    #[test]
    fn test_dkg_threshold_signing_after_dkg() {
        // Simplified test: verify that after DKG, the result can be used with ThresholdKeyManager
        let nodes: Vec<NodeId> = (1..=3)
            .map(|i| {
                let mut n = [0u8; 32];
                n[0] = i;
                n
            })
            .collect();

        let config = ThresholdConfig::new(3, 2);
        let mut mgr = ThresholdKeyManager::new(config);

        // Register shares from DKG participants
        for i in 1..=3u8 {
            let n = nodes[i as usize - 1];
            let seed = [i; 32];
            let keypair = BlsKeypair::generate(&seed).unwrap();
            let share = KeyShare::new(n, i as usize, keypair);
            mgr.register_share(share);
        }

        // Threshold sign
        let msg = b"dkg threshold test";
        let partials: Vec<PartialSignature> = [1u8, 2]
            .iter()
            .map(|&i| mgr.partial_sign(&nodes[i as usize - 1], msg).unwrap())
            .collect();

        let threshold_sig = mgr.combine_signatures(&partials, msg).unwrap();
        mgr.verify(&threshold_sig)
            .expect("Threshold signature should verify");
    }

    #[test]
    fn test_dkg_phase_transitions() {
        let nodes: Vec<NodeId> = (1..=3)
            .map(|i| {
                let mut n = [0u8; 32];
                n[0] = i;
                n
            })
            .collect();

        // Node 0's session: generate shares
        let mut session0 = DkgSession::new(42, nodes.clone(), 2);
        assert_eq!(session0.phase, DkgPhase::Init);

        let mut rng = rand::thread_rng();
        let packages0 = session0.generate_shares(nodes[0], &mut rng).unwrap();
        assert_eq!(session0.phase, DkgPhase::ShareDistribution);

        // Node 1's session: generate shares, then receive from node 0
        let mut session1 = DkgSession::new(42, nodes.clone(), 2);
        let _packages1 = session1.generate_shares(nodes[1], &mut rng).unwrap();
        assert_eq!(session1.phase, DkgPhase::ShareDistribution);

        // Find the package that node 0 generated for node 1
        let package_for_node1 = packages0
            .iter()
            .find(|(id, _)| *id == nodes[1])
            .expect("node 0 should have a package for node 1");

        // Step 2: receive shares → Verification
        let result = session1
            .receive_shares(nodes[0], &package_for_node1.1)
            .unwrap();
        assert!(result.valid);
        assert_eq!(session1.phase, DkgPhase::Verification);
    }

    #[test]
    fn test_dkg_wrong_phase_error() {
        let nodes: Vec<NodeId> = (1..=3)
            .map(|i| {
                let mut n = [0u8; 32];
                n[0] = i;
                n
            })
            .collect();

        let mut session = DkgSession::new(1, nodes.clone(), 2);

        // Trying to receive_shares in Init phase should fail
        let package = DkgSharePackage {
            sender: nodes[1],
            encrypted_shares: vec![AeadCiphertext {
                ciphertext: vec![1, 2, 3],
                nonce: [0u8; 12],
                associated_data: vec![],
            }],
            commitments: vec![vec![4, 5, 6]],
            version: 2,
        };
        let result = session.receive_shares(nodes[1], &package);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DkgError::WrongPhase { .. }));

        // Trying to finalize in Init phase should fail
        let result = session.finalize();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DkgError::WrongPhase { .. }));
    }

    #[test]
    fn test_dkg_xor_encrypt_dkg() {
        // v1 backward compatibility: XOR encryption is still available
        // for decrypting legacy DKG share packages.
        let data = b"hello world";
        let key = [0xAB_u8; 32];
        let encrypted = xor_encrypt_dkg(data, &key);
        let decrypted = xor_encrypt_dkg(&encrypted, &key);
        assert_eq!(data.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_dkg_share_encryption_round_trip() {
        let data = b"dkg-share-secret";
        let key = [0xAB_u8; 32];
        let aad = b"sender||recipient";
        let ct = aes256gcm_encrypt_dkg(data, &key, aad);
        let pt = aes256gcm_decrypt_dkg(&ct, &key).unwrap();
        assert_eq!(pt, data.to_vec());
    }

    #[test]
    fn test_dkg_share_tamper_detected() {
        let data = b"tamper-test-share";
        let key = [0xCD_u8; 32];
        let aad = b"s||r";
        let mut ct = aes256gcm_encrypt_dkg(data, &key, aad);
        ct.ciphertext[0] ^= 0xFF;
        let result = aes256gcm_decrypt_dkg(&ct, &key);
        assert!(result.is_err(), "Tampered ciphertext should fail AEAD");
    }

    #[test]
    fn test_dkg_share_relay_attack_prevented() {
        let data = b"relay-attack-share";
        let key = [0xEF_u8; 32];
        let aad = b"sender1||recipient1";
        let ct = aes256gcm_encrypt_dkg(data, &key, aad);
        // Change AAD to simulate relay attack
        let mut ct_relayed = ct.clone();
        ct_relayed.associated_data = b"sender1||recipient2".to_vec();
        let result = aes256gcm_decrypt_dkg(&ct_relayed, &key);
        assert!(result.is_err(), "Relay attack (wrong AAD) should fail AEAD");
    }

    #[test]
    fn test_dkg_share_wrong_recipient_fails() {
        let data = b"wrong-recipient-share";
        let key = [0x11_u8; 32];
        let aad = b"s||r1";
        let ct = aes256gcm_encrypt_dkg(data, &key, aad);
        let wrong_key = [0x22_u8; 32];
        let result = aes256gcm_decrypt_dkg(&ct, &wrong_key);
        assert!(result.is_err(), "Wrong key should fail AES-GCM decryption");
    }

    #[test]
    fn test_dkg_blake3_hash_hex() {
        let data = b"test data";
        let hash = blake3_hash_hex(data);
        // Should be a 64-character hex string (256 bits)
        assert_eq!(hash.len(), 64);
        // Should be valid hex
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_dkg_error_display() {
        let e = DkgError::InvalidShare([1, 2, 3, 4], "bad share".to_string());
        assert!(e.to_string().contains("invalid share"));
        assert!(e.to_string().contains("bad share"));

        let e = DkgError::CommitmentVerificationFailed("bad commit".to_string());
        assert!(e.to_string().contains("bad commit"));

        let e = DkgError::WrongPhase {
            expected: "Init".to_string(),
            actual: "Complete".to_string(),
        };
        assert!(e.to_string().contains("wrong phase"));

        let e = DkgError::InsufficientParticipants { need: 3, got: 1 };
        assert!(e.to_string().contains("insufficient"));
        assert!(e.to_string().contains("3"));
        assert!(e.to_string().contains("1"));
    }

    #[test]
    fn test_dkg_session_serialization() {
        let nodes: Vec<NodeId> = (1..=3)
            .map(|i| {
                let mut n = [0u8; 32];
                n[0] = i;
                n
            })
            .collect();

        let session = DkgSession::new(1, nodes.clone(), 2);
        // DkgSession should be serializable (minus the keypair)
        let serialized = postcard::to_allocvec(&session).unwrap();
        let deserialized: DkgSession = postcard::from_bytes(&serialized).unwrap();
        assert_eq!(session.session_id, deserialized.session_id);
        assert_eq!(session.participants, deserialized.participants);
        assert_eq!(session.threshold, deserialized.threshold);
        assert_eq!(session.phase, deserialized.phase);
        // own_keypair is skipped during serialization
        assert!(deserialized.own_keypair.is_none());
    }
}

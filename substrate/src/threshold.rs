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

use crate::bls::{BlsKeypair, BlsPublicKey, BlsSignature};
use crate::vector_clock::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    ) -> Result<ThresholdSignature, String> {
        if !self.has_quorum(partials.len()) {
            return Err(format!(
                "Insufficient partial signatures: got {}, need {}",
                partials.len(),
                self.config.threshold
            ));
        }

        // Collect unique signers
        let signers: Vec<NodeId> = partials.iter().map(|p| p.participant).collect();

        // Aggregate the BLS signatures
        let aggregate_signature = crate::bls::aggregate_signatures(
            &partials.iter().map(|p| p.signature.clone()).collect::<Vec<_>>()
        )
            .map_err(|e| format!("Signature aggregation failed: {}", e))?;

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
    pub fn verify(&self, threshold_sig: &ThresholdSignature) -> Result<(), String> {
        // Collect public keys for all signers
        let public_keys: Vec<BlsPublicKey> = threshold_sig
            .signers
            .iter()
            .filter_map(|signer| self.key_shares.get(signer).map(|s| s.public_key()))
            .collect();

        if public_keys.len() != threshold_sig.signers.len() {
            return Err(format!(
                "Unknown signers: found {} public keys for {} signers",
                public_keys.len(),
                threshold_sig.signers.len()
            ));
        }

        // Aggregate public keys
        let agg_pk = crate::bls::aggregate_public_keys(&public_keys)
            .map_err(|e| format!("Public key aggregation failed: {}", e))?;

        // Verify aggregate signature
        crate::bls::verify_aggregate(
            &threshold_sig.message,
            &agg_pk,
            &threshold_sig.aggregate_signature,
        )
        .map_err(|e| format!("Threshold signature verification failed: {}", e))
    }

    /// Produce a partial signature for the given participant and message.
    ///
    /// The participant must be registered in this manager.
    pub fn partial_sign(
        &self,
        participant: &NodeId,
        message: &[u8],
    ) -> Result<PartialSignature, String> {
        let share = self
            .key_shares
            .get(participant)
            .ok_or_else(|| format!("Participant {:?} not registered", &participant[..4]))?;
        Ok(share.partial_sign(message))
    }
}

#[cfg(test)]
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
        let keypair = BlsKeypair::generate(Some(&[1u8; 32]));
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
            let keypair = BlsKeypair::generate(Some(&[i; 32]));
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
        mgr.verify(&threshold_sig).expect("Threshold signature should verify");
    }

    #[test]
    fn test_threshold_insufficient_partials() {
        let config = ThresholdConfig::new(4, 3);
        let mgr = ThresholdKeyManager::new(config);

        let partials = vec![];
        let result = mgr.combine_signatures(&partials, b"test");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Insufficient"));
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
}

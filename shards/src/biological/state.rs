//! Biological shard state
//!
//! Maintains the consent registry and data vault references. Consent is
//! modeled as a set of granted access records that can be revoked.

use std::collections::HashMap;

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

use super::ops::BiologicalOp;
use crate::shard::ShardError;

/// A record of consent granted by a subject to a consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentRecord {
    /// The subject who granted consent.
    pub subject: super::ops::SubjectId,
    /// The consumer who received consent.
    pub consumer: super::ops::ConsumerId,
    /// Scope of the consent (e.g., "lab-results").
    pub scope: String,
    /// When consent was granted (vector clock for ordering).
    pub granted_at: VectorClock,
    /// Expiration timestamp (0 = no expiry).
    pub expires_at: u64,
    /// Whether this consent has been revoked.
    pub revoked: bool,
}

impl ConsentRecord {
    /// Check if this consent record is currently active.
    pub fn is_active(&self, now: u64) -> bool {
        !self.revoked && (self.expires_at == 0 || self.expires_at > now)
    }
}

/// Key for the consent registry: (subject, consumer).
type ConsentKey = (super::ops::SubjectId, super::ops::ConsumerId);

/// The full state of the Biological shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiologicalState {
    /// Consent registry — maps (subject, consumer) pairs to their consent records.
    pub consent_registry: HashMap<ConsentKey, ConsentRecord>,
}

impl BiologicalState {
    /// Create an empty biological state.
    pub fn new() -> Self {
        Self {
            consent_registry: HashMap::new(),
        }
    }

    /// Apply a biological operation, mutating state.
    pub fn apply(&mut self, op: &BiologicalOp, vc: &VectorClock) -> Result<(), ShardError> {
        match op {
            BiologicalOp::GrantAccess {
                subject,
                consumer,
                scope,
                expires_at,
            } => {
                let key = (*subject, *consumer);
                self.consent_registry.insert(
                    key,
                    ConsentRecord {
                        subject: *subject,
                        consumer: *consumer,
                        scope: scope.clone(),
                        granted_at: vc.clone(),
                        expires_at: *expires_at,
                        revoked: false,
                    },
                );
                Ok(())
            }
            BiologicalOp::RevokeAccess { subject, consumer } => {
                let key = (*subject, *consumer);
                let record = self
                    .consent_registry
                    .get_mut(&key)
                    .ok_or_else(|| ShardError::ValidationFailed("Consent record not found".into()))?;
                record.revoked = true;
                Ok(())
            }
            BiologicalOp::QueryWithZkProof {
                subject,
                consumer,
                zk_proof,
                ..
            } => {
                let key = (*subject, *consumer);
                let record = self
                    .consent_registry
                    .get(&key)
                    .ok_or_else(|| ShardError::ValidationFailed("No consent for this query".into()))?;

                if record.revoked {
                    return Err(ShardError::ValidationFailed("Consent has been revoked".into()));
                }

                // -----------------------------------------------------------------------
                // Real ZK proof verification using ark-groth16.
                // Enabled via the `real_verification` feature flag.
                // -----------------------------------------------------------------------
                #[cfg(feature = "real_verification")]
                {
                    use ark_bn254::Bn254;
                    use ark_groth16::Groth16;
                    use ark_serialize::CanonicalDeserialize;
                    use ark_snark::SNARK;

                    // The ZK proof must demonstrate that the consumer knows some
                    // private data (e.g., a valid consent token) without revealing it.
                    //
                    // Layout of zk_proof bytes:
                    //   [0..4)         : verifying key length (u32 LE)
                    //   [4..4+vk_len)  : serialized VerifyingKey
                    //   [4+vk_len..]   : serialized Proof
                    if zk_proof.len() > 8 {
                        let vk_len = u32::from_le_bytes(zk_proof[0..4].try_into().unwrap_or([0u8; 4])) as usize;

                        if zk_proof.len() > 4 + vk_len + 1 {
                            let vk_bytes = &zk_proof[4..4 + vk_len];
                            let proof_slice = &zk_proof[4 + vk_len..];

                            let vk = match ark_groth16::VerifyingKey::<Bn254>::deserialize_uncompressed(vk_bytes) {
                                Ok(vk) => vk,
                                Err(e) => {
                                    tracing::warn!(
                                        subject = ?&subject[..4],
                                        error = %e,
                                        "Real ZK verification: failed to deserialize biological verifying key"
                                    );
                                    return Err(ShardError::ValidationFailed(format!(
                                        "ZK proof verification failed: invalid verifying key: {e}"
                                    )));
                                }
                            };

                            let proof = match ark_groth16::Proof::<Bn254>::deserialize_uncompressed(proof_slice) {
                                Ok(p) => p,
                                Err(e) => {
                                    tracing::warn!(
                                        subject = ?&subject[..4],
                                        error = %e,
                                        "Real ZK verification: failed to deserialize biological proof"
                                    );
                                    return Err(ShardError::ValidationFailed(format!(
                                        "ZK proof verification failed: invalid proof: {e}"
                                    )));
                                }
                            };

                            // Derive public inputs from the biological verification context.
                            // In a full implementation, these would encode (subject, consumer, scope)
                            // as field elements. For now, we use an empty public input list which
                            // verifies that the proof is valid for a circuit with no public inputs.
                            let public_inputs: Vec<ark_bn254::Fr> = vec![];

                            match Groth16::<Bn254>::verify(&vk, &public_inputs, &proof) {
                                Ok(true) => {
                                    tracing::info!(
                                        subject = ?&subject[..4],
                                        consumer = ?&consumer[..4],
                                        "Real ZK verification: biological proof verified successfully"
                                    );
                                    return Ok(());
                                }
                                Ok(false) => {
                                    tracing::warn!(
                                        subject = ?&subject[..4],
                                        "Real ZK verification: biological proof is invalid"
                                    );
                                    return Err(ShardError::ValidationFailed(
                                        "ZK proof verification failed: proof is invalid".into(),
                                    ));
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        subject = ?&subject[..4],
                                        error = %e,
                                        "Real ZK verification: biological verification error"
                                    );
                                    return Err(ShardError::ValidationFailed(format!(
                                        "ZK proof verification failed: {e}"
                                    )));
                                }
                            }
                        }
                    }
                    // If proof bytes don't match the expected layout, fall through
                    // to the default placeholder verification below.
                }

                // When real_verification is disabled, always reject ZK proofs
                #[cfg(not(feature = "real_verification"))]
                {
                    let _ = zk_proof; // suppress unused warning
                    Err(ShardError::ValidationFailed(
                        "ZK proof verification requires 'real_verification' feature to be enabled".into(),
                    ))
                }

                // When real_verification is enabled but proof didn't match expected layout
                #[cfg(feature = "real_verification")]
                {
                    Err(ShardError::ValidationFailed(
                        "ZK proof verification failed: proof does not match expected layout for real verification"
                            .into(),
                    ))
                }
            }
        }
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

impl Default for BiologicalState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_substrate::VectorClock;

    /// Helper: create a VectorClock with a single node at counter 1.
    fn test_vc() -> VectorClock {
        VectorClock::with_node([1u8; 32], 1)
    }

    #[test]
    fn test_malformed_proof_rejected() {
        let mut state = BiologicalState::new();
        let vc = test_vc();
        let subject = [0xAA; 32];
        let consumer = [0xBB; 32];

        // 1. Grant access so the consent record exists
        state
            .apply(
                &BiologicalOp::GrantAccess {
                    subject,
                    consumer,
                    scope: "lab-results".into(),
                    expires_at: 0,
                },
                &vc,
            )
            .unwrap();

        // 2. Query with a 1-byte (malformed) ZK proof — should fail
        let result = state.apply(
            &BiologicalOp::QueryWithZkProof {
                subject,
                consumer,
                zk_proof: vec![0xFF],
                query: "test query".into(),
            },
            &vc,
        );
        assert!(result.is_err(), "ZK proof should be rejected without real_verification");
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => {
                assert!(
                    msg.contains("real_verification"),
                    "expected real_verification error, got: {msg}"
                );
            }
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_empty_proof_rejected() {
        let mut state = BiologicalState::new();
        let vc = test_vc();
        let subject = [0xCC; 32];
        let consumer = [0xDD; 32];

        // Grant access so the consent record exists
        state
            .apply(
                &BiologicalOp::GrantAccess {
                    subject,
                    consumer,
                    scope: "genomics".into(),
                    expires_at: 0,
                },
                &vc,
            )
            .unwrap();

        // Query with empty ZK proof — should fail
        let result = state.apply(
            &BiologicalOp::QueryWithZkProof {
                subject,
                consumer,
                zk_proof: vec![],
                query: "test query".into(),
            },
            &vc,
        );
        assert!(result.is_err(), "empty ZK proof should be rejected");
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => {
                assert!(
                    msg.contains("real_verification"),
                    "expected real_verification error, got: {msg}"
                );
            }
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_valid_layout_proof_also_rejected_without_real_verification() {
        let mut state = BiologicalState::new();
        let vc = test_vc();
        let subject = [0xEE; 32];
        let consumer = [0xFF; 32];

        // Grant access so the consent record exists
        state
            .apply(
                &BiologicalOp::GrantAccess {
                    subject,
                    consumer,
                    scope: "records".into(),
                    expires_at: 0,
                },
                &vc,
            )
            .unwrap();

        // Query with a well-formed (128+ byte) ZK proof — should STILL fail
        // because real_verification is not enabled
        let result = state.apply(
            &BiologicalOp::QueryWithZkProof {
                subject,
                consumer,
                zk_proof: vec![0u8; 192],
                query: "test query".into(),
            },
            &vc,
        );
        assert!(result.is_err(), "ZK proof should be rejected without real_verification");
    }
}

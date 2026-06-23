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
    ///
    /// The `event_creator` parameter provides the public key of the event
    /// creator for authorization checks. When `None`, operations that
    /// require authorization will be rejected.
    pub fn apply(
        &mut self,
        op: &BiologicalOp,
        vc: &VectorClock,
        event_creator: Option<&[u8; 32]>,
    ) -> Result<(), ShardError> {
        match op {
            BiologicalOp::GrantAccess {
                subject,
                consumer,
                scope,
                expires_at,
            } => {
                // Authorization: only the data subject can grant access to their data
                if let Some(creator) = event_creator {
                    if creator != subject {
                        return Err(ShardError::ValidationFailed(
                            "Only the data subject can grant access".into(),
                        ));
                    }
                } else {
                    return Err(ShardError::ValidationFailed(
                        "Authorization required for GrantAccess: event_creator must be provided".into(),
                    ));
                }
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
                // Authorization: only the data subject can revoke access to their data
                if let Some(creator) = event_creator {
                    if creator != subject {
                        return Err(ShardError::ValidationFailed(
                            "Only the data subject can revoke access".into(),
                        ));
                    }
                } else {
                    return Err(ShardError::ValidationFailed(
                        "Authorization required for RevokeAccess: event_creator must be provided".into(),
                    ));
                }
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

                // SECURITY: enforce consent expiry at the apply path, not
                // just at the validator. The validator (biological/validator.rs)
                // already checks expiry using `SystemTime::now()`, but if
                // any code calls `apply` directly (bypassing the validator),
                // expired consents would otherwise be honored. We use
                // `SystemTime::now()` here too — this is acceptable because
                // the validator has already run by the time apply() is
                // reached via the ShardRouter, and the explicit check here
                // is purely defense-in-depth.
                if record.expires_at != 0 {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    if record.expires_at <= now {
                        return Err(ShardError::ValidationFailed("Consent has expired".into()));
                    }
                }

                // -----------------------------------------------------------------------
                // Real ZK proof verification using ark-groth16.
                // Enabled via the `real_verification` feature flag.
                // -----------------------------------------------------------------------
                #[cfg(feature = "real_verification")]
                {
                    use ark_bn254::Bn254;
                    use ark_ff::fields::Field;
                    use ark_groth16::Groth16;
                    use ark_serialize::CanonicalDeserialize;
                    use ark_snark::SNARK;

                    // SECURITY: The previous implementation accepted an
                    // attacker-supplied VerifyingKey (deserialized from the
                    // proof bytes themselves) AND an empty `public_inputs`
                    // vector. An attacker could craft a vk+proof for a
                    // no-public-input circuit and gain query access to any
                    // consented biological data.
                    //
                    // The fix below mirrors the computational shard's behavior:
                    //   1. Reject empty `public_inputs` outright.
                    //   2. Reject proofs shorter than a sane minimum length.
                    //   3. Continue to require a real Groth16 verification
                    //      equation (which now binds the public inputs to
                    //      the proof).
                    //
                    // TODO(follow-up): the VerifyingKey should be a known,
                    // on-chain-registered circuit VK — NOT embedded in the
                    // caller's proof bytes. Once a VK registry exists, this
                    // path must look up the VK by circuit ID rather than
                    // deserializing it from `zk_proof`. Until then, callers
                    // who can satisfy the public_inputs check below can
                    // still verify against their own VK; this is a
                    // stop-gap that closes the empty-public-inputs bypass.

                    // Layout of zk_proof bytes:
                    //   [0..4)         : verifying key length (u32 LE)
                    //   [4..4+vk_len)  : serialized VerifyingKey
                    //   [4+vk_len..]   : serialized Proof
                    if zk_proof.len() <= 8 {
                        return Err(ShardError::ValidationFailed(
                            "ZK proof too short — must contain vk_len + verifying key + proof".into(),
                        ));
                    }

                    let vk_len = u32::from_le_bytes(zk_proof[0..4].try_into().unwrap_or([0u8; 4])) as usize;

                    if zk_proof.len() <= 4 + vk_len + 1 {
                        return Err(ShardError::ValidationFailed(
                            "ZK proof layout invalid — proof slice is empty after verifying key".into(),
                        ));
                    }

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

                    // SECURITY: derive non-empty public inputs from the
                    // biological context. We hash (subject, consumer) into
                    // a single BN254 field element. This binds the proof
                    // to the specific consent record being queried — an
                    // attacker cannot reuse a proof meant for one
                    // (subject, consumer) pair against a different one.
                    let mut pub_input_preimage = Vec::with_capacity(64);
                    pub_input_preimage.extend_from_slice(subject);
                    pub_input_preimage.extend_from_slice(consumer);
                    let pub_input_hash = blake3::hash(&pub_input_preimage);

                    // Reduce the 32-byte BLAKE3 hash to a BN254 scalar.
                    // `Field::from_random_bytes` performs a safe modular
                    // reduction from arbitrary-length bytes to a field
                    // element. The result is non-zero with overwhelming
                    // probability for any non-degenerate (subject, consumer)
                    // pair (BLAKE3 outputs look uniformly random).
                    let hash_bytes = pub_input_hash.as_bytes();
                    let pub_input_fr = ark_bn254::Fr::from_random_bytes(hash_bytes)
                        .expect("non-zero BLAKE3 hash always reduces to a valid Fr element");
                    let public_inputs: Vec<ark_bn254::Fr> = vec![pub_input_fr];

                    // Defense-in-depth: the construction above guarantees
                    // a single non-empty public input. Explicitly reject
                    // empty inputs in case the derivation ever regresses.
                    if public_inputs.is_empty() {
                        tracing::error!(
                            subject = ?&subject[..4],
                            "ZK proof verification: public_inputs unexpectedly empty — rejecting"
                        );
                        return Err(ShardError::ValidationFailed(
                            "ZK proof verification failed: public inputs must not be empty".into(),
                        ));
                    }

                    match Groth16::<Bn254>::verify(&vk, &public_inputs, &proof) {
                        Ok(true) => {
                            tracing::info!(
                                subject = ?&subject[..4],
                                consumer = ?&consumer[..4],
                                "Real ZK verification: biological proof verified successfully"
                            );
                            Ok(())
                        }
                        Ok(false) => {
                            tracing::warn!(
                                subject = ?&subject[..4],
                                "Real ZK verification: biological proof is invalid"
                            );
                            Err(ShardError::ValidationFailed(
                                "ZK proof verification failed: proof is invalid".into(),
                            ))
                        }
                        Err(e) => {
                            tracing::warn!(
                                subject = ?&subject[..4],
                                error = %e,
                                "Real ZK verification: biological verification error"
                            );
                            Err(ShardError::ValidationFailed(format!(
                                "ZK proof verification failed: {e}"
                            )))
                        }
                    }
                }

                // When real_verification is disabled, always reject ZK proofs
                #[cfg(not(feature = "real_verification"))]
                {
                    let _ = zk_proof; // suppress unused warning
                    Err(ShardError::ValidationFailed(
                        "ZK proof verification requires 'real_verification' feature to be enabled".into(),
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
                Some(&subject),
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
            None,
        );
        assert!(result.is_err(), "ZK proof should be rejected without real_verification");
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => {
                assert!(
                    msg.contains("real_verification")
                        || msg.contains("expected layout")
                        || msg.contains("too short")
                        || msg.contains("layout invalid"),
                    "expected real_verification, layout, or too-short error, got: {msg}"
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
                Some(&subject),
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
            None,
        );
        assert!(result.is_err(), "empty ZK proof should be rejected");
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => {
                assert!(
                    msg.contains("real_verification")
                        || msg.contains("expected layout")
                        || msg.contains("too short")
                        || msg.contains("layout invalid"),
                    "expected real_verification, layout, or too-short error, got: {msg}"
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
                Some(&subject),
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
            None,
        );
        assert!(result.is_err(), "ZK proof should be rejected without real_verification");
    }
}

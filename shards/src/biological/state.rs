//! Biological shard state
//!
//! Maintains the consent registry and data vault references. Consent is
//! modeled as a set of granted access records that can be revoked.

use std::collections::BTreeMap;

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
    pub consent_registry: BTreeMap<ConsentKey, ConsentRecord>,
}

impl BiologicalState {
    /// Create an empty biological state.
    pub fn new() -> Self {
        Self {
            consent_registry: BTreeMap::new(),
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

                // P0-7 fix: SystemTime::now() breaks consensus determinism — two honest
                // nodes processing the same event at different wall-clock times may reach
                // different accept/reject decisions. The consent expiry check is deferred
                // to the validator layer which should use consensus round numbers, not
                // wall-clock time. See biological/validator.rs.
                // TODO: Thread current_time from the event timestamp or consensus round
                // through the apply pipeline. For now, skip the expiry check in apply()
                // and rely on the validator's check (which also needs fixing).
                //
                // The previous block read:
                //     if record.expires_at != 0 {
                //         let now = std::time::SystemTime::now()...
                //         if record.expires_at <= now { return Err(...); }
                //     }
                // and was removed because wall-clock time is a non-deterministic
                // input that violates consensus safety invariants.

                // -----------------------------------------------------------------------
                // Real ZK proof verification using ark-groth16.
                // Enabled via the `real_verification` feature flag.
                // -----------------------------------------------------------------------
                #[cfg(feature = "real_verification")]
                {
                    // AUDIT-2026-07 C9 (#347): the verifying key comes from
                    // the node's VK registry, never from the caller. The
                    // submission is `[32-byte circuit ID || proof]`; an
                    // unregistered circuit ID is rejected outright. The
                    // single public input binds the proof to this exact
                    // (subject, consumer) consent record, so a proof for
                    // one pair cannot be replayed against another.
                    let public_inputs = vec![crate::zk::groth16::biological_public_input(subject, consumer)];
                    match crate::zk::groth16::verify_with_registry(zk_proof, &public_inputs, "biological query") {
                        Ok(()) => {
                            tracing::info!(
                                subject = ?&subject[..4],
                                consumer = ?&consumer[..4],
                                "Real ZK verification: biological proof verified successfully"
                            );
                            Ok(())
                        }
                        Err(e) => {
                            tracing::warn!(
                                subject = ?&subject[..4],
                                error = %e,
                                "Real ZK verification: biological proof rejected"
                            );
                            Err(e)
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
            .expect("test assertion failed");

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
        match result.expect_err("test assertion failed") {
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
            .expect("test assertion failed");

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
        match result.expect_err("test assertion failed") {
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
            .expect("test assertion failed");

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

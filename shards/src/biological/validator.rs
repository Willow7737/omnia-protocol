//! Biological shard validator
//!
//! Pre-flight validation for biological operations, including ZK-proof
//! validation stubs.

use super::ops::BiologicalOp;
use super::state::BiologicalState;
use crate::payload::ShardOp;
use crate::shard::ShardError;

/// Validator for the Biological shard.
pub struct BiologicalValidator;

impl BiologicalValidator {
    /// Validate a biological operation against the given state.
    pub fn validate(state: &BiologicalState, op: &BiologicalOp) -> Result<(), ShardError> {
        match op {
            BiologicalOp::GrantAccess {
                subject,
                consumer,
                scope,
                ..
            } => {
                if scope.is_empty() {
                    return Err(ShardError::InvalidOperation("Scope cannot be empty".into()));
                }
                let _ = (subject, consumer);
                Ok(())
            }
            BiologicalOp::RevokeAccess { subject, consumer } => {
                let key = (*subject, *consumer);
                if !state.consent_registry.contains_key(&key) {
                    return Err(ShardError::ValidationFailed("Consent record not found".into()));
                }
                Ok(())
            }
            BiologicalOp::QueryWithZkProof { subject, consumer, .. } => {
                let key = (*subject, *consumer);
                match state.consent_registry.get(&key) {
                    Some(record) if !record.revoked => {
                        // P0-7 fix: consent expiry must NOT use wall-clock time here.
                        // `SystemTime::now()` is a non-deterministic input that breaks
                        // consensus safety — two honest nodes processing the same
                        // event at different wall-clock times may reach different
                        // accept/reject decisions, leading to forked state.
                        //
                        // TODO(consensus-time): thread a deterministic `current_time`
                        // (derived from the consensus round number or the carrying
                        // event's timestamp) through the validator pipeline and use
                        // it here instead of `SystemTime::now()`. The state.rs apply
                        // path has been updated to skip the wall-clock expiry check
                        // entirely (see P0-7 fix in biological/state.rs); once a
                        // deterministic time source is available, both this check
                        // and the apply() path should use it.
                        //
                        // For now, we deliberately do NOT check `expires_at` against
                        // wall-clock time. Expiry is effectively deferred until a
                        // deterministic time source is wired through.
                        let _ = record.expires_at;
                        Ok(())
                    }
                    Some(_) => Err(ShardError::ValidationFailed("Consent has been revoked".into())),
                    None => Err(ShardError::ValidationFailed("No consent for this query".into())),
                }
            }
        }
    }

    /// Validate a `ShardOp::Biological` variant.
    pub fn validate_shard_op(state: &BiologicalState, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Biological(bio_op) => Self::validate(state, bio_op),
            _ => Err(ShardError::InvalidOperation("Not a Biological operation".into())),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::ops::{ConsumerId, SubjectId};
    use super::*;
    use crate::payload::ShardOp;

    fn subject() -> SubjectId {
        [0xAA; 32]
    }
    fn consumer() -> ConsumerId {
        [0xBB; 32]
    }

    #[test]
    fn test_grant_access_empty_scope_rejected() {
        let state = BiologicalState::new();
        let op = BiologicalOp::GrantAccess {
            subject: subject(),
            consumer: consumer(),
            scope: String::new(),
            expires_at: 0,
        };
        let result = BiologicalValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.expect_err("test assertion failed") {
            ShardError::InvalidOperation(msg) => assert!(msg.contains("Scope")),
            other => panic!("Expected InvalidOperation, got {other:?}"),
        }
    }

    #[test]
    fn test_grant_access_valid_scope_accepted() {
        let state = BiologicalState::new();
        let op = BiologicalOp::GrantAccess {
            subject: subject(),
            consumer: consumer(),
            scope: "lab-results".into(),
            expires_at: 0,
        };
        assert!(BiologicalValidator::validate(&state, &op).is_ok());
    }

    #[test]
    fn test_revoke_access_no_consent_rejected() {
        let state = BiologicalState::new();
        let op = BiologicalOp::RevokeAccess {
            subject: subject(),
            consumer: consumer(),
        };
        let result = BiologicalValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.expect_err("test assertion failed") {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("Consent record not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_query_with_zk_proof_no_consent_rejected() {
        let state = BiologicalState::new();
        let op = BiologicalOp::QueryWithZkProof {
            subject: subject(),
            consumer: consumer(),
            zk_proof: vec![0xAA],
            query: "count".into(),
        };
        let result = BiologicalValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.expect_err("test assertion failed") {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("No consent")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_shard_op_wrong_variant_rejected() {
        let state = BiologicalState::new();
        let op = ShardOp::Financial(crate::FinancialOp::BalanceQuery { account: [0u8; 32] });
        let result = BiologicalValidator::validate_shard_op(&state, &op);
        assert!(result.is_err());
        match result.expect_err("test assertion failed") {
            ShardError::InvalidOperation(msg) => assert!(msg.contains("Not a Biological")),
            other => panic!("Expected InvalidOperation, got {other:?}"),
        }
    }
}

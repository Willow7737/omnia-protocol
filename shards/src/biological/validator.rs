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
                        // Check consent expiry: reject if the consent has expired
                        if record.expires_at != 0 {
                            // TODO: Use a proper time provider instead of a hardcoded epoch.
                            // For now, use a simple heuristic — the caller should supply
                            // the current time via the validator context. We use 0 as a
                            // placeholder so that expiry checks are structurally correct.
                            let now: u64 = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            if record.expires_at <= now {
                                return Err(ShardError::ValidationFailed("Consent has expired".into()));
                            }
                        }
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
        match result.unwrap_err() {
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
        match result.unwrap_err() {
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
        match result.unwrap_err() {
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
        match result.unwrap_err() {
            ShardError::InvalidOperation(msg) => assert!(msg.contains("Not a Biological")),
            other => panic!("Expected InvalidOperation, got {other:?}"),
        }
    }
}

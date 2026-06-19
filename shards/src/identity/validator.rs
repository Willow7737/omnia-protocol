//! Identity shard validator
//!
//! Pre-flight validation for DID operations. Checks whether an operation
//! would succeed without actually mutating state.

use super::ops::IdentityOp;
use super::state::IdentityState;
use crate::payload::ShardOp;
use crate::shard::ShardError;

/// Validator for the Identity shard.
pub struct IdentityValidator;

impl IdentityValidator {
    /// Validate an identity operation against the given state.
    pub fn validate(state: &IdentityState, op: &IdentityOp) -> Result<(), ShardError> {
        match op {
            IdentityOp::CreateDid { document } => {
                if document.id.is_empty() {
                    return Err(ShardError::InvalidOperation("DID cannot be empty".into()));
                }
                if state.dids.contains_key(&document.id) {
                    return Err(ShardError::StateConflict(format!(
                        "DID already exists: {}",
                        document.id
                    )));
                }
                Ok(())
            }
            IdentityOp::UpdateDid { did, .. } => {
                if !state.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!("DID not found: {did}")));
                }
                Ok(())
            }
            IdentityOp::RecoverDid { did, shares } => {
                if let Some(config) = state.recovery_registry.get(did) {
                    if shares.len() < config.threshold as usize {
                        return Err(ShardError::ValidationFailed("Insufficient recovery shares".into()));
                    }
                } else {
                    return Err(ShardError::ValidationFailed(format!(
                        "No recovery config for DID: {did}"
                    )));
                }
                Ok(())
            }
            IdentityOp::VerifyDid { did } => {
                if !state.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!("DID not found: {did}")));
                }
                Ok(())
            }
            IdentityOp::AddAgent { did, agent } => {
                if !state.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!("Owner DID not found: {did}")));
                }
                if state.agent_registry.contains_key(&agent.did) {
                    return Err(ShardError::StateConflict(format!(
                        "Agent already exists: {}",
                        agent.did
                    )));
                }
                Ok(())
            }
            IdentityOp::EnrollBiometric { did, .. } => {
                if !state.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!("DID not found: {did}")));
                }
                Ok(())
            }
            IdentityOp::VerifyBiometric { did, .. } => {
                if !state.biometric_registry.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!(
                        "No biometric enrolled for DID: {did}"
                    )));
                }
                Ok(())
            }
            IdentityOp::RevokeAgent { agent_did } => {
                if !state.agent_registry.contains_key(agent_did) {
                    return Err(ShardError::ValidationFailed(format!("Agent not found: {agent_did}")));
                }
                Ok(())
            }
            IdentityOp::ConfigureRecovery {
                did,
                threshold,
                total_shares,
                ..
            } => {
                if !state.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!("DID not found: {did}")));
                }
                if *threshold < 2 {
                    return Err(ShardError::InvalidOperation(
                        "Recovery threshold must be at least 2".into(),
                    ));
                }
                if *threshold > *total_shares {
                    return Err(ShardError::InvalidOperation(
                        "Recovery threshold cannot exceed total shares".into(),
                    ));
                }
                Ok(())
            }
        }
    }

    /// Validate a `ShardOp::Identity` variant.
    pub fn validate_shard_op(state: &IdentityState, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Identity(id_op) => Self::validate(state, id_op),
            _ => Err(ShardError::InvalidOperation("Not an Identity operation".into())),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::agent::AgentIdentity;
    use super::super::recovery::RecoveryShare;
    use super::super::state::RecoveryConfig;
    use super::*;
    use crate::payload::ShardOp;
    use crate::Did;
    use omnia_substrate::VectorClock;

    fn test_did() -> Did {
        "did:omnia:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()
    }

    fn test_agent_did() -> Did {
        "did:omnia:agent:abc123".to_string()
    }

    fn make_doc() -> super::super::state::DidDocument {
        super::super::state::DidDocument::new(test_did(), [0x42; 32], 1_700_000_000_000)
    }

    fn make_agent() -> AgentIdentity {
        AgentIdentity {
            did: test_agent_did(),
            owner_did: test_did(),
            capabilities: vec![],
            created_at: VectorClock::new(),
            expires_at: None,
            revoked: false,
        }
    }

    // ── CreateDid ─────────────────────────────────────────────────────

    #[test]
    fn test_create_did_empty_id_rejected() {
        let state = IdentityState::new();
        let doc = super::super::state::DidDocument::new(String::new(), [0x42; 32], 0);
        let op = IdentityOp::CreateDid { document: doc };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::InvalidOperation(msg) => assert!(msg.contains("DID cannot be empty")),
            other => panic!("Expected InvalidOperation, got {other:?}"),
        }
    }

    #[test]
    fn test_create_did_new_accepted() {
        let state = IdentityState::new();
        let op = IdentityOp::CreateDid { document: make_doc() };
        assert!(IdentityValidator::validate(&state, &op).is_ok());
    }

    #[test]
    fn test_create_did_duplicate_rejected() {
        let mut state = IdentityState::new();
        state.dids.insert(test_did(), make_doc());
        let op = IdentityOp::CreateDid { document: make_doc() };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::StateConflict(msg) => assert!(msg.contains("DID already exists")),
            other => panic!("Expected StateConflict, got {other:?}"),
        }
    }

    // ── UpdateDid ─────────────────────────────────────────────────────

    #[test]
    fn test_update_did_not_found_rejected() {
        let state = IdentityState::new();
        let op = IdentityOp::UpdateDid {
            did: test_did(),
            updates: vec![],
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("DID not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_update_did_exists_accepted() {
        let mut state = IdentityState::new();
        state.dids.insert(test_did(), make_doc());
        let op = IdentityOp::UpdateDid {
            did: test_did(),
            updates: vec![],
        };
        assert!(IdentityValidator::validate(&state, &op).is_ok());
    }

    // ── RecoverDid ────────────────────────────────────────────────────

    #[test]
    fn test_recover_did_no_config_rejected() {
        let state = IdentityState::new();
        let op = IdentityOp::RecoverDid {
            did: test_did(),
            shares: vec![],
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("No recovery config")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_recover_did_insufficient_shares_rejected() {
        let mut state = IdentityState::new();
        state.recovery_registry.insert(
            test_did(),
            RecoveryConfig {
                threshold: 3,
                total_shares: 5,
            },
        );
        let op = IdentityOp::RecoverDid {
            did: test_did(),
            shares: vec![
                RecoveryShare {
                    index: 1,
                    value: vec![0xAA],
                },
                RecoveryShare {
                    index: 2,
                    value: vec![0xBB],
                },
            ],
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("Insufficient recovery shares")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_recover_did_sufficient_shares_accepted() {
        let mut state = IdentityState::new();
        state.recovery_registry.insert(
            test_did(),
            RecoveryConfig {
                threshold: 2,
                total_shares: 3,
            },
        );
        let op = IdentityOp::RecoverDid {
            did: test_did(),
            shares: vec![
                RecoveryShare {
                    index: 1,
                    value: vec![0xAA],
                },
                RecoveryShare {
                    index: 2,
                    value: vec![0xBB],
                },
            ],
        };
        assert!(IdentityValidator::validate(&state, &op).is_ok());
    }

    // ── VerifyDid ─────────────────────────────────────────────────────

    #[test]
    fn test_verify_did_not_found_rejected() {
        let state = IdentityState::new();
        let op = IdentityOp::VerifyDid { did: test_did() };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("DID not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_verify_did_exists_accepted() {
        let mut state = IdentityState::new();
        state.dids.insert(test_did(), make_doc());
        let op = IdentityOp::VerifyDid { did: test_did() };
        assert!(IdentityValidator::validate(&state, &op).is_ok());
    }

    // ── AddAgent ──────────────────────────────────────────────────────

    #[test]
    fn test_add_agent_owner_not_found_rejected() {
        let state = IdentityState::new();
        let op = IdentityOp::AddAgent {
            did: test_did(),
            agent: make_agent(),
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("Owner DID not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_add_agent_duplicate_agent_rejected() {
        let mut state = IdentityState::new();
        state.dids.insert(test_did(), make_doc());
        state.agent_registry.insert(test_agent_did(), make_agent());
        let op = IdentityOp::AddAgent {
            did: test_did(),
            agent: make_agent(),
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::StateConflict(msg) => assert!(msg.contains("Agent already exists")),
            other => panic!("Expected StateConflict, got {other:?}"),
        }
    }

    #[test]
    fn test_add_agent_valid_accepted() {
        let mut state = IdentityState::new();
        state.dids.insert(test_did(), make_doc());
        let op = IdentityOp::AddAgent {
            did: test_did(),
            agent: make_agent(),
        };
        assert!(IdentityValidator::validate(&state, &op).is_ok());
    }

    // ── EnrollBiometric ───────────────────────────────────────────────

    #[test]
    fn test_enroll_biometric_did_not_found_rejected() {
        let state = IdentityState::new();
        let op = IdentityOp::EnrollBiometric {
            did: test_did(),
            template: vec![0xAA; 32],
            algorithm: "fingerprint_v2".into(),
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("DID not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_enroll_biometric_valid_accepted() {
        let mut state = IdentityState::new();
        state.dids.insert(test_did(), make_doc());
        let op = IdentityOp::EnrollBiometric {
            did: test_did(),
            template: vec![0xAA; 32],
            algorithm: "fingerprint_v2".into(),
        };
        assert!(IdentityValidator::validate(&state, &op).is_ok());
    }

    // ── VerifyBiometric ───────────────────────────────────────────────

    #[test]
    fn test_verify_biometric_not_enrolled_rejected() {
        let mut state = IdentityState::new();
        state.dids.insert(test_did(), make_doc());
        let op = IdentityOp::VerifyBiometric {
            did: test_did(),
            template: vec![0xAA; 32],
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("No biometric enrolled")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    // ── RevokeAgent ───────────────────────────────────────────────────

    #[test]
    fn test_revoke_agent_not_found_rejected() {
        let state = IdentityState::new();
        let op = IdentityOp::RevokeAgent {
            agent_did: test_agent_did(),
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("Agent not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    // ── ConfigureRecovery ─────────────────────────────────────────────

    #[test]
    fn test_configure_recovery_did_not_found_rejected() {
        let state = IdentityState::new();
        let op = IdentityOp::ConfigureRecovery {
            did: test_did(),
            secret: vec![0xAA; 32],
            threshold: 3,
            total_shares: 5,
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("DID not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_configure_recovery_threshold_below_2_rejected() {
        let mut state = IdentityState::new();
        state.dids.insert(test_did(), make_doc());
        let op = IdentityOp::ConfigureRecovery {
            did: test_did(),
            secret: vec![0xAA; 32],
            threshold: 1,
            total_shares: 5,
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::InvalidOperation(msg) => assert!(msg.contains("threshold must be at least 2")),
            other => panic!("Expected InvalidOperation, got {other:?}"),
        }
    }

    #[test]
    fn test_configure_recovery_threshold_exceeds_total_rejected() {
        let mut state = IdentityState::new();
        state.dids.insert(test_did(), make_doc());
        let op = IdentityOp::ConfigureRecovery {
            did: test_did(),
            secret: vec![0xAA; 32],
            threshold: 5,
            total_shares: 3,
        };
        let result = IdentityValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::InvalidOperation(msg) => assert!(msg.contains("threshold cannot exceed total")),
            other => panic!("Expected InvalidOperation, got {other:?}"),
        }
    }

    #[test]
    fn test_configure_recovery_valid_accepted() {
        let mut state = IdentityState::new();
        state.dids.insert(test_did(), make_doc());
        let op = IdentityOp::ConfigureRecovery {
            did: test_did(),
            secret: vec![0xAA; 32],
            threshold: 3,
            total_shares: 5,
        };
        assert!(IdentityValidator::validate(&state, &op).is_ok());
    }

    // ── validate_shard_op ─────────────────────────────────────────────

    #[test]
    fn test_validate_shard_op_wrong_variant_rejected() {
        let state = IdentityState::new();
        let op = ShardOp::Financial(crate::FinancialOp::BalanceQuery { account: [0u8; 32] });
        let result = IdentityValidator::validate_shard_op(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::InvalidOperation(msg) => assert!(msg.contains("Not an Identity")),
            other => panic!("Expected InvalidOperation, got {other:?}"),
        }
    }
}

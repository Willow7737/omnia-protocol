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

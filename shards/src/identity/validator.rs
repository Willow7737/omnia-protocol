//! Identity shard validator
//!
//! Pre-flight validation for DID operations. Checks whether an operation
//! would succeed without actually mutating state.

use crate::payload::ShardOp;
use crate::shard::ShardError;
use super::ops::IdentityOp;
use super::state::IdentityState;

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
                    return Err(ShardError::ValidationFailed(format!(
                        "DID not found: {}",
                        did
                    )));
                }
                Ok(())
            }
            IdentityOp::RecoverDid { did, recovery_shares } => {
                let config = state.recovery_registry.get(did);
                if let Some(config) = config {
                    if (recovery_shares.len() as u8) < config.threshold {
                        return Err(ShardError::ValidationFailed(
                            "Insufficient recovery shares".into(),
                        ));
                    }
                }
                // If no recovery config exists, the operation will fail at apply time
                Ok(())
            }
            IdentityOp::VerifyDid { did } => {
                if !state.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!(
                        "DID not found: {}",
                        did
                    )));
                }
                Ok(())
            }
            IdentityOp::AddAgent { did, .. } => {
                if !state.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!(
                        "DID not found: {}",
                        did
                    )));
                }
                Ok(())
            }
        }
    }

    /// Validate a `ShardOp::Identity` variant.
    pub fn validate_shard_op(state: &IdentityState, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Identity(id_op) => Self::validate(state, id_op),
            _ => Err(ShardError::InvalidOperation(
                "Not an Identity operation".into(),
            )),
        }
    }
}

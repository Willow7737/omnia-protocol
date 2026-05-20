//! Physical shard validator
//!
//! Pre-flight validation for physical asset operations, especially
//! ownership verification for transfers.

use super::ops::PhysicalOp;
use super::state::PhysicalState;
use crate::payload::ShardOp;
use crate::shard::ShardError;

/// Validator for the Physical shard.
pub struct PhysicalValidator;

impl PhysicalValidator {
    /// Validate a physical operation against the given state.
    pub fn validate(state: &PhysicalState, op: &PhysicalOp) -> Result<(), ShardError> {
        match op {
            PhysicalOp::AnchorItem { item_id, .. } => {
                if state.provenance.contains_key(item_id) {
                    return Err(ShardError::StateConflict(format!("Item already anchored: {item_id:?}")));
                }
                Ok(())
            }
            PhysicalOp::TransferOwnership { item_id, .. } => {
                if !state.provenance.contains_key(item_id) {
                    return Err(ShardError::ValidationFailed("Item not found".into()));
                }
                Ok(())
            }
            PhysicalOp::VerifyChain { item_id } => {
                if !state.provenance.contains_key(item_id) {
                    return Err(ShardError::ValidationFailed("Item not found".into()));
                }
                Ok(())
            }
        }
    }

    /// Validate a `ShardOp::Physical` variant.
    pub fn validate_shard_op(state: &PhysicalState, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Physical(phys_op) => Self::validate(state, phys_op),
            _ => Err(ShardError::InvalidOperation("Not a Physical operation".into())),
        }
    }
}

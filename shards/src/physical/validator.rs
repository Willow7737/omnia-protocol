//! Physical shard validator
//!
//! Pre-flight validation for physical asset operations, including
//! ownership verification for transfers (defense-in-depth).

use super::ops::{OwnerId, PhysicalOp};
use super::state::PhysicalState;
use crate::payload::ShardOp;
use crate::shard::ShardError;

/// Validator for the Physical shard.
pub struct PhysicalValidator;

impl PhysicalValidator {
    /// Validate a physical operation against the given state.
    ///
    /// Performs structural checks (item existence, no duplicate anchors).
    /// For ownership pre-flight checks, use [`Self::validate_with_caller`] instead.
    pub fn validate(state: &PhysicalState, op: &PhysicalOp) -> Result<(), ShardError> {
        Self::validate_with_caller(state, op, None)
    }

    /// Validate a physical operation with optional caller identity.
    ///
    /// Performs structural checks and, when `caller` is provided,
    /// authorization checks (ownership for transfers). This provides
    /// defense-in-depth alongside the apply-time owner check in
    /// [`PhysicalState::apply`].
    ///
    /// # Arguments
    ///
    /// * `state` — Current physical shard state
    /// * `op` — The operation to validate
    /// * `caller` — Optional identity of the caller (typically `event.creator_pubkey`).
    ///   When `Some`, the validator additionally checks that the caller is the current
    ///   owner for `TransferOwnership` operations.
    pub fn validate_with_caller(
        state: &PhysicalState,
        op: &PhysicalOp,
        caller: Option<OwnerId>,
    ) -> Result<(), ShardError> {
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
                // Defense-in-depth: if the caller identity is available, verify
                // they are the current owner. The apply-time check in PhysicalState::apply
                // enforces this unconditionally, but catching it early provides
                // better error messages and prevents invalid ops from entering the
                // consensus pipeline.
                if let Some(caller_id) = caller {
                    if let Some(current_owner) = state.current_owner(item_id) {
                        if current_owner != caller_id {
                            return Err(ShardError::ValidationFailed(
                                "TransferOwnership pre-flight check: caller is not the current owner".into(),
                            ));
                        }
                    }
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

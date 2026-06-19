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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::ops::{ItemId, OwnerId};
    use super::*;
    use crate::payload::ShardOp;

    fn item_id() -> ItemId {
        [0xDD; 32]
    }
    fn owner() -> OwnerId {
        [0xEE; 32]
    }

    #[test]
    fn test_anchor_item_new_accepted() {
        let state = PhysicalState::new();
        let op = PhysicalOp::AnchorItem {
            item_id: item_id(),
            owner: owner(),
            metadata: vec![1, 2, 3],
        };
        assert!(PhysicalValidator::validate(&state, &op).is_ok());
    }

    #[test]
    fn test_anchor_item_duplicate_rejected() {
        let mut state = PhysicalState::new();
        // Simulate an existing anchored item
        state.provenance.insert(
            item_id(),
            vec![super::super::state::ProvenanceEvent {
                event_type: "anchor".into(),
                owner: owner(),
                clock: omnia_substrate::VectorClock::new(),
                metadata: vec![],
            }],
        );
        let op = PhysicalOp::AnchorItem {
            item_id: item_id(),
            owner: owner(),
            metadata: vec![1, 2, 3],
        };
        let result = PhysicalValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::StateConflict(msg) => assert!(msg.contains("Item already anchored")),
            other => panic!("Expected StateConflict, got {other:?}"),
        }
    }

    #[test]
    fn test_transfer_ownership_item_not_found_rejected() {
        let state = PhysicalState::new();
        let op = PhysicalOp::TransferOwnership {
            item_id: item_id(),
            new_owner: [0xFF; 32],
        };
        let result = PhysicalValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("Item not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_verify_chain_item_not_found_rejected() {
        let state = PhysicalState::new();
        let op = PhysicalOp::VerifyChain { item_id: item_id() };
        let result = PhysicalValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("Item not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_shard_op_wrong_variant_rejected() {
        let state = PhysicalState::new();
        let op = ShardOp::Financial(crate::FinancialOp::BalanceQuery { account: [0u8; 32] });
        let result = PhysicalValidator::validate_shard_op(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::InvalidOperation(msg) => assert!(msg.contains("Not a Physical")),
            other => panic!("Expected InvalidOperation, got {other:?}"),
        }
    }
}

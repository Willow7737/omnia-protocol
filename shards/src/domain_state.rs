//! Domain state trait for shard state machines
//!
//! The [`ShardState`] trait normalizes the `apply` method signature across
//! all shard state types. Each state type has a different `apply` method
//! (some take `&Event`, some take `&VectorClock`, some take just the op),
//! but the macro-generated `Shard` impl needs a uniform call site.
//!
//! This trait provides [`ShardState::apply_op`] which always takes
//! `(&Op, &Event)` and lets each implementation extract what it needs
//! from the `Event`.

use omnia_economics::EconomicsState;
use omnia_substrate::Event;

use crate::biological::state::BiologicalState;
use crate::computational::state::ComputationalState;
use crate::financial::state::FinancialState;
use crate::identity::state::IdentityState;
use crate::physical::state::PhysicalState;
use crate::shard::ShardError;

/// Trait that normalizes the `apply` call across all shard state types.
///
/// Each shard state type implements this trait to adapt its domain-specific
/// `apply` method to the uniform signature `(op, event) -> Result`,
/// which is required by the `impl_shard!` macro.
pub trait ShardState: Send + Sync {
    /// The operation type this state processes.
    type Op;

    /// Apply an operation with event context, mutating state.
    ///
    /// The `event` parameter provides context (creator, vector clock,
    /// signature) that some state types need (e.g., `FinancialState`
    /// uses `event.creator_pubkey` for transfer senders).
    fn apply_op(&mut self, op: &Self::Op, event: &Event) -> Result<(), ShardError>;

    /// Serialize the current state to bytes for snapshots.
    fn snapshot(&self) -> Result<Vec<u8>, ShardError>;
}

// ---------------------------------------------------------------------------
// Implementations for each shard state type
// ---------------------------------------------------------------------------

impl ShardState for FinancialState {
    type Op = crate::financial::ops::FinancialOp;

    fn apply_op(&mut self, op: &Self::Op, event: &Event) -> Result<(), ShardError> {
        self.apply(op, event)
    }

    fn snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
    }
}

impl ShardState for IdentityState {
    type Op = crate::identity::ops::IdentityOp;

    fn apply_op(&mut self, op: &Self::Op, event: &Event) -> Result<(), ShardError> {
        self.apply(op, &event.vector_clock, Some(&event.creator_pubkey))
    }

    fn snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
    }
}

impl ShardState for ComputationalState {
    type Op = crate::computational::ops::ComputationalOp;

    fn apply_op(&mut self, op: &Self::Op, event: &Event) -> Result<(), ShardError> {
        self.apply(op, &event.vector_clock)
    }

    fn snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
    }
}

impl ShardState for PhysicalState {
    type Op = crate::physical::ops::PhysicalOp;

    fn apply_op(&mut self, op: &Self::Op, event: &Event) -> Result<(), ShardError> {
        // Pass the event creator as the authorization identity for ownership checks.
        // The `apply` method will verify that only the current owner can transfer.
        let event_creator = Some(event.creator_pubkey);
        self.apply(op, &event.vector_clock, event_creator)
    }

    fn snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
    }
}

impl ShardState for BiologicalState {
    type Op = crate::biological::ops::BiologicalOp;

    fn apply_op(&mut self, op: &Self::Op, event: &Event) -> Result<(), ShardError> {
        self.apply(op, &event.vector_clock, Some(&event.creator_pubkey))
    }

    fn snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
    }
}

impl ShardState for EconomicsState {
    type Op = omnia_economics::EconomicsOp;

    fn apply_op(&mut self, op: &Self::Op, event: &Event) -> Result<(), ShardError> {
        // EconomicsState::apply takes (op, current_epoch, event_creator).
        // We use the state's tracked epoch and pass the event creator for admin gating.
        self.apply(op, self.current_epoch(), Some(&event.creator_pubkey))
            .map_err(|e| ShardError::ValidationFailed(e.to_string()))
    }

    fn snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_substrate::{crypto::generate_keypair, Event, NodeId, VectorClock};

    fn test_node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    /// Create a signed Event for testing shard state apply_op calls.
    fn make_event() -> Event {
        let keypair = generate_keypair();
        let creator = test_node(1);
        let mut event = Event::genesis(creator, vec![]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");
        event
    }

    #[test]
    fn test_financial_state_apply_op_balance_query() {
        use crate::financial::ops::FinancialOp;
        let mut state = FinancialState::new();
        let event = make_event();
        let op = FinancialOp::BalanceQuery { account: test_node(1) };
        assert!(state.apply_op(&op, &event).is_ok());
    }

    #[test]
    fn test_financial_state_snapshot() {
        let state = FinancialState::new();
        let bytes = state.snapshot();
        assert!(bytes.is_ok());
        assert!(!bytes.unwrap().is_empty());
    }

    #[test]
    fn test_identity_state_apply_op_create_did() {
        use crate::identity::ops::IdentityOp;
        use crate::identity::state::DidDocument;
        let mut state = IdentityState::new();
        let event = make_event();
        let op = IdentityOp::CreateDid {
            document: DidDocument::new("did:omnia:test".to_string(), [0x42; 32], 0),
        };
        assert!(state.apply_op(&op, &event).is_ok());
    }

    #[test]
    fn test_identity_state_snapshot() {
        let state = IdentityState::new();
        let bytes = state.snapshot();
        assert!(bytes.is_ok());
        assert!(!bytes.unwrap().is_empty());
    }

    #[test]
    fn test_computational_state_apply_op_submit_task() {
        use crate::computational::ops::ComputationalOp;
        let mut state = ComputationalState::new();
        let event = make_event();
        let op = ComputationalOp::SubmitTask {
            task_id: [0xAA; 32],
            spec: vec![1, 2, 3],
            reward: 100,
        };
        assert!(state.apply_op(&op, &event).is_ok());
    }

    #[test]
    fn test_computational_state_snapshot() {
        let state = ComputationalState::new();
        let bytes = state.snapshot();
        assert!(bytes.is_ok());
        assert!(!bytes.unwrap().is_empty());
    }

    #[test]
    fn test_physical_state_apply_op_anchor_item() {
        use crate::physical::ops::PhysicalOp;
        let mut state = PhysicalState::new();
        let event = make_event();
        let op = PhysicalOp::AnchorItem {
            item_id: [0xBB; 32],
            owner: [0xCC; 32],
            metadata: vec![],
        };
        assert!(state.apply_op(&op, &event).is_ok());
    }

    #[test]
    fn test_physical_state_snapshot() {
        let state = PhysicalState::new();
        let bytes = state.snapshot();
        assert!(bytes.is_ok());
        assert!(!bytes.unwrap().is_empty());
    }

    #[test]
    fn test_biological_state_apply_op_grant_access() {
        use crate::biological::ops::BiologicalOp;
        let mut state = BiologicalState::new();
        let event = make_event();
        let op = BiologicalOp::GrantAccess {
            subject: [0xDD; 32],
            consumer: [0xEE; 32],
            scope: "lab-results".to_string(),
            expires_at: 0,
        };
        assert!(state.apply_op(&op, &event).is_ok());
    }

    #[test]
    fn test_biological_state_snapshot() {
        let state = BiologicalState::new();
        let bytes = state.snapshot();
        assert!(bytes.is_ok());
        assert!(!bytes.unwrap().is_empty());
    }

    #[test]
    fn test_economics_state_apply_op_register_did() {
        let mut state = EconomicsState::new();
        let event = make_event();
        let op = omnia_economics::EconomicsOp::RegisterDid {
            did: "did:omnia:test".to_string(),
        };
        assert!(state.apply_op(&op, &event).is_ok());
    }

    #[test]
    fn test_economics_state_snapshot() {
        let state = EconomicsState::new();
        let bytes = state.snapshot();
        assert!(bytes.is_ok());
        assert!(!bytes.unwrap().is_empty());
    }
}

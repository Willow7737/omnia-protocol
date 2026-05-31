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

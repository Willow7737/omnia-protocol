//! # Omnia Protocol — Layer 2: Domain Shards
//!
//! Domain shards are specialized state machines that build on top of the
//! Layer 1 substrate. Each shard type has its own:
//!
//! - **State machine** — defines what operations are valid and how they
//!   mutate state
//! - **Validation rules** — domain-specific constraints (e.g., sufficient
//!   balance for transfers)
//! - **CRDT semantics** — how state converges when events arrive in
//!   different orders
//!
//! But all shards share:
//! - The same **causal graph** for event ordering
//! - The same **consensus** for finality
//! - The same **gossip protocol** for propagation
//!
//! # The 5 Shard Types
//!
//! | Shard         | Domain                   | State Type                   |
//! |---------------|--------------------------|------------------------------|
//! | Financial     | Assets, value transfer   | Account balances (causal)    |
//! | Computational | AI training, compute     | Task queue, proof registry   |
//! | Physical      | Supply chain, real estate| Provenance log (append-only) |
//! | Biological    | Health records, bio      | Consent registry, ZK queries |
//! | Identity      | DIDs, social recovery    | DID document registry        |
//!
//! # Usage
//!
//! ```ignore
//! use omnia_shards::{ShardRouter, FinancialShard, IdentityShard};
//!
//! let mut router = ShardRouter::new();
//! router.register(Box::new(FinancialShard::new()));
//! router.register(Box::new(IdentityShard::new()));
//!
//! // Route an event to the appropriate shard
//! router.route_event(&event)?;
//! ```

#![warn(missing_docs)]

pub mod biological;
pub mod computational;
pub mod cross_shard;
pub mod financial;
pub mod identity;
pub mod physical;
pub mod payload;
pub mod router;
pub mod shard;

// Re-export core types
pub use cross_shard::CrossShardMessage;
pub use payload::{ShardOp, ShardPayload};
pub use router::ShardRouter;
pub use shard::{Shard, ShardError, ShardId};

// Re-export shard-specific types for convenience
pub use financial::{
    AccountBalance as FinancialAccountBalance, FinancialOp, FinancialState,
    FinancialValidator,
};
pub use identity::{
    AgentIdentity, Did, DidDocument, DidUpdate, IdentityOp, IdentityState,
    IdentityValidator, RecoveryConfig, RecoveryShare,
};
pub use computational::{ComputationalOp, ComputationalState, ComputationalValidator, TaskStatus};
pub use physical::{PhysicalOp, PhysicalState, PhysicalValidator, ProvenanceEvent};
pub use biological::{BiologicalOp, BiologicalState, BiologicalValidator, ConsentRecord};

use omnia_substrate::Event;

// ---------------------------------------------------------------------------
// Concrete shard implementations that implement the `Shard` trait
// ---------------------------------------------------------------------------

/// The Financial shard — handles asset transfers, minting, and burning.
///
/// Uses strict causal ordering (not CRDTs) for balance updates because
/// financial operations like transfers are not commutative.
pub struct FinancialShard {
    state: FinancialState,
}

impl FinancialShard {
    /// Create a new Financial shard with empty state.
    pub fn new() -> Self {
        Self {
            state: FinancialState::new(),
        }
    }

    /// Get a reference to the internal state.
    pub fn state(&self) -> &FinancialState {
        &self.state
    }
}

impl Default for FinancialShard {
    fn default() -> Self {
        Self::new()
    }
}

impl Shard for FinancialShard {
    fn shard_id(&self) -> ShardId {
        ShardId::financial()
    }

    fn process_event(&mut self, event: &Event, op: ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Financial(fin_op) => {
                FinancialValidator::validate(&self.state, &fin_op)?;
                self.state.apply(&fin_op, event)
            }
            _ => Err(ShardError::InvalidOperation(
                "Financial shard received non-Financial operation".into(),
            )),
        }
    }

    fn state_snapshot(&self) -> Vec<u8> {
        self.state.to_bytes()
    }

    fn validate(&self, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Financial(fin_op) => FinancialValidator::validate(&self.state, fin_op),
            _ => Err(ShardError::InvalidOperation(
                "Financial shard received non-Financial operation".into(),
            )),
        }
    }
}

/// The Identity shard — handles DID lifecycle and social recovery.
pub struct IdentityShard {
    state: IdentityState,
}

impl IdentityShard {
    /// Create a new Identity shard with empty state.
    pub fn new() -> Self {
        Self {
            state: IdentityState::new(),
        }
    }

    /// Get a reference to the internal state.
    pub fn state(&self) -> &IdentityState {
        &self.state
    }
}

impl Default for IdentityShard {
    fn default() -> Self {
        Self::new()
    }
}

impl Shard for IdentityShard {
    fn shard_id(&self) -> ShardId {
        ShardId::identity()
    }

    fn process_event(&mut self, event: &Event, op: ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Identity(id_op) => {
                IdentityValidator::validate(&self.state, &id_op)?;
                self.state.apply(&id_op, &event.vector_clock)
            }
            _ => Err(ShardError::InvalidOperation(
                "Identity shard received non-Identity operation".into(),
            )),
        }
    }

    fn state_snapshot(&self) -> Vec<u8> {
        self.state.to_bytes()
    }

    fn validate(&self, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Identity(id_op) => IdentityValidator::validate(&self.state, id_op),
            _ => Err(ShardError::InvalidOperation(
                "Identity shard received non-Identity operation".into(),
            )),
        }
    }
}

/// The Computational shard — handles AI training tasks and proof verification.
pub struct ComputationalShard {
    state: ComputationalState,
}

impl ComputationalShard {
    /// Create a new Computational shard with empty state.
    pub fn new() -> Self {
        Self {
            state: ComputationalState::new(),
        }
    }

    /// Get a reference to the internal state.
    pub fn state(&self) -> &ComputationalState {
        &self.state
    }
}

impl Default for ComputationalShard {
    fn default() -> Self {
        Self::new()
    }
}

impl Shard for ComputationalShard {
    fn shard_id(&self) -> ShardId {
        ShardId::computational()
    }

    fn process_event(&mut self, event: &Event, op: ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Computational(comp_op) => {
                ComputationalValidator::validate(&self.state, &comp_op)?;
                self.state.apply(&comp_op, &event.vector_clock)
            }
            _ => Err(ShardError::InvalidOperation(
                "Computational shard received non-Computational operation".into(),
            )),
        }
    }

    fn state_snapshot(&self) -> Vec<u8> {
        self.state.to_bytes()
    }

    fn validate(&self, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Computational(comp_op) => {
                ComputationalValidator::validate(&self.state, comp_op)
            }
            _ => Err(ShardError::InvalidOperation(
                "Computational shard received non-Computational operation".into(),
            )),
        }
    }
}

/// The Physical shard — handles supply chain and real-world asset provenance.
pub struct PhysicalShard {
    state: PhysicalState,
}

impl PhysicalShard {
    /// Create a new Physical shard with empty state.
    pub fn new() -> Self {
        Self {
            state: PhysicalState::new(),
        }
    }

    /// Get a reference to the internal state.
    pub fn state(&self) -> &PhysicalState {
        &self.state
    }
}

impl Default for PhysicalShard {
    fn default() -> Self {
        Self::new()
    }
}

impl Shard for PhysicalShard {
    fn shard_id(&self) -> ShardId {
        ShardId::physical()
    }

    fn process_event(&mut self, event: &Event, op: ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Physical(phys_op) => {
                PhysicalValidator::validate(&self.state, &phys_op)?;
                self.state.apply(&phys_op, &event.vector_clock)
            }
            _ => Err(ShardError::InvalidOperation(
                "Physical shard received non-Physical operation".into(),
            )),
        }
    }

    fn state_snapshot(&self) -> Vec<u8> {
        self.state.to_bytes()
    }

    fn validate(&self, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Physical(phys_op) => PhysicalValidator::validate(&self.state, phys_op),
            _ => Err(ShardError::InvalidOperation(
                "Physical shard received non-Physical operation".into(),
            )),
        }
    }
}

/// The Biological shard — handles health records and consent management.
pub struct BiologicalShard {
    state: BiologicalState,
}

impl BiologicalShard {
    /// Create a new Biological shard with empty state.
    pub fn new() -> Self {
        Self {
            state: BiologicalState::new(),
        }
    }

    /// Get a reference to the internal state.
    pub fn state(&self) -> &BiologicalState {
        &self.state
    }
}

impl Default for BiologicalShard {
    fn default() -> Self {
        Self::new()
    }
}

impl Shard for BiologicalShard {
    fn shard_id(&self) -> ShardId {
        ShardId::biological()
    }

    fn process_event(&mut self, event: &Event, op: ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Biological(bio_op) => {
                BiologicalValidator::validate(&self.state, &bio_op)?;
                self.state.apply(&bio_op, &event.vector_clock)
            }
            _ => Err(ShardError::InvalidOperation(
                "Biological shard received non-Biological operation".into(),
            )),
        }
    }

    fn state_snapshot(&self) -> Vec<u8> {
        self.state.to_bytes()
    }

    fn validate(&self, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Biological(bio_op) => BiologicalValidator::validate(&self.state, bio_op),
            _ => Err(ShardError::InvalidOperation(
                "Biological shard received non-Biological operation".into(),
            )),
        }
    }
}

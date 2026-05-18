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
//! use omnia_shards::{FeeSchedule, ShardRouter, FinancialShard, IdentityShard};
//! use omnia_economics::QuotaSystem;
//!
//! let mut router = ShardRouter::new(FeeSchedule::standard(), QuotaSystem::default_system());
//! router.register(Box::new(FinancialShard::new()));
//! router.register(Box::new(IdentityShard::new()));
//!
//! // Route an event to the appropriate shard
//! router.route_event(&event)?;
//! ```

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod biological;
pub mod computational;
pub mod cross_shard;
pub mod economics_shard;
pub mod fee_schedule;
pub mod financial;
pub mod identity;
pub mod nonce_store;
pub mod payload;
pub mod physical;
pub mod router;
pub mod shard;

// Re-export core types
pub use cross_shard::CrossShardMessage;
pub use economics_shard::{EconomicsOp, EconomicsVoteChoice};
pub use fee_schedule::FeeSchedule;
pub use nonce_store::{InMemoryNonceStore, NonceStore, NonceStoreError, RedbNonceStore};
pub use payload::{ShardOp, ShardPayload};
pub use router::ShardRouter;
pub use shard::{Shard, ShardError, ShardId};

// Re-export shard-specific types for convenience
pub use biological::{BiologicalOp, BiologicalState, BiologicalValidator, ConsentRecord};
pub use computational::{ComputationalOp, ComputationalState, ComputationalValidator, TaskStatus};
pub use financial::{
    AccountBalance as FinancialAccountBalance, FinancialOp, FinancialState, FinancialValidator,
};
pub use identity::{
    format_did, AgentCapability, AgentIdentity, BiometricAnchor, Did, DidDocument, DidError,
    DidUpdate, EncryptedShare, IdentityOp, IdentityState, IdentityValidator, RecoveryConfig,
    RecoveryShare, ShamirRecovery, DID_METHOD, DID_PREFIX,
};
pub use physical::{PhysicalOp, PhysicalState, PhysicalValidator, ProvenanceEvent};

use omnia_substrate::Event;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Economics shard implementation
// ---------------------------------------------------------------------------

/// Simplified internal state for the Economics shard.
///
/// Tracks UBC balances per DID and the current epoch. The full
/// economics logic (governance, useful-work verification) lives in
/// the `omnia-economics` crate; this state is sufficient for shard
/// routing and basic balance operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EconomicsShardState {
    /// UBC balances keyed by DID.
    balances: HashMap<String, u64>,
    /// Current epoch number.
    current_epoch: u64,
    /// Default UBC quota per DID per epoch.
    default_quota: u64,
}

impl EconomicsShardState {
    fn new() -> Self {
        Self {
            balances: HashMap::new(),
            current_epoch: 0,
            default_quota: 1000,
        }
    }

    fn apply(&mut self, op: &EconomicsOp) -> Result<(), ShardError> {
        match op {
            EconomicsOp::MintUbc { did, amount } => {
                if *amount == 0 {
                    return Err(ShardError::ValidationFailed(
                        "Mint amount must be > 0".into(),
                    ));
                }
                let current = self.balances.get(did).copied().unwrap_or(0);
                self.balances
                    .insert(did.clone(), current.saturating_add(*amount));
                Ok(())
            }
            EconomicsOp::SpendUbc { did, amount } => {
                if *amount == 0 {
                    return Err(ShardError::ValidationFailed(
                        "Spend amount must be > 0".into(),
                    ));
                }
                let balance = self.balances.get(did).copied().unwrap_or(0);
                if balance < *amount {
                    return Err(ShardError::ValidationFailed(format!(
                        "Insufficient UBC: have {}, need {}",
                        balance, amount
                    )));
                }
                self.balances.insert(did.clone(), balance - amount);
                Ok(())
            }
            EconomicsOp::RegisterDid { did } => {
                self.balances
                    .entry(did.clone())
                    .or_insert(self.default_quota);
                Ok(())
            }
            EconomicsOp::AdvanceEpoch => {
                self.current_epoch += 1;
                // Reset all balances to default quota
                for balance in self.balances.values_mut() {
                    *balance = self.default_quota;
                }
                Ok(())
            }
            EconomicsOp::SubmitWork { did, .. } => {
                // Simplified: reward 100 UBC for any submitted work
                let current = self
                    .balances
                    .get(did)
                    .copied()
                    .unwrap_or(self.default_quota);
                self.balances
                    .insert(did.clone(), current.saturating_add(100));
                Ok(())
            }
            EconomicsOp::CreateProposal { .. } | EconomicsOp::Vote { .. } => {
                // Governance operations accepted but not tracked in simplified state
                Ok(())
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }
}

/// The Economics shard — handles UBC tokens, useful work rewards, and governance.
///
/// This shard implements the `Shard` trait so it can be registered with
/// `ShardRouter`, making the economics layer a first-class shard in the
/// Omnia architecture.
pub struct EconomicsShard {
    state: EconomicsShardState,
}

impl EconomicsShard {
    /// Create a new Economics shard with default state.
    pub fn new() -> Self {
        Self {
            state: EconomicsShardState::new(),
        }
    }

    /// Get a reference to the internal state.
    pub fn state(&self) -> &EconomicsShardState {
        &self.state
    }
}

impl Default for EconomicsShard {
    fn default() -> Self {
        Self::new()
    }
}

impl Shard for EconomicsShard {
    fn shard_id(&self) -> ShardId {
        ShardId::economics()
    }

    fn process_event(&mut self, _event: &Event, op: ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Economics(econ_op) => self.state.apply(&econ_op),
            _ => Err(ShardError::InvalidOperation(
                "Economics shard received non-Economics operation".into(),
            )),
        }
    }

    fn state_snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.state
            .to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
    }

    fn validate(&self, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Economics(econ_op) => match econ_op {
                EconomicsOp::SpendUbc { amount, .. } if *amount == 0 => Err(
                    ShardError::ValidationFailed("Spend amount must be > 0".into()),
                ),
                EconomicsOp::MintUbc { amount, .. } if *amount == 0 => Err(
                    ShardError::ValidationFailed("Mint amount must be > 0".into()),
                ),
                _ => Ok(()),
            },
            _ => Err(ShardError::InvalidOperation(
                "Economics shard received non-Economics operation".into(),
            )),
        }
    }
}

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

    fn state_snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.state
            .to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
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

    fn state_snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.state
            .to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
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

    fn state_snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.state
            .to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
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

    fn state_snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.state
            .to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
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

    fn state_snapshot(&self) -> Result<Vec<u8>, ShardError> {
        self.state
            .to_bytes()
            .map_err(|e| ShardError::SerializationError(e.to_string()))
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

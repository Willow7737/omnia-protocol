//! CRDT (Conflict-free Replicated Data Types) Module
//!
//! CRDTs are data structures that can be replicated across multiple nodes and
//! merged without coordination. This is fundamental to Omnia's design:
//! causally independent operations on CRDTs converge automatically.
//!
//! This module provides three core CRDT types:
//! - GCounter: Grow-only counter (monotonic increment)
//! - OrSet: Observed-Remove Set (add/remove with unique tokens)
//! - LwwRegister: Last-Write-Wins Register (timestamp-based resolution)
//!
//! All CRDTs in this module are:
//! - Associative: merge(a, merge(b, c)) == merge(merge(a, b), c)
//! - Commutative: merge(a, b) == merge(b, a)
//! - Idempotent: merge(a, a) == a

pub mod g_counter;
pub mod lww_register;
pub mod or_set;

pub use g_counter::{CrdtError, GCounter};
pub use lww_register::LwwRegister;
pub use or_set::OrSet;

use omnia_primitives::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};

/// Trait for state-based CRDTs (CvRDTs)
///
/// State-based CRDTs replicate the entire state and merge it using a
/// deterministic merge function. The merge function **must** satisfy three
/// mathematical properties that together guarantee eventual convergence
/// across all replicas without coordination:
///
/// # Mathematical Properties
///
/// ## 1. Commutativity
///
/// The order of merging does not affect the result:
///
/// ```text
/// merge(A, B) = merge(B, A)    ∀ A, B ∈ S
/// ```
///
/// This ensures that regardless of the order in which replicas exchange
/// state, they arrive at the same result.
///
/// ## 2. Associativity
///
/// Grouping of merges does not affect the result:
///
/// ```text
/// merge(merge(A, B), C) = merge(A, merge(B, C))    ∀ A, B, C ∈ S
/// ```
///
/// This ensures that multi-replica merges converge regardless of the
/// merge topology (star, chain, tree, etc.).
///
/// ## 3. Idempotency
///
/// Merging a state with itself yields the same state:
///
/// ```text
/// merge(A, A) = A    ∀ A ∈ S
///
/// ```
/// This ensures that redundant re-delivery of state (e.g., due to
/// network retries) does not change the result.
///
/// # Convergence Proof Sketch
///
/// Given commutativity + associativity + idempotency, any set of
/// replicas {R₁, R₂, …, Rₙ} that eventually perform pairwise merges
/// (in any order, with possible duplicates) will converge to the
/// same state:
///
/// ```text
/// S_final = merge(R₁, merge(R₂, merge(..., Rₙ)))
/// ```
///
/// The result is independent of merge order (commutativity + associativity)
/// and of duplicate deliveries (idempotency). ∎
pub trait CvRDT: Clone {
    /// Merge another CRDT into this one
    fn merge(&mut self, other: &Self);

    /// Create a merged copy
    fn merged(&self, other: &Self) -> Self
    where
        Self: Sized,
    {
        let mut result = self.clone();
        result.merge(other);
        result
    }
}

/// Trait for operation-based CRDTs
///
/// Op-based CRDTs replicate only the operations, not full state.
/// More efficient for large data structures but require reliable broadcast.
///
/// **Note**: This trait was deprecated in v0.1.56 and has been removed.
/// The protocol uses only state-based CRDTs ([`CvRDT`]). If operation-based
/// CRDTs are needed in the future, this trait can be re-introduced.

/// Trait for CRDTs that can be used as account state
///
/// Account state CRDTs must support:
/// - Incremental updates (no full state replacement needed)
/// - Deterministic merge for consensus
/// - Efficient serialization
pub trait AccountCRDT: CvRDT + Serialize + for<'de> Deserialize<'de> {
    /// Get a stable hash of the current state
    fn state_hash(&self) -> [u8; 32];

    /// Get the vector clock of the last update
    fn last_update(&self) -> &VectorClock;
}

/// Wrapper for account balances using G-Counter CRDT
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountBalance {
    /// The underlying counter
    counter: GCounter,
    /// Track which node last updated
    last_updater: Option<NodeId>,
    /// Vector clock at last update
    vector_clock: VectorClock,
}

impl AccountBalance {
    /// Create a new account balance
    pub fn new() -> Self {
        Self {
            counter: GCounter::new(),
            last_updater: None,
            vector_clock: VectorClock::new(),
        }
    }

    /// Increment the balance for a node
    pub fn increment(&mut self, node_id: NodeId, amount: u64) -> Result<(), CrdtError> {
        self.counter.increment(node_id, amount)?;
        self.last_updater = Some(node_id);
        let _ = self.vector_clock.increment(node_id);
        Ok(())
    }

    /// Get the total balance value
    pub fn value(&self) -> u64 {
        self.counter.value()
    }
}

impl CvRDT for AccountBalance {
    fn merge(&mut self, other: &Self) {
        self.counter.merge(&other.counter);
        // Merge vector clocks so both causal histories are preserved.
        // Without this, concurrent updates from different nodes can lose
        // causal tracking, leading to incorrect merge semantics.
        self.vector_clock = self.vector_clock.merged(&other.vector_clock);
        // Keep the update from the higher clock
        if self.vector_clock.happened_before(&other.vector_clock) {
            self.last_updater = other.last_updater;
        }
    }
}

impl AccountCRDT for AccountBalance {
    fn state_hash(&self) -> [u8; 32] {
        // Include both the counter hash and vector clock in the state hash
        // to ensure determinism across nodes. Previously only the counter
        // was hashed, which could produce identical hashes for states with
        // different causal histories (AUDIT-10).
        use omnia_primitives::blake3_hash_domain;
        let counter_hash = self.counter.state_hash();
        let vc_bytes = self.vector_clock.to_bytes().unwrap_or_default();
        blake3_hash_domain(
            b"omnia-account-balance",
            &[counter_hash.as_slice(), vc_bytes.as_slice()].concat(),
        )
    }

    fn last_update(&self) -> &VectorClock {
        &self.vector_clock
    }
}

impl Default for AccountBalance {
    fn default() -> Self {
        Self::new()
    }
}

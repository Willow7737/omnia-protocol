//! # Omnia Consensus — Consensus engine, causal graph, CRDTs, slashing, and mempool
//!
//! This crate provides the consensus logic for the Omnia Protocol:
//! - **Causal Graph (DAG)**: Directed acyclic graph for event causality tracking
//! - **Consensus Engine**: BFT finality gadget for deterministic event ordering
//! - **CRDTs**: Conflict-free Replicated Data Types for state convergence
//! - **Slashing**: Byzantine fault detection and validator penalization
//! - **Mempool**: Bounded queue for pending events awaiting block inclusion
//! - **Rate Limiter**: Per-peer token-bucket rate limiting
//!
//! Heavy dependencies (persistent storage via redb) are feature-gated.

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod causal_graph;
pub mod consensus;
pub mod crdt;
pub mod mempool;
pub mod rate_limiter;
pub mod slashing;
pub mod slashing_undo;

#[cfg(feature = "persistent-storage")]
pub mod consensus_store;

// Re-export commonly used types at crate root
pub use causal_graph::{
    CausalGraph, CausalGraphError, GraphSnapshot, GraphStats, PrunedEventMetadata,
};
pub use consensus::{
    ConsensusConfig, ConsensusEngine, ConsensusError, ConsensusState, DefaultConsensusEngine,
    RoundTimer,
};
pub use crdt::{CrdtError, CvRDT, GCounter, LwwRegister, OrSet};
pub use mempool::{Mempool, MempoolError};
pub use rate_limiter::RateLimiter;
pub use slashing::{
    InMemorySlashingStore, JailState, SlashOffense, SlashOutcome, SlashPenalty, SlashingEngine,
    SlashingEvent, SlashingEventType, SlashingState, SlashingStore, SlashingStoreError,
    DEFAULT_EJECTION_THRESHOLD, DEFAULT_SLASH_THRESHOLD,
};
pub use slashing_undo::{
    SlashingUndoError, SlashingUndoManager, SlashingUndoRecord, SlashingUndoRequest,
};

#[cfg(feature = "persistent-storage")]
pub use consensus_store::{
    ConsensusState as PersistedConsensusState, ConsensusStore, ConsensusStoreError,
    RedbConsensusStore,
};

/// Trait for slashing backend — enables dependency inversion so that
/// `ConsensusEngine` is not tightly coupled to the concrete `SlashingEngine`.
///
/// Implementations can wrap the built-in `SlashingEngine` or provide
/// custom slashing logic (e.g., governance-controlled slashing).
pub trait SlashingBackend: Send + Sync {
    /// Check whether a node has been slashed.
    fn is_slashed(&self, node: &omnia_primitives::NodeId) -> bool;

    /// Record a slashing offense for a node and return the outcome.
    fn record_offense(
        &mut self,
        node: omnia_primitives::NodeId,
        offense: SlashOffense,
    ) -> SlashOutcome;

    /// Register a validator with its stake amount.
    fn register_validator(&mut self, node: omnia_primitives::NodeId, stake: u64);
}

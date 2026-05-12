//! Shard trait and core error types
//!
//! Every domain shard implements the `Shard` trait, which defines how a shard
//! processes events, validates operations, and snapshots its state.

use omnia_substrate::Event;
use serde::{Deserialize, Serialize};

use crate::payload::ShardOp;

/// Unique identifier for a shard instance.
///
/// Each shard type (Financial, Identity, etc.) gets a unique 32-byte ID
/// derived from its domain and optional instance discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShardId(pub [u8; 32]);

impl ShardId {
    /// Create a ShardId from raw bytes.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Well-known ShardId for the Financial shard.
    pub fn financial() -> Self {
        let mut id = [0u8; 32];
        id[0..8].copy_from_slice(b"FINANCE0");
        Self(id)
    }

    /// Well-known ShardId for the Computational shard.
    pub fn computational() -> Self {
        let mut id = [0u8; 32];
        id[0..8].copy_from_slice(b"COMP___0");
        Self(id)
    }

    /// Well-known ShardId for the Physical shard.
    pub fn physical() -> Self {
        let mut id = [0u8; 32];
        id[0..8].copy_from_slice(b"PHYS___0");
        Self(id)
    }

    /// Well-known ShardId for the Biological shard.
    pub fn biological() -> Self {
        let mut id = [0u8; 32];
        id[0..8].copy_from_slice(b"BIO____0");
        Self(id)
    }

    /// Well-known ShardId for the Identity shard.
    pub fn identity() -> Self {
        let mut id = [0u8; 32];
        id[0..8].copy_from_slice(b"IDENT__0");
        Self(id)
    }

    /// Well-known ShardId for the Economics shard.
    pub fn economics() -> Self {
        let mut id = [0u8; 32];
        id[0..8].copy_from_slice(b"ECONOM00");
        Self(id)
    }
}

impl std::fmt::Display for ShardId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ShardId({})", hex::encode(&self.0[..8]))
    }
}

/// Errors that can occur during shard operations.
#[derive(Debug, thiserror::Error)]
pub enum ShardError {
    /// The operation is not valid for this shard or state.
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// The operation failed validation (e.g., insufficient balance).
    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    /// A state conflict was detected (e.g., double-spend).
    #[error("State conflict: {0}")]
    StateConflict(String),

    /// An error occurred during cross-shard communication.
    #[error("Cross-shard error: {0}")]
    CrossShardError(String),

    /// Failed to deserialize a shard payload.
    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    /// The requested shard was not found in the router.
    #[error("Unknown shard: {0}")]
    UnknownShard(String),
}

/// The core trait that every domain shard must implement.
///
/// Shards are specialized state machines that process events from the causal
/// graph. Each shard maintains its own state and validation rules, but all
/// shards share the same consensus and causal ordering from Layer 1.
pub trait Shard: Send + Sync {
    /// Return the unique identifier for this shard.
    fn shard_id(&self) -> ShardId;

    /// Process a single event and apply the operation to shard state.
    ///
    /// The event provides context (creator, vector clock, signature) while
    /// the operation carries the domain-specific payload.
    fn process_event(&mut self, event: &Event, op: ShardOp) -> Result<(), ShardError>;

    /// Serialize the current shard state into bytes for snapshots.
    ///
    /// Must be deterministic: the same state always produces the same bytes.
    fn state_snapshot(&self) -> Vec<u8>;

    /// Validate an operation without mutating state.
    ///
    /// Used to check whether an operation would succeed before committing it.
    fn validate(&self, op: &ShardOp) -> Result<(), ShardError>;
}

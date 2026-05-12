//! Cross-shard messaging protocol
//!
//! Cross-shard messages allow one shard to communicate with another through
//! the causal graph. The key insight is that cross-shard messages leverage
//! the same vector clock mechanism that the substrate uses for causal
//! ordering — if Shard A sends a message to Shard B, the vector clock
//! captures the dependency, ensuring that B processes the message only
//! after A's state transition is causally confirmed.

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

use crate::shard::ShardId;

/// A message sent from one shard to another through the causal graph.
///
/// Cross-shard messages are embedded as `ShardOp::CrossShard` operations
/// inside events. The target shard detects these messages by scanning events
/// whose `ShardPayload.shard_id` matches its own, and whose operation is a
/// `CrossShardMessage` targeting it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossShardMessage {
    /// The shard that sent this message.
    pub source_shard: ShardId,
    /// The shard that should receive this message.
    pub target_shard: ShardId,
    /// Serialized operation for the target shard to execute.
    pub payload: Vec<u8>,
    /// Vector clock proving the source operation happened before this message.
    pub causal_proof: VectorClock,
}

impl CrossShardMessage {
    /// Create a new cross-shard message.
    pub fn new(
        source_shard: ShardId,
        target_shard: ShardId,
        payload: Vec<u8>,
        causal_proof: VectorClock,
    ) -> Self {
        Self {
            source_shard,
            target_shard,
            payload,
            causal_proof,
        }
    }

    /// Verify that the source event causally precedes this message.
    ///
    /// Returns `true` if `source_vc` (the vector clock at the source shard
    /// when the originating operation was applied) happened before the
    /// `target_vc` (the current vector clock at the target shard).
    pub fn verify_causality(&self, source_vc: &VectorClock, target_vc: &VectorClock) -> bool {
        source_vc.happened_before(target_vc)
    }
}

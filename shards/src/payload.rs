//! Shard payload and operation types
//!
//! A shard operation is embedded inside an `Event.payload` as opaque bytes.
//! The substrate doesn't know or care about `ShardPayload` — it just sees
//! `Vec<u8>`. Shards deserialize the payload and execute the operation.

use serde::{Deserialize, Serialize};

use crate::biological::ops::BiologicalOp;
use crate::computational::ops::ComputationalOp;
use crate::cross_shard::CrossShardMessage;
use crate::financial::ops::FinancialOp;
use crate::identity::ops::IdentityOp;
use crate::physical::ops::PhysicalOp;
use crate::shard::ShardId;
use omnia_economics::EconomicsOp;

/// The top-level payload that wraps every shard operation.
///
/// Each event that carries a shard operation has a `ShardPayload` serialized
/// into `Event.payload`. The `shard_id` tells the router which shard should
/// handle this event, and `nonce` provides replay protection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardPayload {
    /// Which shard handles this operation.
    pub shard_id: ShardId,
    /// The actual domain-specific operation.
    pub operation: ShardOp,
    /// Replay protection — must be monotonically increasing per account.
    pub nonce: u64,
}

/// Union type over all possible shard operations.
///
/// Each variant corresponds to one of the six domain shards. The
/// `CrossShard` variant is used for inter-shard messaging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShardOp {
    /// Financial domain operation (transfers, mint, burn).
    Financial(FinancialOp),
    /// Computational domain operation (task submission, proof verification).
    Computational(ComputationalOp),
    /// Physical domain operation (asset anchoring, ownership transfer).
    Physical(PhysicalOp),
    /// Biological domain operation (consent management, ZK queries).
    Biological(BiologicalOp),
    /// Identity domain operation (DID lifecycle, social recovery).
    Identity(IdentityOp),
    /// Economics domain operation (UBC, useful work, governance).
    Economics(EconomicsOp),
    /// Cross-shard message carrying a payload from one shard to another.
    CrossShard(CrossShardMessage),
}

impl ShardPayload {
    /// Serialize this payload into compact binary bytes for embedding
    /// into an `Event.payload`.
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    /// Deserialize a `ShardPayload` from bytes extracted from `Event.payload`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

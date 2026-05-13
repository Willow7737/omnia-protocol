//! Shard router — dispatches events to the appropriate shard
//!
//! The `ShardRouter` is the central dispatch point for shard operations.
//! When an event arrives with a shard payload, the router deserializes
//! the payload, looks up the target shard, and delegates the operation.

use std::collections::HashMap;

use omnia_substrate::Event;

use crate::cross_shard::CrossShardMessage;
use crate::payload::{ShardOp, ShardPayload};
use crate::shard::{Shard, ShardError, ShardId};

/// Routes shard events to the appropriate shard handler.
///
/// The router maintains a map of `ShardId → Box<dyn Shard>` and
/// dispatches incoming events based on the `shard_id` in the payload.
/// It also handles cross-shard messages by routing them to their
/// target shard.
///
/// Replay protection is enforced via per-creator nonce tracking:
/// each `creator_pubkey` must submit events with strictly increasing
/// nonces, preventing stale or duplicate events from being processed.
pub struct ShardRouter {
    /// Registered shards, indexed by their shard ID.
    shards: HashMap<ShardId, Box<dyn Shard>>,
    /// Last seen nonce per creator pubkey — replay protection.
    last_nonces: HashMap<[u8; 32], u64>,
}

impl ShardRouter {
    /// Create a new, empty shard router.
    pub fn new() -> Self {
        Self {
            shards: HashMap::new(),
            last_nonces: HashMap::new(),
        }
    }

    /// Register a shard with the router.
    pub fn register(&mut self, shard: Box<dyn Shard>) {
        let id = shard.shard_id();
        self.shards.insert(id, shard);
    }

    /// Route an event to the appropriate shard based on the payload.
    ///
    /// The event's `payload` field is deserialized as a `ShardPayload`,
    /// and the `shard_id` determines which shard handles the operation.
    pub fn route(&mut self, event: &Event, op: ShardOp) -> Result<(), ShardError> {
        // Handle cross-shard messages specially
        if let ShardOp::CrossShard(msg) = op {
            return self.route_cross_shard(event, &msg);
        }

        // Determine the target shard ID from the payload
        let shard_id = match &op {
            ShardOp::Financial(_) => ShardId::financial(),
            ShardOp::Computational(_) => ShardId::computational(),
            ShardOp::Physical(_) => ShardId::physical(),
            ShardOp::Biological(_) => ShardId::biological(),
            ShardOp::Identity(_) => ShardId::identity(),
            ShardOp::Economics(_) => ShardId::economics(),
            ShardOp::CrossShard(_) => unreachable!(),
        };

        if let Some(shard) = self.shards.get_mut(&shard_id) {
            shard.process_event(event, op)
        } else {
            Err(ShardError::UnknownShard(format!("{}", shard_id)))
        }
    }

    /// Route a cross-shard message to its target shard.
    fn route_cross_shard(
        &mut self,
        event: &Event,
        msg: &CrossShardMessage,
    ) -> Result<(), ShardError> {
        // Deserialize the inner payload for the target shard
        let inner_op: ShardOp = bincode::deserialize(&msg.payload)
            .map_err(|e| ShardError::DeserializationError(e.to_string()))?;

        let target_id = msg.target_shard;
        if let Some(shard) = self.shards.get_mut(&target_id) {
            shard.process_event(event, inner_op)
        } else {
            Err(ShardError::UnknownShard(format!("{}", target_id)))
        }
    }

    /// Route an event by deserializing its payload.
    ///
    /// Convenience method that deserializes the event's payload,
    /// checks the nonce for replay protection, and delegates to `route()`.
    pub fn route_event(&mut self, event: &Event) -> Result<(), ShardError> {
        if event.payload.is_empty() {
            return Ok(());
        }

        let payload = ShardPayload::from_bytes(&event.payload)
            .map_err(|e| ShardError::ValidationFailed(format!("Invalid payload: {}", e)))?;

        // Replay protection — check nonce
        let creator = event.creator_pubkey;
        let last_nonce = self.last_nonces.get(&creator).copied().unwrap_or(0);
        if payload.nonce <= last_nonce {
            return Err(ShardError::ValidationFailed(format!(
                "Replay detected: nonce {} <= last {}",
                payload.nonce, last_nonce
            )));
        }
        self.last_nonces.insert(creator, payload.nonce);

        self.route(event, payload.operation)
    }

    /// Get a reference to a registered shard by ID.
    pub fn get_shard(&self, id: &ShardId) -> Option<&dyn Shard> {
        self.shards.get(id).map(|s| s.as_ref())
    }

    /// Check whether a shard is registered.
    pub fn has_shard(&self, id: &ShardId) -> bool {
        self.shards.contains_key(id)
    }

    /// Get the number of registered shards.
    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }
}

impl Default for ShardRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl omnia_substrate::EventProcessor for ShardRouter {
    fn process_event(&mut self, event: &Event) -> Result<(), String> {
        self.route_event(event).map_err(|e| e.to_string())
    }
}

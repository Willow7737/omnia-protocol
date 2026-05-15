//! Shard router — dispatches events to the appropriate shard
//!
//! The `ShardRouter` is the central dispatch point for shard operations.
//! When an event arrives with a shard payload, the router deserializes
//! the payload, looks up the target shard, and delegates the operation.
//!
//! Fee enforcement is applied **before** routing: the router consults
//! a `FeeSchedule` to determine the cost of the operation, then
//! deducts the fee from the `QuotaSystem`. If the caller lacks
//! sufficient UBC balance, the operation is rejected with
//! `ShardError::InsufficientFee`.

use std::collections::HashMap;
use std::sync::Arc;

use omnia_economics::QuotaSystem;
use omnia_substrate::Event;

use crate::cross_shard::CrossShardMessage;
use crate::fee_schedule::FeeSchedule;
use crate::nonce_store::{InMemoryNonceStore, NonceStore};
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
///
/// Fee enforcement is applied after nonce validation but before
/// shard dispatch: the operation's fee is looked up in the
/// `FeeSchedule` and deducted from the caller's UBC quota.
pub struct ShardRouter {
    /// Registered shards, indexed by their shard ID.
    shards: HashMap<ShardId, Box<dyn Shard>>,
    /// Last seen nonce per creator pubkey — replay protection.
    last_nonces: HashMap<[u8; 32], u64>,
    /// Per-operation-type fee schedule (UBC units).
    fee_schedule: FeeSchedule,
    /// UBC quota system for fee deduction.
    quota: QuotaSystem,
    /// Persistent nonce store — survives restarts for replay protection.
    nonce_store: Arc<dyn NonceStore>,
}

impl ShardRouter {
    /// Create a new shard router with the given fee schedule and quota system.
    ///
    /// The `fee_schedule` maps each operation type to its UBC cost.
    /// The `quota` system tracks per-DID balances; every operation
    /// whose fee is > 0 will deduct from the caller's balance.
    pub fn new(fee_schedule: FeeSchedule, quota: QuotaSystem) -> Self {
        Self {
            shards: HashMap::new(),
            last_nonces: HashMap::new(),
            fee_schedule,
            quota,
            nonce_store: Arc::new(InMemoryNonceStore::new()),
        }
    }

    /// Create a router with zero fees and an empty quota system.
    ///
    /// This is a backward-compatible constructor for tests and
    /// scenarios that do not require fee enforcement. Because all
    /// fees are zero, no quota deductions are ever attempted.
    pub fn new_without_fees() -> Self {
        Self {
            shards: HashMap::new(),
            last_nonces: HashMap::new(),
            fee_schedule: FeeSchedule::zero(),
            quota: QuotaSystem::default_system(),
            nonce_store: Arc::new(InMemoryNonceStore::new()),
        }
    }

    /// Create a shard router with a custom nonce store.
    ///
    /// Use this constructor when you need persistent nonce storage
    /// (e.g., `SledNonceStore`) for replay protection across restarts.
    ///
    /// # Arguments
    /// * `fee_schedule` - Per-operation-type fee schedule (UBC units)
    /// * `quota` - UBC quota system for fee deduction
    /// * `nonce_store` - Persistent nonce store implementation
    pub fn with_nonce_store(
        fee_schedule: FeeSchedule,
        quota: QuotaSystem,
        nonce_store: Arc<dyn NonceStore>,
    ) -> Self {
        // Load existing nonces from the store
        let last_nonces = nonce_store.load().unwrap_or_else(|e| {
            tracing::warn!("Failed to load nonces from store: {}", e);
            HashMap::new()
        });
        Self {
            shards: HashMap::new(),
            last_nonces,
            fee_schedule,
            quota,
            nonce_store,
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
    /// checks the nonce for replay protection, deducts the fee
    /// from the caller's quota, and delegates to `route()`.
    pub fn route_event(&mut self, event: &Event) -> Result<(), ShardError> {
        if event.payload.is_empty() {
            return Ok(());
        }

        // Reject oversized payloads early, before any deserialization work
        if event.payload.len() > omnia_substrate::event::MAX_PAYLOAD_SIZE {
            return Err(ShardError::ValidationFailed(
                format!("Payload too large: {} bytes (max {})",
                    event.payload.len(),
                    omnia_substrate::event::MAX_PAYLOAD_SIZE
                )
            ));
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

        // Persist nonce state (best-effort, log on failure)
        if let Err(e) = self.nonce_store.save(&self.last_nonces) {
            tracing::warn!("Failed to persist nonce state: {}", e);
        }

        // Fee enforcement — deduct before routing
        let fee = self.fee_schedule.fee_for_op(&payload.operation);
        if fee > 0 {
            let did = Self::pubkey_to_did(&event.creator_pubkey);
            self.quota.spend(&did, fee).map_err(|e| {
                tracing::warn!(
                    did = %did,
                    fee = fee,
                    error = %e,
                    "Fee deduction failed — insufficient quota"
                );
                ShardError::InsufficientFee(format!("Quota exceeded: {}", e))
            })?;
        }

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

    /// Convert a 32-byte Ed25519 public key to a DID string.
    ///
    /// Uses hex encoding of the public key bytes to form a
    /// `did:omnia:<hex>` identifier. This DID is used as the
    /// account key in the quota system.
    pub fn pubkey_to_did(pubkey: &[u8; 32]) -> String {
        format!("did:omnia:{}", hex::encode(pubkey))
    }
}

impl Default for ShardRouter {
    fn default() -> Self {
        Self::new_without_fees()
    }
}

impl omnia_substrate::EventProcessor for ShardRouter {
    fn process_event(&mut self, event: &Event) -> Result<(), String> {
        self.route_event(event).map_err(|e| e.to_string())
    }
}

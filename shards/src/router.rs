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

/// Maximum allowed gap between the submitted nonce and the last seen nonce.
///
/// Prevents nonce-gap attacks where a malicious actor reserves a large range
/// of future nonces, blocking legitimate events. This constant should be
/// tuned based on the expected concurrent event rate per creator.
const NONCE_GAP_LIMIT: u64 = 1000;

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
    /// (e.g., `RedbNonceStore`) for replay protection across restarts.
    ///
    /// # Arguments
    /// * `fee_schedule` - Per-operation-type fee schedule (UBC units)
    /// * `quota` - UBC quota system for fee deduction
    /// * `nonce_store` - Persistent nonce store implementation
    pub fn with_nonce_store(fee_schedule: FeeSchedule, quota: QuotaSystem, nonce_store: Arc<dyn NonceStore>) -> Self {
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
    ///
    /// # Arguments
    ///
    /// * `shard` — A boxed [`Shard`] implementation. The shard's ID
    ///   (from [`Shard::shard_id`]) is used as the routing key.
    pub fn register(&mut self, shard: Box<dyn Shard>) {
        let id = shard.shard_id();
        self.shards.insert(id, shard);
    }

    /// Route an event to the appropriate shard based on the payload.
    ///
    /// The event's `payload` field is deserialized as a `ShardPayload`,
    /// and the `shard_id` determines which shard handles the operation.
    ///
    /// # Arguments
    ///
    /// * `event` — The event to route.
    /// * `op` — The shard operation to execute.
    ///
    /// # Errors
    ///
    /// Returns [`ShardError::UnknownShard`] if the target shard is not registered.
    /// Returns [`ShardError::DeserializationError`] for cross-shard message decoding failures.
    /// May propagate shard-specific errors from [`Shard::process_event`].
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
            Err(ShardError::UnknownShard(format!("{shard_id}")))
        }
    }

    /// Route a cross-shard message to its target shard.
    fn route_cross_shard(&mut self, event: &Event, msg: &CrossShardMessage) -> Result<(), ShardError> {
        // TODO: Verify cross-shard causal proof before processing.
        // The message must include a vector clock or causal proof demonstrating
        // that the source shard observed all events that the target shard depends on.
        // Without this verification, a malicious source shard could fabricate
        // cross-shard messages that violate causal ordering, leading to
        // inconsistent state across shards. Implement causal proof verification
        // by checking that msg.causal_proof is consistent with the target shard's
        // observed vector clock before dispatching the inner operation.

        // Deserialize the inner payload for the target shard
        let inner_op: ShardOp =
            postcard::from_bytes(&msg.payload).map_err(|e| ShardError::DeserializationError(e.to_string()))?;

        // Verify the source shard matches the shard type that would generate this event
        let expected_source = Self::shard_id_from_op(&inner_op)?;
        if expected_source != msg.source_shard {
            return Err(ShardError::ValidationFailed(format!(
                "cross-shard message source mismatch: expected {:?}, got {:?}",
                expected_source, msg.source_shard
            )));
        }

        let target_id = msg.target_shard;
        if let Some(shard) = self.shards.get_mut(&target_id) {
            shard.process_event(event, inner_op)
        } else {
            Err(ShardError::UnknownShard(format!("{target_id}")))
        }
    }

    /// Route an event by deserializing its payload.
    ///
    /// Convenience method that deserializes the event's payload,
    /// checks the nonce for replay protection, deducts the fee
    /// from the caller's quota, and delegates to `route()`.
    ///
    /// # Arguments
    ///
    /// * `event` — The event whose payload contains the shard operation.
    ///
    /// # Errors
    ///
    /// - [`ShardError::ValidationFailed`] — payload deserialization failed,
    ///   payload exceeds `MAX_PAYLOAD_SIZE`, or replay detected (nonce too low).
    /// - [`ShardError::InsufficientFee`] — the caller lacks sufficient UBC quota.
    /// - [`ShardError::UnknownShard`] — the target shard is not registered.
    ///
    /// # Security
    ///
    /// Enforces strictly increasing nonces per `creator_pubkey` to prevent
    /// replay attacks. A nonce that is ≤ the last seen nonce for that
    /// creator is rejected. Nonces are persisted to the backing store
    /// for crash recovery.
    pub fn route_event(&mut self, event: &Event) -> Result<(), ShardError> {
        if event.payload.is_empty() {
            return Ok(());
        }

        // Reject oversized payloads early, before any deserialization work
        if event.payload.len() > omnia_substrate::event::MAX_PAYLOAD_SIZE {
            return Err(ShardError::ValidationFailed(format!(
                "Payload too large: {} bytes (max {})",
                event.payload.len(),
                omnia_substrate::event::MAX_PAYLOAD_SIZE
            )));
        }

        let payload = ShardPayload::from_bytes(&event.payload)
            .map_err(|e| ShardError::ValidationFailed(format!("Invalid payload: {e}")))?;

        // Replay protection — check nonce
        let creator = event.creator_pubkey;
        let last_nonce = self.last_nonces.get(&creator).copied().unwrap_or(0);
        if payload.nonce <= last_nonce {
            return Err(ShardError::ValidationFailed(format!(
                "Replay detected: nonce {} <= last {}",
                payload.nonce, last_nonce
            )));
        }
        // Prevent nonce gap attacks: reject nonces more than NONCE_GAP_LIMIT above the last seen
        if payload.nonce > last_nonce + NONCE_GAP_LIMIT {
            return Err(ShardError::ValidationFailed(format!(
                "nonce {} too far ahead of last nonce {} (max gap: {NONCE_GAP_LIMIT})",
                payload.nonce, last_nonce
            )));
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
                ShardError::InsufficientFee(format!("Quota exceeded: {e}"))
            })?;
        }

        let result = self.route(event, payload.operation);

        // Refund the fee if the route failed
        if result.is_err() && fee > 0 {
            let did = Self::pubkey_to_did(&event.creator_pubkey);
            if let Err(e) = self.quota.reward(&did, fee) {
                tracing::error!(
                    did = %did,
                    fee = fee,
                    error = %e,
                    "CRITICAL: Failed to refund fee after route failure — balance inconsistency"
                );
            }
        }

        // Only persist nonce and insert into in-memory map if operation succeeded
        if result.is_ok() {
            self.last_nonces.insert(creator, payload.nonce);
            if let Err(e) = self.nonce_store.save_incremental(&creator, payload.nonce) {
                tracing::warn!("Failed to persist nonce for creator: {}", e);
            }
        }

        result
    }

    /// Get a reference to a registered shard by ID.
    ///
    /// # Returns
    ///
    /// `Some(&dyn Shard)` if the shard is registered, `None` otherwise.
    pub fn get_shard(&self, id: &ShardId) -> Option<&dyn Shard> {
        self.shards.get(id).map(|s| s.as_ref())
    }

    /// Check whether a shard is registered.
    ///
    /// # Returns
    ///
    /// `true` if a shard with the given ID has been registered.
    pub fn has_shard(&self, id: &ShardId) -> bool {
        self.shards.contains_key(id)
    }

    /// Get the number of registered shards.
    ///
    /// # Returns
    ///
    /// The count of shards currently registered with the router.
    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    /// Convert a public key to a DID string.
    pub fn pubkey_to_did(pubkey: &[u8; 32]) -> String {
        format!("did:omnia:{}", hex::encode(pubkey))
    }

    /// Determine which shard an operation belongs to.
    ///
    /// Maps each `ShardOp` variant to its corresponding `ShardId`.
    /// Used for cross-shard message source verification.
    fn shard_id_from_op(op: &ShardOp) -> Result<ShardId, ShardError> {
        match op {
            ShardOp::Financial(_) => Ok(ShardId::financial()),
            ShardOp::Computational(_) => Ok(ShardId::computational()),
            ShardOp::Physical(_) => Ok(ShardId::physical()),
            ShardOp::Biological(_) => Ok(ShardId::biological()),
            ShardOp::Identity(_) => Ok(ShardId::identity()),
            ShardOp::Economics(_) => Ok(ShardId::economics()),
            ShardOp::CrossShard(_) => Err(ShardError::ValidationFailed(
                "Nested cross-shard messages are not supported".into(),
            )),
        }
    }
}

impl Default for ShardRouter {
    fn default() -> Self {
        Self::new_without_fees()
    }
}

// The EventProcessor trait lives in the deprecated omnia-substrate crate.
// Allow deprecated usage here since this impl provides backward compatibility
// for consumers that still reference the substrate layer.
#[allow(deprecated)]
impl omnia_substrate::EventProcessor for ShardRouter {
    #[allow(deprecated)]
    fn process_event(&mut self, event: &Event) -> Result<(), omnia_substrate::EventProcessorError> {
        self.route_event(event).map_err(|e| match e {
            ShardError::DeserializationError(msg) => omnia_substrate::EventProcessorError::Deserialization(msg),
            ShardError::ValidationFailed(msg) => omnia_substrate::EventProcessorError::ValidationFailed(msg),
            ShardError::UnknownShard(msg) => omnia_substrate::EventProcessorError::UnknownShard(msg),
            other => omnia_substrate::EventProcessorError::ShardError(other.to_string()),
        })
    }
}

/// Wrapper that allows a shared [`ShardRouter`] (behind `Arc<std::sync::Mutex>`)
/// to implement the [`omnia_substrate::EventProcessor`] trait.
///
/// This enables the **same** `ShardRouter` instance to be used both by the
/// HTTP API layer (via `AppState` in the node crate) and by the Substrate
/// consensus loop (via `EventProcessor`). Committed events from consensus
/// are automatically routed to the appropriate domain shard.
///
/// # Why `std::sync::Mutex`?
///
/// The `EventProcessor::process_event` method is synchronous, so we cannot
/// use `tokio::sync::Mutex` (which requires `.await`). `std::sync::Mutex` is
/// appropriate here because `ShardRouter` operations are CPU-only (no I/O),
/// so the lock is held for only a few microseconds.
///
/// # Mutex poisoning
///
/// If the mutex is poisoned (a panic occurred while holding the lock), the
/// processor returns [`omnia_substrate::EventProcessorError::Internal`]. This is preferable
/// to silently dropping events or unwinding across the consensus boundary.
///
/// # Example
///
/// ```ignore
/// use std::sync::{Arc, Mutex};
/// use omnia_shards::{ShardRouter, MutexShardRouter};
/// use omnia_substrate::{Substrate, SubstrateConfig, EventProcessor};
///
/// let shard_router = ShardRouter::new(fee_schedule, quota);
/// let shared: Arc<Mutex<ShardRouter>> = Arc::new(Mutex::new(shard_router));
///
/// // Clone the Arc for the EventProcessor wrapper
/// let processor = MutexShardRouter::new(Arc::clone(&shared));
///
/// // Wire into substrate
/// let substrate = Substrate::new(config)
///     .with_shard_processor(Box::new(processor));
///
/// // The same Arc can be stored in AppState for the HTTP API
/// // app_state.shard_router = shared;
/// ```
pub struct MutexShardRouter {
    inner: Arc<std::sync::Mutex<ShardRouter>>,
}

impl MutexShardRouter {
    /// Create a new wrapper around an `Arc<std::sync::Mutex<ShardRouter>>`.
    ///
    /// The caller should clone the `Arc` before passing it so that the
    /// same `ShardRouter` can be shared with the HTTP API layer.
    pub fn new(inner: Arc<std::sync::Mutex<ShardRouter>>) -> Self {
        Self { inner }
    }
}

#[allow(deprecated)]
impl omnia_substrate::EventProcessor for MutexShardRouter {
    #[allow(deprecated)]
    fn process_event(&mut self, event: &Event) -> Result<(), omnia_substrate::EventProcessorError> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| omnia_substrate::EventProcessorError::Internal(format!("ShardRouter mutex poisoned: {e}")))?;
        guard.process_event(event)
    }
}

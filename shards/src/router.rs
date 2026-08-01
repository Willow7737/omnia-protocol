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
    /// AUDIT-2026-07 C4 (#342): registered validator-set attestation key
    /// per source shard. A cross-shard message is authorized only if its
    /// attestation verifies against the key registered here for its
    /// `source_shard`. Populated by node-operator/genesis config via
    /// [`register_shard_attestation_key`](Self::register_shard_attestation_key)
    /// — deliberately not reachable from consensus payloads, so no network
    /// participant can register a key for a shard they do not control.
    shard_attestation_keys: HashMap<ShardId, [u8; 32]>,
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
            shard_attestation_keys: HashMap::new(),
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
            shard_attestation_keys: HashMap::new(),
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
        // SECURITY: refuse to start with an empty nonce map if the store
        // load failed. The previous implementation silently reset replay
        // protection to an empty map on load failure (e.g., corrupted redb
        // file or disk error), which would allow every previously-seen
        // nonce to be replayed.
        //
        // We log the error and start with an empty map ONLY because the
        // constructor signature does not return Result. Callers who need
        // fail-closed behavior should call `with_nonce_store_checked`
        // instead. For backwards compatibility, this method still panics
        // on load failure — same behavior as before, but no longer silent.
        let last_nonces = nonce_store.load().unwrap_or_else(|e| {
            // Log loudly then panic. Silent recovery was the original bug;
            // the only safe options are (a) refuse to start, or (b) start
            // with explicit operator acknowledgement. We choose (a) for the
            // default constructor and provide (b) as `with_nonce_store_checked`.
            tracing::error!(
                error = %e,
                "FAILED to load nonces from store — refusing to start with \
                 empty replay-protection map. Use with_nonce_store_checked() \
                 if you want explicit error handling."
            );
            panic!(
                "Nonce store load failed: {e}. Refusing to start with \
                 replay protection disabled. Use with_nonce_store_checked() \
                 to handle this error explicitly."
            );
        });
        Self {
            shards: HashMap::new(),
            last_nonces,
            fee_schedule,
            quota,
            nonce_store,
            shard_attestation_keys: HashMap::new(),
        }
    }

    /// Create a shard router with a custom nonce store, returning an error
    /// if the store cannot be loaded.
    ///
    /// This is the fail-closed variant of [`Self::with_nonce_store`]. Use
    /// this in production deployments where replay protection must not be
    /// silently disabled.
    pub fn with_nonce_store_checked(
        fee_schedule: FeeSchedule,
        quota: QuotaSystem,
        nonce_store: Arc<dyn NonceStore>,
    ) -> Result<Self, ShardError> {
        let last_nonces = nonce_store
            .load()
            .map_err(|e| ShardError::DeserializationError(format!("Nonce store load failed: {e}")))?;
        Ok(Self {
            shards: HashMap::new(),
            last_nonces,
            fee_schedule,
            quota,
            nonce_store,
            shard_attestation_keys: HashMap::new(),
        })
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

    /// Register the validator-set attestation key for a source shard
    /// (AUDIT-2026-07 C4, #342).
    ///
    /// Cross-shard messages claiming to originate from `shard_id` are
    /// authorized only if their attestation verifies against `pubkey`.
    /// This is a node-operator/genesis action; it must be driven from
    /// trusted configuration or startup code, never from a consensus
    /// payload — otherwise a network participant could register a key for
    /// a shard they do not control and forge that shard's messages.
    /// Re-registering overwrites the previous key (key rotation).
    pub fn register_shard_attestation_key(&mut self, shard_id: ShardId, pubkey: [u8; 32]) {
        self.shard_attestation_keys.insert(shard_id, pubkey);
    }

    /// The registered attestation key for a source shard, if any.
    pub fn shard_attestation_key(&self, shard_id: &ShardId) -> Option<&[u8; 32]> {
        self.shard_attestation_keys.get(shard_id)
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
            ShardOp::CrossShard(_) => {
                return Err(ShardError::InvalidOperation(
                    "CrossShard operations must be routed via route_cross_shard(), not route()".into(),
                ));
            }
        };

        if let Some(shard) = self.shards.get_mut(&shard_id) {
            shard.process_event(event, op)
        } else {
            Err(ShardError::UnknownShard(format!("{shard_id}")))
        }
    }

    /// Route a cross-shard message to its target shard.
    fn route_cross_shard(&mut self, event: &Event, msg: &CrossShardMessage) -> Result<(), ShardError> {
        // AUDIT-2026-07 C4 (#342): AUTHORIZATION, not just authentication.
        //
        // The old code verified `source_signature` against
        // `event.creator_pubkey` — the creator of the CARRYING event, who
        // can be any authenticated user. So any user could forge a message
        // "from" any source shard by signing it with their own key. The
        // causal_proof was likewise attacker-set and only checked against
        // the attacker's own event.
        //
        // Now the message must carry an attestation by the SOURCE SHARD's
        // registered validator-set key, verified against the key this
        // router has registered for `msg.source_shard`. A user who does
        // not hold that key cannot forge a message from that shard.
        //
        //   1. The source shard must have a registered attestation key. An
        //      unregistered source shard cannot originate cross-shard
        //      messages (fail closed).
        //   2. The attestation must verify against that registered key over
        //      the message's state-transition commitment.
        let source_key = self.shard_attestation_keys.get(&msg.source_shard).ok_or_else(|| {
            tracing::error!(
                source_shard = ?msg.source_shard,
                target_shard = ?msg.target_shard,
                "cross-shard message rejected — source shard has no registered attestation key"
            );
            ShardError::ValidationFailed(format!(
                "cross-shard message rejected: source shard {:?} has no registered attestation key",
                msg.source_shard
            ))
        })?;

        if !msg.verify_attestation(source_key) {
            tracing::error!(
                source_shard = ?msg.source_shard,
                target_shard = ?msg.target_shard,
                "cross-shard message rejected — attestation does not verify against the registered source-shard key"
            );
            return Err(ShardError::ValidationFailed(
                "cross-shard message rejected: attestation does not verify against the \
                 registered source-shard validator-set key"
                    .into(),
            ));
        }

        // SECURITY: Verify cross-shard causal proof before processing.
        //
        // The message carries a `causal_proof` vector clock that must
        // represent a real causal dependency on the source shard's state.
        // We check two things:
        //
        //   1. The `causal_proof` is non-empty. A fabricated message from
        //      a malicious source shard would have an empty (or zero)
        //      vector clock because the attacker never actually executed
        //      the source operation. Real cross-shard messages always
        //      carry a non-trivial vector clock set by the source shard's
        //      apply path.
        //
        //   2. The `causal_proof` happened-before the carrying event's
        //      vector clock. This ensures the cross-shard message is
        //      being processed at a point in causal history that is
        //      strictly after the source operation it claims to depend
        //      on. Without this check, a malicious source could fabricate
        //      cross-shard messages that violate causal ordering, leading
        //      to inconsistent state across shards.
        if msg.causal_proof.is_empty() {
            return Err(ShardError::ValidationFailed(
                "cross-shard message rejected: causal_proof vector clock is empty \
                 — message may be fabricated"
                    .into(),
            ));
        }
        if !msg.verify_causality(&msg.causal_proof, &event.vector_clock) {
            return Err(ShardError::ValidationFailed(
                "cross-shard message rejected: causal_proof does not causally precede \
                 the carrying event's vector clock"
                    .into(),
            ));
        }

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
    /// Not every event carries a shard operation. The substrate DAG also
    /// carries events whose payloads are opaque to the shard layer — raw
    /// API event submissions and transfer-provenance receipts, for
    /// example. Such events are **skipped** (returned `Ok`) rather than
    /// rejected: a payload that does not deserialize as a [`ShardPayload`]
    /// is simply "not for the shards", not an error to log on every
    /// committed non-shard event.
    ///
    /// # Errors
    ///
    /// - [`ShardError::ValidationFailed`] — payload exceeds
    ///   `MAX_PAYLOAD_SIZE`, or replay detected (nonce too low).
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

        // A payload that isn't a ShardPayload is a non-shard event (raw API
        // submission, transfer receipt, …) — skip it cleanly rather than
        // erroring on every committed non-shard event.
        let payload = match ShardPayload::from_bytes(&event.payload) {
            Ok(payload) => payload,
            Err(_) => return Ok(()),
        };

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

        // Capture the operation's debug representation before `route()` moves it.
        // C-6 fix (audit v0.1.68): fees are NON-REFUNDABLE — we log on failure
        // for observability, but the fee is burned regardless of outcome.
        let op_debug = format!("{:?}", payload.operation);
        let result = self.route(event, payload.operation);

        // Previously, a failed `route()` refunded the fee via `quota.reward()`.
        // This defeats the anti-spam purpose of fees — an attacker can spam
        // the network with deliberately-invalid operations at zero cost. The
        // fee is now burned on attempt regardless of outcome, which is the
        // standard anti-spam model in fee-based systems.
        //
        // We only log the failure here for observability.
        if let Err(ref e) = result {
            tracing::debug!(
                operation = %op_debug,
                fee_burned = fee,
                error = %e,
                "Operation failed; fee non-refundable (cost of attempting)"
            );
        }

        // Only persist nonce and insert into in-memory map if operation succeeded
        if result.is_ok() {
            // AUDIT-2026-07 C8 (#346): persist FIRST, acknowledge in memory
            // only after persistence succeeds. The previous order inserted
            // into `last_nonces` and then persisted — on save failure the
            // node halted (NEW-M7 fail-closed), but memory had acknowledged
            // a nonce that disk never recorded, so the memory/disk states
            // straddling the halt disagreed. With persist-first, at every
            // instant the durable store is at least as advanced as the
            // in-memory map, so a restart can never resurrect a nonce
            // acknowledgment that was not durable.
            //
            // Remaining (documented) gap: the shard-state mutation above
            // still precedes nonce durability; closing that fully requires
            // one transaction covering shard-state snapshot + nonce
            // (tracked in #346).
            //
            // NEW-M7 fix retained: on save_incremental failure, halt the
            // node instead of logging a warning — fail-closed, so the
            // operator fixes the disk issue before more events are
            // processed.
            if let Err(e) = self.nonce_store.save_incremental(&creator, payload.nonce) {
                tracing::error!(
                    creator = ?&creator[..4],
                    error = %e,
                    "CRITICAL: Failed to persist nonce — halting to prevent \
                     replay attacks. Fix the disk issue and restart."
                );
                // Panic is intentional — this is a consensus-safety issue.
                // Silent continuation would allow replay of processed events
                // after restart.
                panic!(
                    "Nonce persistence failed for creator {:?}: {}. \
                     Halting to prevent replay protection bypass. \
                     Fix the disk issue and restart.",
                    &creator[..4],
                    e
                );
            }
            self.last_nonces.insert(creator, payload.nonce);
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

    /// Borrow the registered economics shard's [`omnia_economics::EconomicsState`], the
    /// single source of truth for UBC balances, quotas, and governance.
    ///
    /// Both the consensus/event path (this router, via the substrate's
    /// shard processor) and the HTTP API share one `Arc<Mutex<ShardRouter>>`,
    /// so this accessor lets the API read/write the very same economics
    /// state the event path mutates — eliminating the divergent second
    /// copy the C4 audit flagged.
    ///
    /// Returns `None` if no economics shard is registered.
    pub fn economics(&self) -> Option<&omnia_economics::EconomicsState> {
        self.shards
            .get(&ShardId::economics())?
            .as_any()
            .downcast_ref::<crate::EconomicsShard>()
            .map(|s| s.state())
    }

    /// Mutably borrow the registered economics shard's [`omnia_economics::EconomicsState`].
    ///
    /// See [`economics`](Self::economics). Returns `None` if no economics
    /// shard is registered.
    pub fn economics_mut(&mut self) -> Option<&mut omnia_economics::EconomicsState> {
        self.shards
            .get_mut(&ShardId::economics())?
            .as_any_mut()
            .downcast_mut::<crate::EconomicsShard>()
            .map(|s| s.state_mut())
    }

    /// Borrow the registered financial shard's [`crate::FinancialState`] —
    /// the transferable-asset ledger.
    ///
    /// This is a different economy from [`economics`](Self::economics).
    /// UBC, which the economics shard owns, is soulbound: it is a monthly
    /// compute right that resets each epoch and can only be spent, never
    /// moved between identities. The financial shard holds balances that
    /// genuinely move from one account to another. Both are reachable
    /// through the one shared router, so the API and the event path see
    /// the same state for each.
    ///
    /// Returns `None` if no financial shard is registered.
    pub fn financial(&self) -> Option<&crate::FinancialState> {
        self.shards
            .get(&ShardId::financial())?
            .as_any()
            .downcast_ref::<crate::FinancialShard>()
            .map(|s| s.state())
    }

    /// Mutably borrow the registered financial shard's [`crate::FinancialState`].
    ///
    /// See [`financial`](Self::financial). Returns `None` if no financial
    /// shard is registered.
    pub fn financial_mut(&mut self) -> Option<&mut crate::FinancialState> {
        self.shards
            .get_mut(&ShardId::financial())?
            .as_any_mut()
            .downcast_mut::<crate::FinancialShard>()
            .map(|s| s.state_mut())
    }

    /// Convert a creator public key to a DID string.
    ///
    /// DIDs are derived as `did:omnia:<hex(pubkey)>` so that each Ed25519
    /// public key maps to exactly one DID. This is used internally for
    /// quota lookups but is exposed publicly for callers that need to
    /// pre-fund a DID before its first operation.
    pub fn pubkey_to_did(pubkey: &[u8; 32]) -> String {
        format!("did:omnia:{}", hex::encode(pubkey))
    }

    /// Look up the current UBC quota balance for a DID.
    ///
    /// Returns `None` if the DID is not registered in the quota system.
    ///
    /// This accessor exists primarily for tests and observability — it
    /// allows verifying that fee deductions (and lack of refunds, per
    /// the C-6 fix) behaved as expected without exposing the entire
    /// `QuotaSystem` internals.
    pub fn quota_balance(&self, did: &str) -> Option<u64> {
        self.quota.balance_of(did)
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

// F-23 fix: removed stale #[allow(deprecated)] markers. The EventProcessor
// trait and the omnia-substrate crate have no #[deprecated] attribute —
// the markers were left over from a previous deprecation that was reverted.
impl omnia_substrate::EventProcessor for ShardRouter {
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

// F-23 fix: removed stale #[allow(deprecated)] — see comment above.
impl omnia_substrate::EventProcessor for MutexShardRouter {
    fn process_event(&mut self, event: &Event) -> Result<(), omnia_substrate::EventProcessorError> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| omnia_substrate::EventProcessorError::Internal(format!("ShardRouter mutex poisoned: {e}")))?;
        guard.process_event(event)
    }
}

impl ShardRouter {
    /// Read the in-memory acknowledged nonce for a creator.
    ///
    /// Hidden from docs: exists for the C8 (#346) persist-ordering
    /// regression tests, which must observe that a failed nonce persist
    /// never acknowledges in memory. Read-only; not a public API.
    #[doc(hidden)]
    pub fn last_acknowledged_nonce(&self, creator: &[u8; 32]) -> Option<u64> {
        self.last_nonces.get(creator).copied()
    }
}

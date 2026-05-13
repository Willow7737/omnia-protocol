# ADR-007: Binding Shard Interface — PhysicalShard and ProvenanceTracker

**Status**: Proposed
**Date**: 2026-05-14
**Decision**: Use a `ProvenanceTracker` wrapper that enriches PhysicalShard events with Binding Layer capabilities (RF fingerprints, quantum commitments, provenance logs), rather than modifying the existing PhysicalShard implementation.

## Context

The Binding Layer (`binding/` crate) provides cryptographic binding between physical-world items and their digital representations in the causal graph. The core types are:

- `RfFingerprint` (`binding/src/rf_fingerprint.rs`) — Proves a physical device's identity via RF spectral analysis.
- `QuantumCommitment` (`binding/src/quantum_commit.rs`) — Ensures long-term cryptographic integrity using hybrid classical/post-quantum signatures.
- `ProvenanceLog` (`binding/src/provenance.rs`) — An append-only CRDT that records the complete chain of custody for a physical item.
- `PhysicalAnchor` (`binding/src/anchor.rs`) — The unified type combining all three pillars.
- `ProvenanceTracker` (`binding/src/physical_shard.rs`) — The integration point between the Binding Layer and the Physical Shard.

The Physical Shard (`shards/src/physical/`) handles physical-world state: item registration, location tracking, and sensor data. It implements the `Shard` trait from `shards/src/shard.rs`.

The design constraint is that the existing PhysicalShard implementation must not be modified. Instead, the Binding Layer provides a `ProvenanceTracker` that wraps the shard and enriches it with binding-layer capabilities.

## Decision

### Event Flow: PhysicalShard → ProvenanceTracker → CausalGraph

When a physical item is anchored on-chain, the event flow is:

```
PhysicalShard event → ProvenanceTracker.create() → CausalGraph anchor
```

Concretely:

1. A `ShardOp::PhysicalCreate` event arrives at the `ShardRouter`.
2. The router dispatches the event to the Physical Shard, which creates the item in its internal state (`PhysicalState`).
3. Simultaneously, the `ProvenanceTracker` intercepts the event and calls `anchor_item()`:

```rust
pub fn anchor_item(
    &mut self,
    item_id: [u8; 32],
    creator_did: String,
    rf_proof: RfFingerprint,
    commitment: QuantumCommitment,
    causal_anchor: [u8; 32],  // EventId from the causal graph
) -> Result<(), ProvenanceTrackerError>
```

4. `anchor_item()` creates a new `ProvenanceLog` with a `Created` event and wraps it in a `PhysicalAnchor` along with the RF fingerprint and quantum commitment.
5. The `causal_anchor` parameter is the `EventId` of the corresponding event in the `CausalGraph`, linking the provenance log to the L1 event stream.

For transfers, the flow is similar:

```
PhysicalShard transfer event → ProvenanceTracker.transfer_item() → ProvenanceLog.append(Transferred)
```

### Error Handling: Hardware Unavailable → Soft Failure

The Binding Layer interacts with physical hardware (RF sensors, quantum key generators) that may be temporarily unavailable. The error handling strategy is:

- **RF sensor unavailable**: The `ProvenanceTracker` logs a warning via `tracing::warn!` and continues processing. The event is recorded in the provenance log with a stub RF fingerprint. The `RfFingerprint::stub()` method creates a deterministic placeholder that will be replaced when the sensor comes back online.

- **Quantum commitment failure**: The `QuantumCommitment::new_stub()` method creates a placeholder commitment. This is acceptable for Phase 0 where real post-quantum signatures are not yet required.

- **Item not found / already anchored / destroyed**: These are hard errors returned as `ProvenanceTrackerError` variants. The caller (the `ShardRouter`) logs the error and continues processing other events.

The key principle is: **hardware unavailability is a soft failure (log warning, continue), but logical errors (double-anchor, transfer of destroyed item) are hard failures (return error)**.

### State Consistency: Provenance Log Must Match PhysicalShard State

A critical invariant is that the `ProvenanceTracker`'s state must be consistent with the Physical Shard's `PhysicalState`. Specifically:

- If an item exists in `PhysicalState`, it must have a corresponding `PhysicalAnchor` in the `ProvenanceTracker`.
- If an item is destroyed in `PhysicalState`, the `ProvenanceLog` must have a `Destroyed` event as its last entry.
- The `current_holder` in the `ProvenanceLog` must match the owner in `PhysicalState`.

This consistency is maintained by the `ShardRouter`, which calls both the Physical Shard and the `ProvenanceTracker` in sequence for every physical operation. If either call fails, the router rolls back both (or, in Phase 0, logs the inconsistency and continues).

### How ProvenanceLog's Append-Only CRDT Interacts with the Shard Trait

The `ProvenanceLog` (defined in `binding/src/provenance.rs`) is an append-only CRDT. Appends are:

- **Commutative**: The order of appending events from different shards doesn't matter — they all end up in the log.
- **Associative**: Grouping of appends doesn't affect the result.
- **Idempotent**: Appending the same event twice has no effect (enforced by the causal graph's `DuplicateEvent` check at the substrate level).

This CRDT property is essential for the `Shard` trait contract (ADR-006), which requires deterministic `process_event()` results. The `ProvenanceLog` achieves determinism because:

1. Events are processed in causal order (determined by the event's vector clock).
2. The append-only nature means no deletions or modifications — the log only grows.
3. The `verify_chain()` method checks that every consecutive pair of events has a valid cryptographic link (each event's `QuantumCommitment` must link to the previous event's commitment).

However, the `ProvenanceLog` is not itself a `Shard` implementation. It is a data structure used by the `ProvenanceTracker`, which sits alongside the Physical Shard. The `ProvenanceTracker` does not implement the `Shard` trait directly — instead, it is called by the `ShardRouter` as a sidecar to the Physical Shard.

This design avoids a conflict between the CRDT's commutativity and the `Shard` trait's determinism requirement. The `ProvenanceLog` is commutative across *different* items (two different items' logs can be updated in any order), but for a *single* item, the log is strictly ordered (creation → transfers → destruction). This single-item ordering is enforced by the `ProvenanceTracker`'s `transfer_item()` and `destroy_item()` methods, which append events in the order they are received from the causal graph.

### The PhysicalAnchor as the Unified Verification Type

The `PhysicalAnchor` (in `binding/src/anchor.rs`) is the unified type that combines:

1. `rf_fingerprint: RfFingerprint` — Physical identity proof
2. `quantum_commitment: QuantumCommitment` — Long-term cryptographic integrity proof
3. `provenance_log: ProvenanceLog` — Chain of custody

The `PhysicalAnchor::verify()` method performs all three checks:

```rust
pub fn verify(&self, current_rf: &[u8; 32], public_key: &PqPublicKey) -> bool {
    self.rf_fingerprint.verify(current_rf)
        && self.quantum_commitment.verify(public_key, &self.provenance_log.to_bytes(), CommitmentPhase::ClassicalOnly)
        && self.provenance_log.verify_chain()
}
```

This is the type that replaces trusted third-party attestations. When a physical claim needs to be verified ("this diamond is real", "this shipment arrived"), the verifier checks the `PhysicalAnchor` rather than trusting an oracle.

## Consequences

- **Positive**: The existing Physical Shard implementation is unchanged, maintaining the constraint of not modifying shards core logic.
- **Positive**: The `ProvenanceTracker` sidecar pattern can be applied to other shards in the future (e.g., adding provenance tracking to the Biological shard).
- **Positive**: Hardware unavailability is handled gracefully as soft failures, allowing the system to continue operating.
- **Positive**: The `PhysicalAnchor` provides a unified verification type that combines physical, cryptographic, and chain-of-custody proofs.
- **Negative**: State consistency between `ProvenanceTracker` and `PhysicalState` is not enforced by the type system — it relies on the `ShardRouter` calling both in sequence. A bug in the router could lead to state divergence.
- **Negative**: The sidecar pattern means that physical operations involve two state mutations (Physical Shard + ProvenanceTracker), which doubles the risk of partial failures.
- **Trade-off**: Using stub RF fingerprints and quantum commitments for Phase 0 means that the Binding Layer provides no real physical security until real hardware integration is complete. The stubs are deterministic placeholders that will be replaced with real measurements in later phases.

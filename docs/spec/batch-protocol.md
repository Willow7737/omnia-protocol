# Batch Protocol Specification

## Overview

The batch protocol groups events into batches for amortized validation, proof generation,
and gossip propagation. This reduces per-event CPU cost by ≥40% by amortizing overhead
across multiple events.

## Batch Schema

### ConsensusEventBatch

```rust
pub struct ConsensusEventBatch {
    pub events: Vec<Event>,           // Events in this batch
    pub proof: BatchProof,            // Merkle root proof
    pub creator: NodeId,              // Batch creator node
    pub sequence: u64,                // Monotonically increasing per creator
    pub vector_clock: VectorClock,    // Vector clock at creation time
    pub timestamp: u64,               // Creation timestamp (ms, UNIX epoch)
}
```

### BatchProof

```rust
pub struct BatchProof {
    pub merkle_root: [u8; 32],        // Merkle root of all event hashes
    pub event_count: usize,           // Number of events in the batch
    pub batch_id: [u8; 32],           // BLAKE3("omnia-batch-id" || merkle_root || event_count_le)
}
```

## Proof Computation

### Merkle Root

The Merkle root is computed over all event hashes using a binary Merkle tree
with BLAKE3 domain-separated hashing:

1. **Leaf computation**: For each event, compute `leaf = BLAKE3("omnia-batch-proof" || event.id)`
2. **Sorting**: Sort leaves lexicographically for deterministic ordering
3. **Tree construction**: Build binary Merkle tree bottom-up:
   - For each pair of siblings, compute `parent = BLAKE3("omnia-batch-proof" || left || right)`
   - For odd nodes, duplicate the last node
4. **Root**: The single remaining node is the Merkle root

### Batch ID

The batch ID binds the Merkle root and event count together:

```
batch_id = BLAKE3("omnia-batch-id" || merkle_root || event_count_as_u64_le_bytes)
```

## Wire Format for Batch Gossip

### GossipBatchMessage

```rust
pub enum GossipBatchMessage {
    Batch { batch: ConsensusEventBatch },
    BatchAck { batch_id: [u8; 32], merkle_root: [u8; 32], event_count: usize },
    BatchRequest { batch_id: [u8; 32] },
    BatchDigest { node_id: NodeId, last_sequence: u64, vector_clock: VectorClock, last_batch_event_count: usize },
}
```

### Serialization

- **Format**: postcard (deterministic, compact, no_std-compatible)
- **Compression**: Optional snappy compression for payloads > 256 bytes
- **Compression flag**: First byte of serialized data:
  - `0x00` = uncompressed
  - `0x01` = snappy compressed

### Topic

Batch messages are propagated over the GossipSub topic `omnia_batch_events`.

## Batch Validation Rules

### Full Validation

1. **Non-empty**: Batch must contain at least one event
2. **Size limit**: Batch must not exceed `MAX_BATCH_SIZE` (100) events
3. **Proof validity**: Merkle root and batch ID must match recomputed values
4. **Event validity**: Each event must pass individual validation:
   - Hash integrity (`event.verify_hash()`)
   - Signature validity (checked by event validation)
   - Timestamp sanity (not too far future, not ancient)

### Proof-Only Validation

Lightweight validation that only checks:

1. Merkle root matches recomputed root
2. Batch ID matches recomputed ID
3. Event count matches

### State Root Validation

Verifies that the batch's Merkle root matches an expected state root value.

## Batch Rejection Handling

Batches are rejected if:

| Condition            | Error                               | Action                                         |
| -------------------- | ----------------------------------- | ---------------------------------------------- |
| Empty batch          | `BatchError::EmptyBatch`            | Drop silently                                  |
| Size exceeds max     | `BatchError::BatchTooLarge`         | Drop, log warning                              |
| Merkle root mismatch | `BatchError::InvalidProof`          | Drop, log warning, increment rejection counter |
| Batch ID mismatch    | `BatchError::InvalidProof`          | Drop, log warning                              |
| Event hash invalid   | `BatchError::EventValidationFailed` | Drop, log warning                              |
| State root mismatch  | `BatchError::InvalidStateRoot`      | Drop, log warning                              |

## Integration with Consensus Flow

### BatchIngestor

The `BatchIngestor` buffers events and flushes them as batches:

1. Events are submitted via `submit(event)`
2. When the buffer reaches `flush_size`, a `ConsensusEventBatch` is automatically created
3. Alternatively, `flush()` forces batch creation (used after timeout)
4. Each batch gets a monotonically increasing sequence number per creator

### Configuration

```rust
pub struct BatchConfig {
    pub max_batch_size: usize,       // Default: 100
    pub flush_size: usize,           // Default: 50
    pub flush_timeout_ms: u64,       // Default: 100ms
}
```

### Batch CRDT Merge

The `BatchCrdtMerger` applies CRDT operations in batches:

1. All operations are validated before any are applied (atomic semantics)
2. If any operation fails validation, none are applied (rollback)
3. Supported operations: GCounter increment, OrSet add/remove, LwwRegister update

### ZK Batch Proof Circuit

The `BatchProofCircuit` verifies batch proofs within the ZK circuit:

1. Verifies Merkle path for each event in the batch
2. All paths converge to the claimed Merkle root
3. Batch ID is computed as `Poseidon(merkle_root, event_count)` in-circuit
4. Target: 100-tx batch proof aggregation

## Constants

| Constant                   | Value                | Description                         |
| -------------------------- | -------------------- | ----------------------------------- |
| `MAX_BATCH_SIZE`           | 100                  | Maximum events per batch            |
| `DEFAULT_BATCH_SIZE`       | 50                   | Default flush threshold             |
| `DEFAULT_BATCH_TIMEOUT_MS` | 100                  | Default flush timeout               |
| `MAX_CRDT_BATCH_SIZE`      | 1000                 | Maximum CRDT operations per batch   |
| `MAX_BATCH_GOSSIP_SIZE`    | 1 MiB                | Maximum serialized batch for gossip |
| `BATCH_PROOF_TARGET_SIZE`  | 100                  | Target batch size for ZK proof      |
| `GOSSIP_BATCH_TOPIC`       | `omnia_batch_events` | GossipSub topic for batch messages  |

# Layer 1: The Substrate

> 🎯 Audience: Developers
> 🔗 Context: Foundation layer enabling the network to agree on what happened without global clock time or a single authority
> 📅 Last Updated: 2026-06-24

## Core Components

### 1. VectorClock

- Tracks logical time across all known nodes
- Implements partial ordering: `happened_before`, `concurrent`, `merge`
- Enables parallel execution of causally independent events
- Located in: `substrate/src/vector_clock.rs`

### 2. Event

- Fundamental unit of the protocol
- Two-parent structure (self-parent + other-parent) forming a DAG
- Contains: vector clock, payload, cryptographic signature
- Ed25519 signatures with replay protection via nonce tracking
- Located in: `substrate/src/event.rs`

### 3. CausalGraph

- DAG storage for all events
- O(1) amortized insertion (hash map lookup) and O(1) amortized lookup
- Topological ordering for deterministic sequencing
- Concurrent event detection for parallel execution
- `state_root()` — Merkle root of the entire graph state
- `merkle_proof()` — Inclusion proof for any event
- `prune_old_events()` — Event pruning for long-term sustainability
- `unprocessed_events` queue — O(new_events) consensus processing, not O(n) full graph walk
- Located in: `substrate/src/causal_graph.rs`

### 4. CRDTs (Conflict-free Replicated Data Types)

- **GCounter**: Grow-only counter for monotonic values
- **OrSet**: Observed-remove set with add-wins semantics
- **LwwRegister**: Last-write-wins register for single values
- Located in: `substrate/src/crdt/`

For mathematical convergence proofs, see [crdt-convergence.md](./crdt-convergence.md).

### 5. GossipProtocol

- Epidemic event propagation across the network
- Built on libp2p (QUIC transport + GossipSub + mDNS discovery)
- Kademlia DHT for wide-area peer discovery
- AutoNAT for NAT type detection, relay for NAT traversal, DCutr for direct upgrades
- Snappy compression for messages >256 bytes
- Gossip-about-gossip pattern (inspired by Hashgraph)
- Bandwidth-efficient: only missing events transmitted
- Located in: `omnia-network/src/gossip.rs`, `omnia-network/src/network.rs` (re-exported by substrate)

### 6. ConsensusEngine

- BFT finality mechanism running on top of the causal graph
- Witness/fame/commit model (inspired by Hashgraph + AlephBFT)
- Optimistic confirmation for low-latency finality
- Processes only new (unprocessed) events, not the entire graph each round
- VRF-based leader selection with stake weighting
- Consensus state persisted across restarts via `RedbConsensusStore`
- Located in: `substrate/src/consensus.rs`

v0.1.69 audit fix (H-12): `SubstrateConfig::try_new()` propagates invalid `OMNIA_CONSENSUS_SEED` errors instead of silently falling back to a random seed (which would fork the node). `Substrate::new` now panics loudly on persistence failure instead of silently falling back to in-memory state.

For pipeline and queue design, see [pipeline-design.md](./pipeline-design.md).

### 7. SlashingEngine

- Three offense types: Equivocation (500pts), LivenessViolation (100pts), InvalidAttestation (300pts)
- Gradual slashing with 3-tier model: Warning → Jail → Ejection (ADR-011)
- Persistent state via `RedbSlashingStore` with snapshot-and-rollback pattern
- `SlashingUndoManager` for governance-based reversal
- Located in: `substrate/src/slashing.rs`, `substrate/src/slashing_undo.rs`

v0.1.69 audit fix (M-10): `RedbSlashingStore::open()` recovers from corrupt DB by renaming to `.corrupt` and creating fresh.

### 8. State Management

- `state_root()` — Merkle root of the entire graph state
- `merkle_proof()` — Inclusion proof for any event
- `prune_old_events()` — Event pruning for long-term sustainability
- `StateSnapshot` — Serialized snapshot with integrity verification
- Fast-sync protocol for new nodes (BLAKE3 checkpoints, supermajority agreement)
- Located in: `substrate/src/snapshot.rs`, `omnia-network/src/fast_sync.rs` (re-exported by substrate)

### 9. KeyStore and Crypto

- `EncryptedKeyStore` with AES-256-GCM + HKDF-SHA256 encryption
- BIP-39 mnemonic support with SLIP-0010 HD key derivation
- Key rotation with cryptographic proof (`KeyRotationProof`)
- BLAKE3 domain separation for all hash contexts
- `CryptoProfile` abstraction for hash, signature, VRF, and ZK schemes
- Located in: `substrate/src/keystore.rs`, `substrate/src/crypto.rs`, `substrate/src/blake3_domain.rs`

### 10. BLS Threshold Signatures and DKG

- BLS12-381 signature aggregation for N-to-1 verification
- `ThresholdKeyManager` for t-of-n key sharing
- `DkgSession` state machine with Feldman VSS-based DKG
- AES-256-GCM encrypted share packages with associated data
- Located in: `substrate/src/bls.rs`, `substrate/src/threshold.rs`

### 11. VRF and Leader Selection

- V1: Ed25519 signature + BLAKE3 derivation (legacy)
- V2: ECVRF with Fiat-Shamir + Ed25519 signatures (standard, target)
- Stake-weighted leader selection via `compute_leader()`
- Located in: `substrate/src/vrf.rs`

See [ADR-012](../reference/adr-index.md#adr-012-vrf-construction-choice) for the VRF construction decision.

## Design Decisions

### Why Causal Consistency over Blockchain?

| Property    | Blockchain          | Causal Graph (Omnia)                                  |
| ----------- | ------------------- | ----------------------------------------------------- |
| Ordering    | Total (sequential)  | Partial (parallel)                                    |
| Throughput  | ~100-1000 TPS       | ~7,190 events/sec (v0.1.48 historical; v0.1.68 baseline: 12,000 ops/s) (single-node measured, synchronous) |
| Latency     | ~12s block time     | Not yet benchmarked at scale                          |
| Concurrency | None (single chain) | Automatic (DAG)                                       |
| Finality    | Probabilistic       | Deterministic (BFT)                                   |

### Consensus Model: Hybrid Approach

After researching Hashgraph, IOTA Tangle, and AlephBFT, we chose a hybrid:

| Approach         | Pros for Omnia                                  | Cons                                       |
| ---------------- | ----------------------------------------------- | ------------------------------------------ |
| Pure Hashgraph   | Virtual voting is elegant; proven throughput    | Patented; requires complete history        |
| Pure IOTA        | Simple tip selection; feeless                   | FPC finality not as strong as BFT          |
| Pure AlephBFT    | Strong BFT guarantees; leaderless               | Committee-based, not fully permissionless  |
| **Omnia Hybrid** | Causal ordering + CRDT convergence + simple BFT | Novel combination — needs thorough testing |

- **Structure**: Hashgraph-like DAG with two-parent events
- **Ordering**: Vector clock-based partial ordering
- **Finality**: AlephBFT-inspired supermajority witness
- **State**: CRDT semantics for deterministic convergence

See `substrate/RESEARCH.md` for detailed comparative analysis.

## Performance

⚠️ The `CausalGraph` uses an `unprocessed_events` queue so that consensus only processes new events each round — O(new_events) processing. **Actual measured single-node throughput is ~7,190 events/sec (v0.1.48 historical; v0.1.68 baseline: 12,000 ops/s)** (synchronous; see [benchmark-gates.md](../reference/benchmark-gates.md) for full benchmark results). Multi-node distributed throughput will be lower due to network latency and BFT consensus requirements. CausalGraph insertion is O(1) amortized via hash map operations, not O(1) guaranteed.

## Testing Strategy

Every module has comprehensive unit tests. The critical integration test simulates:

- 3+ nodes in a network
- Each node creates events
- Events propagate via gossip
- CRDT state converges to identical values on all nodes
- Multi-node BFT finality validated (4 nodes, all tests passing)
- All tests in: `substrate/tests/`

---

🔙 **Back**: [architecture/](./) | 🔄 **Related**: [pipeline-design.md](./pipeline-design.md)
🚀 **Next**: [layer-2-shards.md](./layer-2-shards.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

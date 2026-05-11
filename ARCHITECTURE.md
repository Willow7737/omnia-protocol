# Omnia Protocol — Architecture Documentation

[← Back to README](./README.md) | [Governance](./docs/GOVERNANCE.md) | [FAQ](./docs/FAQ.md)

## Overview

Omnia Protocol is a universal coordination layer that replaces trust with mathematics. It uses causal consistency (causal graphs, vector clocks, CRDTs) instead of sequential blockchains to achieve parallel transaction processing.

## The Five Layers

```
┌─────────────────────────────────────────┐
│  LAYER 5: Economics (UBC, Governance)   │
├─────────────────────────────────────────┤
│  LAYER 4: Identity (DIDs, Recovery)     │
├─────────────────────────────────────────┤
│  LAYER 3: Binding (Physical Anchors)    │
├─────────────────────────────────────────┤
│  LAYER 2: Domain Shards (5 lanes)       │
├─────────────────────────────────────────┤
│  LAYER 1: Substrate (Causal Graph)      │ ← Current phase
└─────────────────────────────────────────┘
```

## Layer 1: The Substrate (Current Implementation)

### Core Components

#### 1. VectorClock
- Tracks logical time across all known nodes
- Implements partial ordering: `happened_before`, `concurrent`, `merge`
- Enables parallel execution of causally independent events
- Located in: `substrate/src/vector_clock.rs`

#### 2. Event
- Fundamental unit of the protocol
- Two-parent structure (self-parent + other-parent) forming a DAG
- Contains: vector clock, payload, cryptographic signature
- Located in: `substrate/src/event.rs`

#### 3. CausalGraph
- DAG storage for all events
- O(1) insertion and lookup
- Topological ordering for deterministic sequencing
- Concurrent event detection for parallel execution
- Located in: `substrate/src/causal_graph.rs`

#### 4. CRDTs (Conflict-free Replicated Data Types)
- **GCounter**: Grow-only counter for monotonic values
- **OrSet**: Observed-remove set with add-wins semantics
- **LwwRegister**: Last-write-wins register for single values
- Located in: `substrate/src/crdt/`

#### 5. GossipProtocol
- Epidemic event propagation across the network
- Gossip-about-gossip pattern (inspired by Hashgraph)
- Bandwidth-efficient: only missing events transmitted
- Located in: `substrate/src/gossip.rs`

#### 6. ConsensusEngine
- BFT finality mechanism running on top of the causal graph
- Witness/fame/commit model (inspired by Hashgraph + AlephBFT)
- Optimistic confirmation for low-latency finality
- Located in: `substrate/src/consensus.rs`

### Design Decisions

#### Why Causal Consistency over Blockchain?
| Property | Blockchain | Causal Graph (Omnia) |
|----------|-----------|---------------------|
| Ordering | Total (sequential) | Partial (parallel) |
| Throughput | ~100-1000 TPS | 10,000+ TPS target |
| Latency | ~12s block time | ~1-5s finality |
| Concurrency | None (single chain) | Automatic (DAG) |
| Finality | Probabilistic | Deterministic (BFT) |

#### Consensus Model: Hybrid Approach
After researching Hashgraph, IOTA Tangle, and AlephBFT, we chose a hybrid:
- **Structure**: Hashgraph-like DAG with two-parent events
- **Ordering**: Vector clock-based partial ordering
- **Finality**: AlephBFT-inspired supermajority witness
- **State**: CRDT semantics for deterministic convergence

See `substrate/RESEARCH.md` for detailed comparative analysis.

### Testing Strategy

Every module has comprehensive unit tests. The critical integration test simulates:
- 3+ nodes in a network
- Each node creates events
- Events propagate via gossip
- CRDT state converges to identical values on all nodes
- All tests in: `substrate/tests/`

### Performance Targets

| Metric | Target | Status |
|--------|--------|--------|
| Throughput | 10,000+ TPS | In progress |
| Latency | 1-5 seconds | In progress |
| Convergence | 100% CRDT merge | Verified |
| Fault tolerance | <1/3 Byzantine | Designed |

## Integration Points (Future Layers)

### Layer 2: Domain Shards
- Each shard runs its own CRDT state
- Cross-shard messaging via the Substrate's causal graph
- Shard router assigns transactions to shards by domain

### Layer 3: Binding
- Physical anchors linked to events in the causal graph
- RF fingerprinting + quantum-resistant commitments
- Supply chain provenance as append-only CRDT log

### Layer 4: Identity
- DIDs (`did:omnia:`) stored in the causal graph
- Social recovery via Shamir's Secret Sharing
- AI agent identities with capability flags

### Layer 5: Economics
- UBC tokens tracked as non-transferable balances (CRDT)
- Quota system with monthly reset
- Quadratic voting recorded in the causal graph

## Security Considerations

1. **No unwrap() in production paths** — all errors handled explicitly
2. **Constant-time crypto** — where needed for side-channel resistance
3. **Byzantine fault tolerance** — <1/3 malicious nodes tolerated
4. **No single point of failure** — leaderless design
5. **Deterministic convergence** — CRDTs guarantee identical state

## References

- Baird, L. "The Swirlds Hashgraph Consensus Algorithm" (2016)
- Popov, S. "The Tangle" IOTA Whitepaper (2018)
- AlephBFT Documentation: https://docs.alephzero.org/
- `crdts` crate: https://docs.rs/crdts
- Automerge: https://automerge.org/

## License

CC0 1.0 Universal — Public Domain Dedication

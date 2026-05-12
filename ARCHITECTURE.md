# Omnia Protocol — Architecture Documentation

[← Back to README](./README.md) | [Governance](./docs/GOVERNANCE.md) | [FAQ](./docs/FAQ.md)

## Overview

Omnia Protocol is a universal coordination layer that replaces trust with mathematics. It uses causal consistency (causal graphs, vector clocks, CRDTs) instead of sequential blockchains to achieve parallel transaction processing. The protocol is **settlement-agnostic** — it can settle on any L1 that provides data availability and proof verification.

## The Six Layers

```
┌─────────────────────────────────────────┐
│  LAYER 5: Economics (UBC, Governance)   │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 4: Identity (DIDs, Shamir, Bio) │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 3: Binding (Provenance, RF, QC) │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 2: Domain Shards (6 shards)     │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 1: Substrate (Causal Graph)     │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  PHASE 0: ZK-Rollup (Settlement Layer) │ ✅ IMPLEMENTED
└─────────────────────────────────────────┘
```

All five core layers are implemented and tested. Phase 0 (ZK-rollup settlement) has an Ethereum adapter and stubs for other L1s.

---

## Layer 1: The Substrate ✅

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
- Ed25519 signatures with replay protection via nonce tracking
- Located in: `substrate/src/event.rs`

#### 3. CausalGraph
- DAG storage for all events
- O(1) amortized insertion (hash map lookup) and O(1) amortized lookup
- Topological ordering for deterministic sequencing
- Concurrent event detection for parallel execution
- `state_root()` — Merkle root of the entire graph state
- `merkle_proof()` — Inclusion proof for any event
- `prune_old_events()` — Event pruning for long-term sustainability
- `unprocessed_events` queue — O(new_events) consensus processing, not O(n) full graph walk
- Located in: `substrate/src/causal_graph.rs`

#### 4. CRDTs (Conflict-free Replicated Data Types)
- **GCounter**: Grow-only counter for monotonic values
- **OrSet**: Observed-remove set with add-wins semantics
- **LwwRegister**: Last-write-wins register for single values
- Located in: `substrate/src/crdt/`

#### 5. GossipProtocol
- Epidemic event propagation across the network
- Built on libp2p (QUIC transport + GossipSub + mDNS discovery)
- Gossip-about-gossip pattern (inspired by Hashgraph)
- Bandwidth-efficient: only missing events transmitted
- Located in: `substrate/src/gossip.rs`

#### 6. ConsensusEngine
- BFT finality mechanism running on top of the causal graph
- Witness/fame/commit model (inspired by Hashgraph + AlephBFT)
- Optimistic confirmation for low-latency finality
- Processes only new (unprocessed) events, not the entire graph each round
- Located in: `substrate/src/consensus.rs`

### Design Decisions

#### Why Causal Consistency over Blockchain?

| Property | Blockchain | Causal Graph (Omnia) |
|----------|-----------|---------------------|
| Ordering | Total (sequential) | Partial (parallel) |
| Throughput | ~100-1000 TPS | ⚠️ Not yet benchmarked at scale |
| Latency | ~12s block time | ⚠️ Not yet benchmarked at scale |
| Concurrency | None (single chain) | Automatic (DAG) |
| Finality | Probabilistic | Deterministic (BFT) |

#### Consensus Model: Hybrid Approach

After researching Hashgraph, IOTA Tangle, and AlephBFT, we chose a hybrid:

| Approach | Pros for Omnia | Cons |
|----------|---------------|------|
| Pure Hashgraph | Virtual voting is elegant; proven throughput | Patented; requires complete history |
| Pure IOTA | Simple tip selection; feeless | FPC finality not as strong as BFT |
| Pure AlephBFT | Strong BFT guarantees; leaderless | Committee-based, not fully permissionless |
| **Omnia Hybrid** | Causal ordering + CRDT convergence + simple BFT | Novel combination — needs thorough testing |

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

### Performance Notes

⚠️ The `CausalGraph` uses an `unprocessed_events` queue so that consensus only processes new events each round — O(new_events) processing. **Full throughput benchmarking (targeting 10,000+ TPS) has not yet been performed.** CausalGraph insertion is O(1) amortized via hash map operations, not O(1) guaranteed.

---

## Layer Integration ✅

### Layer 2: Domain Shards ✅
- 6 shards: Financial, Identity, Physical, Computational, Biological, Economics
- `EventProcessor` trait — each shard implements this to process events
- `ShardRouter` dispatches events to the correct shard by domain
- Cross-shard messaging via the Substrate's causal graph with causality proofs
- Replay protection via per-creator nonce tracking (`last_nonces` in `ShardRouter`)
- ⚠️ FinancialShard uses strict causal ordering (not CRDTs) for balance consistency

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│  Financial  │    │  Identity   │    │  Physical   │
│   Shard ✅  │    │   Shard ✅  │    │   Shard ✅  │
└──────┬──────┘    └──────┬──────┘    └──────┬──────┘
       │                  │                  │
       └────────┬─────────┴────────┬────────┘
                │                  │
         ┌──────┴──────┐    ┌──────┴──────┐
         │ Computational│    │  Biological │
         │   Shard ✅   │    │   Shard ✅  │
         └──────┬──────┘    └──────┬──────┘
                │                  │
                └────────┬─────────┘
                         │
                  ┌──────┴──────┐
                  │  Economics  │
                  │   Shard ✅  │
                  └──────┬──────┘
                         │
                  ┌──────┴──────┐
                  │  ShardRouter│
                  │ (EventProc) │
                  └──────┬──────┘
                         │
                  ┌──────┴──────┐
                  │  Substrate  │
                  │ (CausalGraph│
                  └─────────────┘
```

### Layer 3: Binding ✅
- `ProvenanceLog` — append-only CRDT log for supply chain tracking
- `PhysicalAnchor` — combines RF fingerprinting stub, quantum commitment stub, and provenance
- `ProvenanceTracker` — full create/transfer/verify/destroy lifecycle
- ⚠️ RF fingerprinting is a stub (Hamming distance comparison); real implementation requires SDR hardware (HackRF/USRP)
- ⚠️ Quantum commitments are a stub (hybrid classical + PQC placeholder); real implementation requires CRYSTALS-Dilithium
- Physical time anchors (previously described as "Gravitational Timestamps") are not implemented. The protocol currently relies on logical time via vector clocks.

### Layer 4: Identity ✅
- `did:omnia:` method with full validation
- Shamir's Secret Sharing over GF(256) for social recovery
- Privacy-preserving biometric anchors: `BLAKE3(salt || template)` — template never stored in cleartext
- `AgentIdentity` with 5 capability types for AI agent identities
- Social recovery with configurable guardian threshold

### Layer 5: Economics ✅
- UBC tokens tracked as non-transferable (soulbound) balances
- `QuotaSystem` with epoch-based advancement and monthly reset
- Quadratic voting with exponential reputation decay (implemented)
- 📋 Conviction voting and delegation are planned for Phase 1 (not yet implemented)
- ⚠️ Proof-of-useful-work stubs (3 work types defined, not production-ready)

### Phase 0: ZK-Rollup ✅
- Settlement-agnostic architecture via `SettlementLayer` trait
- Ethereum adapter with Solidity contract (OmniaRollup.sol)
- Stubs for Bitcoin, Solana, Celestia adapters
- L2 operator with batch builder
- ⚠️ ZK circuit stub — currently uses hash chain, not a full R1CS circuit (arkworks integration is the production target)
- Merkle state root and inclusion proofs
- Event pruning (`prune_old_events`) for long-term state sustainability

```
┌───────────────────────────────────────────────┐
│            Settlement-Agnostic ZK-Rollup      │
├───────────────┬───────────────┬───────────────┤
│  Ethereum ✅  │ Bitcoin 🔄    │  Solana 🔄    │
│  (OmniaRollup │  (stub)       │  (stub)       │
│   .sol)       │               │               │
├───────────────┴───────────────┴───────────────┤
│           SettlementLayer Trait                │
├───────────────────────────────────────────────┤
│         L2 Operator + Batch Builder           │
├───────────────────────────────────────────────┤
│      ZK Circuit (hash chain stub ⚠️)          │
├───────────────────────────────────────────────┤
│    Merkle State Root + Inclusion Proofs        │
├───────────────────────────────────────────────┤
│         Event Pruning (sustainability)         │
└───────────────────────────────────────────────┘
```

---

## 🛡️ Security Considerations

| # | Mechanism | Status |
|---|-----------|--------|
| 1 | No `unwrap()` in production paths — all errors handled explicitly | ✅ Implemented |
| 2 | Replay protection — nonce tracking in CausalGraph and ShardRouter | ✅ Implemented |
| 3 | Constant-time crypto — where needed for side-channel resistance | ✅ Implemented |
| 4 | Byzantine fault tolerance — <1/3 malicious nodes tolerated | ✅ Designed |
| 5 | No single point of failure — leaderless design | ✅ Designed |
| 6 | Deterministic convergence — CRDTs guarantee identical state | ✅ Implemented |
| 7 | State commitments — `state_root()` and `merkle_proof()` | ✅ Implemented |
| 8 | Sustainability — `prune_old_events()` prevents unbounded growth | ✅ Implemented |
| 9 | Economic security (slashing, staking) | 🌑 Not yet implemented |
| 10 | Post-quantum cryptography (Dilithium) | 🔄 Stub |

---

## References

- Baird, L. "The Swirlds Hashgraph Consensus Algorithm" (2016)
- Popov, S. "The Tangle" IOTA Whitepaper (2018)
- AlephBFT Documentation: https://docs.alephzero.org/
- `crdts` crate: https://docs.rs/crdts
- Automerge: https://automerge.org/

## License

CC0 1.0 Universal — Public Domain Dedication

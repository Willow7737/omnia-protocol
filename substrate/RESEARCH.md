# 🧪 Layer 1: The Substrate — Research Document

[← Back to Architecture](../docs/architecture/full-spec.md)

## Comparative Analysis of Causal Graph Consensus Implementations

### 1. Hashgraph (Hedera)

**Gossip-about-Gossip + Virtual Voting**

- Each event references two parents: self-parent (creator's latest event) and other-parent (received event from another node)
- Events contain: transactions, timestamp, two parent hashes, creator signature
- Virtual voting eliminates explicit vote messages — each node independently calculates votes from its local DAG view
- Round-based: events advance rounds when they can "strongly see" >2n/3 witnesses from previous round
- Witness fame determined through virtual voting across subsequent rounds
- **Throughput**: Up to 250k TPS (benchmark), ~10k TPS mainnet
- **Finality**: ~3-5 seconds
- **Trade-offs**: Patented algorithm (US Patent), closed council governance, requires complete DAG history for virtual voting

### 2. IOTA Tangle

**DAG with MCMC Tip Selection**

- Each transaction approves 2 previous tips (unconfirmed transactions)
- Tips selected via Markov Chain Monte Carlo random walk biased by cumulative weight
- Structure: G=(V,E) where vertices are transactions, edges are approvals
- **Throughput**: Theoretically unbounded (parallel attachment), ~1k TPS practical
- **Finality**: Variable, improved with Coordicide (IOTA 2.0) using Fast Probabilistic Consensus (FPC)
- **Trade-offs**: Tip selection parameter α creates fairness vs security tension; parasitic chain attacks possible; required Coordinator until IOTA 2.0

**Tip Selection Algorithms Compared:**
| Algorithm | Security | Fairness | Computational Cost |
|-----------|----------|----------|-------------------|
| MCMC (large α) | High | Low (orphans possible) | High (O(n²)) |
| Uniform Random | Low (double-spend) | High | Low (O(n)) |
| G-IOTA | High | High | Higher (3 walks/tx) |
| E-IOTA | High | High | Reduced (~10% fewer walks) |

### 3. Aleph Zero (AlephBFT)

**Asynchronous BFT on DAG**

- DAG serves as intermediate data structure, not the final chain
- Leaderless, asynchronous Byzantine Fault Tolerant
- Rotating committee of validators
- Combines PoS with DAG for ordering
- **Throughput**: 89,600 TPS (Go benchmark), Rust implementation lower but production-grade
- **Finality**: ~416ms (Go), sub-second in production
- **Trade-offs**: Requires reliable broadcast for non-equivocation; committee rotation adds complexity
- **Implementation**: `aleph-bft` crate available (Rust), well-documented API

### 4. CRDT Implementations

**Automerge 2.0 (Rust)**
- Rewritten from JS to Rust for performance
- Document-based JSON-like CRDT
- Hundreds of times faster than JS version
- Used in collaborative editing applications

**`crdts` crate (Rust)**
- Pure Rust, serializable CRDTs
- Provides: VClock, GCounter, Orswot (OR-Set), LWWReg, MVReg, Map, List
- Well-tested with quickcheck property testing
- Actor-based identification for vector clocks

**Diamond Types**
- Experimental, cutting-edge performance
- Novel algorithms for smaller memory footprint
- Pre-1.0, rapidly evolving

### 5. Causal Graph Libraries

**`causal-graph` (JavaScript)**
- Run-length encoded DAG for operation-based CRDTs
- Each entry: `(agent, seq)` ID + list of parent entries
- Supports: `versionContainsLV`, `diff`, `compareVersions`, `findDominators`
- Vector clock-based remote comparison and serialization

## Design Decisions for Omnia Substrate

### Consensus Model: Hybrid Approach

After analyzing all options, Omnia Layer 1 uses a **hybrid causal consensus**:

1. **Core structure**: Hashgraph-like DAG with two-parent events (self-parent + other-parent)
2. **Ordering**: Vector clock-based partial ordering for causal independence detection
3. **Finality**: AlephBFT-inspired — collect >2n/3 acknowledgments for confirmation
4. **State convergence**: CRDT semantics ensure state merges deterministically

### Why This Hybrid?

| Approach | Pros for Omnia | Cons |
|----------|---------------|------|
| Pure Hashgraph | Virtual voting is elegant; proven throughput | Patented; requires complete history; virtual voting complex to implement correctly |
| Pure IOTA | Simple tip selection; feeless; IoT-friendly | FPC finality not as strong as BFT; tip selection parameter tuning difficult |
| Pure AlephBFT | Strong BFT guarantees; leaderless; async | Committee-based (not fully permissionless); higher complexity |
| **Omnia Hybrid** | Causal ordering + CRDT convergence + simple BFT finality | Novel combination — requires thorough testing |

### Key Differentiators

1. **No global clock**: All ordering is causal (happened-before relationships)
2. **Parallel execution**: Causally independent transactions execute in parallel automatically
3. **CRDT state**: Account state uses CRDTs — no consensus needed for independent operations
4. **Modular finality**: Pluggable finality gadget (can swap between optimistic confirmation and full BFT)

### Performance Targets

- **Target throughput**: 10,000+ TPS (conservative; Hashgraph shows 250k possible)
- **Target latency**: 1-5 seconds for finality
- **Concurrency**: Causally independent transactions processed in parallel with no coordination

---

## 🧪 Implementation Results

| Research Claim | Implementation Status |
|----------------|----------------------|
| Hashgraph-like DAG with two-parent events | ✅ Implemented |
| Vector clock-based partial ordering | ✅ Implemented |
| AlephBFT-inspired BFT finality | ✅ Implemented (simplified for permissionless model) |
| CRDT semantics for state convergence | ✅ Implemented (GCounter, OrSet, LWWRegister) |
| 10,000+ TPS target | ⚠️ Not yet benchmarked |
| 1-5 second finality | ⚠️ Not yet benchmarked |
| O(new_events) consensus processing | ✅ Achieved via unprocessed queue |

### What Worked ✅

**Hashgraph-like DAG with two-parent events** — The two-parent event structure (self-parent + other-parent) proved to be an effective way to represent causality. Events naturally form a DAG that captures all happened-before relationships, and the structure is straightforward to implement and reason about.

**Vector clocks for partial ordering** — Vector clocks provide an efficient mechanism for detecting concurrent vs. causally ordered events. The `happened_before`, `concurrent`, and `merge` operations are all O(n) in the number of known nodes, which is acceptable for the expected network size.

**CRDT state convergence** — Using CRDTs (GCounter, OrSet, LWWRegister) for state management eliminates the need for consensus on causally independent state updates. The merge operation is deterministic, so all nodes converge to identical state after receiving the same set of events, regardless of order.

**O(new_events) consensus processing** — By maintaining an `unprocessed_events` queue in the CausalGraph, the consensus engine only processes new events each round rather than walking the entire graph. This is a significant performance improvement over approaches that require full graph traversal for each consensus round.

**Replay protection via nonce tracking** — Simple per-creator nonce tracking prevents replay attacks without adding significant complexity. Both the CausalGraph and ShardRouter maintain nonce maps, ensuring that each event from a given creator is processed exactly once.

### What Was Adapted 🔄

**AlephBFT finality simplified for permissionless model** — AlephBFT's committee-based finality was adapted to work in a more permissionless setting. The full rotating committee mechanism was simplified to a supermajority witness model, which provides BFT guarantees without requiring the committee management overhead. This is a practical trade-off for Phase 0; the full AlephBFT committee model can be reintroduced when a validator network is established.

**Financial shard uses strict causal ordering** — The FinancialShard does not use CRDTs for balance management. Instead, it uses strict causal ordering to ensure that balance debits always happen before corresponding credits. This is because financial balances cannot be modeled as CRDTs (you cannot merge two debits of the same balance independently). The causal ordering guarantee from the substrate ensures correctness.

### Performance Notes ⚠️

**O(new_events) processing achieved** — The `unprocessed_events` queue successfully limits consensus processing to only new events per round. However, the system has **not yet been benchmarked at scale**. The 10,000+ TPS target remains unverified. Specific performance characteristics (latency, throughput under load, memory usage at scale) require dedicated benchmarking with realistic workloads.

**CausalGraph insertion is O(1) amortized** — Insertion uses hash map operations, giving O(1) amortized performance. This is not O(1) guaranteed — hash map operations can occasionally be O(n) due to resizing. In practice, this has not been a performance concern.

**No real-world network testing** — All testing has been done in simulated environments. Real-world network conditions (latency spikes, partitions, adversarial behavior) have not been tested.

---

## 📚 References

- Baird, L. "The Swirlds Hashgraph Consensus Algorithm" (2016)
- Popov, S. "The Tangle" IOTA Whitepaper (2018)
- AlephBFT Consensus Documentation: https://docs.alephzero.org/aleph-zero/explore/alephbft-consensus
- `crdts` crate: https://docs.rs/crdts
- `aleph-bft` crate: https://github.com/Cardinal-Cryptography/aleph-bft
- automerge: https://automerge.org/
- DAG Meets BFT: https://decentralizedthoughts.github.io/2022-06-28-DAG-meets-BFT/

# Architecture Documentation
> 🎯 Audience: Developers
> 🔗 Context: Index for all architecture documents describing the Omnia Protocol's layered design
> 📅 Last Updated: 2026-05-20

## Layer Overview

Omnia Protocol is a five-layer distributed system designed to enable trustless coordination at global and interplanetary scales.

```
┌─────────────────────────────────────────┐
│  LAYER 5: Economics (UBC, Governance)   │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 4: Identity (DIDs, Shamir, Bio) │ ✅ IMPLEMENTED (in shards)
├─────────────────────────────────────────┤
│  LAYER 3: Binding (Provenance, RF, QC) │ ✅ IMPLEMENTED (QC real, RF stub)
├─────────────────────────────────────────┤
│  LAYER 2: Domain Shards (6 shards)     │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 1: Substrate (Causal Graph)     │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  PHASE 0: ZK-Rollup (Settlement Layer) │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  NODE BINARY (CLI + REST API)          │ ✅ IMPLEMENTED
└─────────────────────────────────────────┘
```

## Architecture Documents

| Document | Layer | Key Topics |
|----------|-------|------------|
| [layer-1-substrate.md](layer-1-substrate.md) | Layer 1 | VectorClock, Event, CausalGraph, CRDTs, Gossip, ConsensusEngine |
| [layer-2-shards.md](layer-2-shards.md) | Layer 2 | 6 domain shards, ShardRouter, cross-shard messaging, fee enforcement |
| [layer-3-binding.md](layer-3-binding.md) | Layer 3 | ProvenanceLog, PhysicalAnchor, PQC signatures, key rotation |
| [layer-4-identity.md](layer-4-identity.md) | Layer 4 | DIDs, Shamir's Secret Sharing, biometric anchors, AI agents |
| [layer-5-economics.md](layer-5-economics.md) | Layer 5 | UBC, quota, quadratic voting, reputation decay, slashing |
| [zk-rollup-settlement.md](zk-rollup-settlement.md) | Phase 0 | Settlement-agnostic ZK-rollup, Groth16, Poseidon, Ethereum adapter |
| [trait-boundaries.md](trait-boundaries.md) | Cross-cutting | EventProcessor, SettlementLayer, Shard trait contracts, ADRs 001–007 |
| [pipeline-design.md](pipeline-design.md) | Cross-cutting | Consensus pipeline, mempool, leader selection, queue invariants |
| [crdt-convergence.md](crdt-convergence.md) | Cross-cutting | CRDT convergence proofs for GCounter, OrSet, LWWRegister |
| [vector-clock-reconciliation.md](vector-clock-reconciliation.md) | Vector clock reconciliation strategy |
| [consensus-queue.md](consensus-queue.md) | Consensus queue invariants |
| [full-spec.md](full-spec.md) | Comprehensive architecture specification | | Consensus queue invariants | | Cross-cutting | Vector clock reconciliation strategy, partition recovery |

## Workspace Crates

| Crate | Purpose | Tests |
|-------|---------|-------|
| `substrate/` | Causal graph, consensus, gossip, crypto, CRDTs, slashing (redb) | 454+ |
| `shards/` | 6 domain shards + cross-shard messaging | 62+ |
| `binding/` | Provenance log, RF stub, hybrid PQC signatures | 61+ |
| `economics/` | UBC token, quota, governance, useful work | 58+ |
| `omnia-adapters/` | ZK-rollup (arkworks R1CS + Groth16 + Merkle), Ethereum adapter | 129+ |
| `node/` | Binary entrypoint, REST API, health/metrics | 30+ |
| `chaos-tests/` | Network partitions, crash recovery, byzantine, message loss | ~15 scenarios |

**Total: 800+ lib tests + chaos/integration tests, all passing.**

---
🔙 **Back**: [docs/](../) | 🔄 **Related**: [trait-boundaries.md](trait-boundaries.md)
🚀 **Next**: [layer-1-substrate.md](layer-1-substrate.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

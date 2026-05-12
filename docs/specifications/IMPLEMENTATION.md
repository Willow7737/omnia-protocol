# Omnia Protocol Implementation Guide

> **⚠️ This document describes the theoretical implementation plan. The actual codebase uses a custom Rust implementation, not the Substrate framework. No REST API exists yet. All current interaction is via the Rust crate API.**

## Current Implementation

### Actual Technology Stack

```
Language: Custom Rust (not Parity Substrate)
- Type-safe, memory-safe
- Excellent for cryptographic code
- No JavaScript/Python client libraries exist

Core Crates:
- substrate/: Causal graph, consensus, gossip, crypto, CRDTs
- shards/: 6 domain shards + cross-shard messaging
- binding/: Provenance log, RF stub, quantum commitment stub
- economics/: UBC token, quota, governance, useful work
- zk/: Settlement-agnostic ZK-rollup, Ethereum adapter

Networking:
- libp2p (QUIC + GossipSub + mDNS)

Cryptography:
- Ed25519 signatures
- BLAKE3 hashing
- Shamir's Secret Sharing over GF(256)

State Management:
- CRDTs (GCounter, OrSet, LWWRegister)
- Merkle state root + inclusion proofs
- Event pruning for sustainability
```

### What's Actually Implemented

- Causal graph consensus with vector clock ordering
- 6 domain shards with cross-shard messaging
- Provenance tracking (full lifecycle)
- DID method (`did:omnia:`) with validation
- Shamir's Secret Sharing for social recovery
- Biometric anchors (BLAKE3-based)
- AI agent identity
- UBC token (soulbound quota)
- Quadratic voting with exponential decay
- Settlement-agnostic ZK-rollup architecture
- Ethereum adapter with Solidity contract

### What's a Stub

- ZK circuit (hash chain, not full R1CS)
- RF fingerprinting (Hamming distance, needs SDR hardware)
- Quantum commitments (hybrid placeholder, needs Dilithium)
- Proof-of-useful-work (3 work types defined, not production)

### What Doesn't Exist

- REST API (all interaction is via Rust library)
- Mobile wallet
- JavaScript/Python client libraries
- Validator network (single-node operator for Phase 0)
- Fee mechanism
- Slashing mechanism

---

## Phase 0: The Seed (Months 0-18)

### Objective

Prove the concept works with a functional prototype that demonstrates:
- Causal graph consensus
- Self-sovereign identity
- Universal Basic Compute
- Settlement-agnostic ZK-rollup

### Development Milestones

#### Milestone 1: Foundation ✅ Completed
- Causal graph with vector clock ordering
- CRDT state convergence (GCounter, OrSet, LWWRegister)
- BFT finality mechanism
- libp2p gossip protocol
- Ed25519 signatures with replay protection

#### Milestone 2: Domain Shards ✅ Completed
- 6 domain shards: Financial, Identity, Physical, Computational, Biological, Economics
- Shard router with automatic dispatch
- Cross-shard messaging with causality proofs
- Replay protection via per-creator nonce tracking

#### Milestone 3: Binding & Identity ✅ Completed
- Provenance log (append-only CRDT)
- ProvenanceTracker lifecycle (create/transfer/verify/destroy)
- `did:omnia:` method with validation
- Shamir's Secret Sharing over GF(256)
- Biometric anchors (BLAKE3(salt || template))
- AI agent identity with capability types

#### Milestone 4: Economics ✅ Completed
- UBC token (soulbound, monthly quota)
- Quota system with epoch advancement
- Quadratic voting with exponential decay
- Proof-of-useful-work stubs

#### Milestone 5: ZK-Rollup 🏗️ In Progress
- Settlement-agnostic architecture (SettlementLayer trait) ✅
- Ethereum adapter with Solidity contract ✅
- L2 operator with batch builder ✅
- Merkle state root + inclusion proofs ✅
- Event pruning ✅
- Full ZK circuit (arkworks R1CS) — Not yet started

---

## Phase 1: The Root (Years 1-2) — Planned 📋

*The following describes planned work. It has not been started.*

### Objective

Build standalone capabilities and expand the protocol's reach.

### Planned Work

- Full ZK circuit implementation (arkworks R1CS)
- Real PQC signatures (CRYSTALS-Dilithium)
- Real RF fingerprinting (SDR hardware integration)
- Fee mechanism design and implementation
- Mobile wallet
- Validator network (multi-node)
- Conviction voting and delegation
- Slashing mechanism

---

## Phase 2: The Trunk (Years 3-5) — Aspirational 🔮

*The following describes a long-term vision. It is not currently being developed.*

### Objective

Decentralize to irrelevance. Build quantum-resistant cryptography, hardware mesh networks, and proof-of-useful-work.

### Key Initiatives

#### Quantum Resistance
- CRYSTALS-Dilithium (signatures) — stub exists
- Kyber (encryption)
- SPHINCS+ (hash-based signatures)
- Gradual migration, no hard fork

#### Hardware Mesh Networks
- Smartphones (Omnia node)
- IoT devices (sensor nodes)
- Mesh networking
- Delay-tolerant routing

#### Proof-of-Useful-Work
- Scientific computation (protein folding, climate modeling)
- AI training (medical, climate, energy)
- Verification via deterministic computation and reproducible results

---

## Phase 3: The Canopy (Years 5-10) — Aspirational 🔮

*The following describes a long-term vision. It is not currently being developed.*

### Objective

Outlive us all. Build interplanetary operation and post-human governance.

### Interplanetary Operation

- Relativistic consensus for interplanetary delays
- Local autonomy with periodic global sync
- Peer-to-peer trade across planets

### Post-Human Governance

- AI agents as full governance participants
- Collective intelligence for complex decisions
- Self-modifying protocol with formal verification

---

## Development Best Practices

### Code Quality

```
Standards:
- Rust: clippy, fmt, audit
- Documentation: rustdoc

Coverage:
- Unit tests: >80%
- Integration tests: >60%
- End-to-end tests: critical paths

Run all tests:
cargo test --workspace
```

### Performance Optimization

```
Profiling:
- CPU: perf, flamegraph
- Memory: valgrind, heaptrack
- Network: tcpdump, wireshark

Note: Performance benchmarking has not been done yet.
The consensus engine processes O(new_events) per round,
but TPS has not been measured at scale.
```

---

## Deployment Checklist

### Pre-Launch — Not Yet Applicable

- [ ] Security audit completed
- [ ] All tests passing (`cargo test --workspace`)
- [ ] Documentation complete
- [ ] Community feedback incorporated
- [ ] Validator network established (50+ validators)
- [ ] Disaster recovery plan in place

---

## References

- Lamport, L. (1978). "Time, Clocks, and the Ordering of Events in a Distributed System"
- Shapiro, M., & Preguiça, N. (2011). "Conflict-free Replicated Data Types"
- Pease, M., Shostak, R., & Lamport, L. (1980). "Reaching Agreement in the Presence of Faults"

---

**Status:** Implementation Guide — Partially Complete
**Note:** All current interaction is via the Rust crate API, not REST.
**Version:** 2.0
**Last Updated:** May 2026

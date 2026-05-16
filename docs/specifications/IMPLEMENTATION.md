# Omnia Protocol Implementation Guide

**Version:** v4.0.0
**Last Updated:** 2026-03-05

> **This document describes the implementation status of the Omnia Protocol as of v4.0.0. The codebase uses a custom Rust implementation (not Parity Substrate). A REST API with Swagger UI is now available.**

---

## Current Implementation Status

```
[█████████████████████████░░░] 90% Complete
```

| Layer | Status | Tests |
|-------|--------|-------|
| Layer 1: Substrate | ✅ Implemented | 75+ |
| Layer 2: Domain Shards | ✅ Implemented | 33+ |
| Layer 3: Binding | ✅ Implemented | 41+ |
| Layer 4: Identity | ✅ Implemented (in shards) | — |
| Layer 5: Economics | ✅ Implemented | 22+ |
| Phase 0: ZK-Rollup | ✅ Architecture | 8+ |
| Node Binary | ✅ Implemented | 15+ |
| Chaos Tests | ✅ Implemented | — |

**Total: 278+ tests, all passing.**

---

## Actual Technology Stack

```
Language: Custom Rust (not Parity Substrate)
- Type-safe, memory-safe
- Excellent for cryptographic code

Core Crates (7 total):
- substrate/: Causal graph, consensus, gossip, crypto, CRDTs, slashing, snapshots
- shards/: 6 domain shards + cross-shard messaging + fee enforcement + nonce store
- binding/: Provenance log, RF stub, quantum commitments (real Dilithium)
- economics/: UBC token, quota, governance, useful work, fixed-point
- zk/: Settlement-agnostic ZK-rollup, Ethereum/Solana/Celestia/Bitcoin adapters
- node/: CLI binary, HTTP server, REST API, Swagger UI, Prometheus metrics
- chaos-tests/: Network partition, crash, drop-rate, equivocation simulation

Networking:
- libp2p (QUIC + GossipSub + mDNS)

Cryptography:
- Ed25519 signatures (ed25519-dalek)
- CRYSTALS-Dilithium PQC signatures (pqc-dilithium)
- BLAKE3 hashing
- Groth16 ZK proofs (arkworks on BN254)
- Shamir's Secret Sharing over GF(256)

State Management:
- CRDTs (GCounter, OrSet, LWWRegister)
- Merkle state root + inclusion proofs
- Event pruning for sustainability
- Sled-backed persistent slashing and nonce stores

HTTP/REST API (axum):
- 9 endpoints under /api/v1/
- Swagger UI at /swagger-ui
- OpenAPI spec at /api-docs/openapi.json
- Prometheus metrics at /metrics
- Health check at /health

Monitoring:
- Grafana dashboard (9 panels)
- Prometheus alert rules (4 alerts)
- Docker Compose with monitoring profile
```

### What's Actually Implemented ✅

- Causal graph consensus with vector clock ordering
- 6 domain shards with cross-shard messaging
- Fee enforcement via FeeSchedule + QuotaSystem
- Replay protection with persistent nonce store (SledNonceStore)
- Slashing engine with persistent sled storage (SledSlashingStore)
- Provenance tracking (full lifecycle)
- DID method (`did:omnia:`) with validation
- Shamir's Secret Sharing for social recovery
- Biometric anchors (BLAKE3-based)
- AI agent identity
- UBC token (soulbound quota with 10% decay, 1000 UBC/month)
- Quadratic voting with exponential decay
- Real Dilithium signature verification (not a stub)
- Real Groth16 ZK proving/verification (with simplified hash)
- Settlement-agnostic ZK-rollup architecture
- Ethereum, Solana, Celestia, Bitcoin adapters
- Full node binary with CLI, REST API, Swagger UI, and Prometheus metrics
- Chaos testing framework (partitions, crashes, drop rates, equivocation)
- Docker deployment with 5-node testnet + monitoring stack
- Reproducible build script
- SBOM generation script
- Fuzz testing with 7 targets
- Powers of Tau trusted setup ceremony (CLI subcommands)
- State snapshot and restore (CLI subcommands)

### What's a Stub ⚠️

| Feature | Status | What's Needed |
|---------|--------|---------------|
| ZK circuit hash | ⚠️ Placeholder | SNARK-friendly hash (Pedersen/Poseidon) instead of field-addition |
| RF fingerprinting | ⚠️ Stub | SDR hardware (HackRF/USRP) |
| Proof-of-useful-work | ⚠️ Stub | Production verification |

### What Doesn't Exist 🌑

- 🌑 API authentication / rate limiting / authorization
- 🌑 Encrypted key storage
- 🌑 Mobile wallet
- 🌑 JavaScript/Python client libraries
- 🌑 Validator network (single-node operator for Phase 0)
- 🌑 Sybil resistance / staking requirement
- 🌑 Constant-time guarantee for Dilithium verify()

---

## Phase 0: The Seed (Months 0-18) — ✅ In Progress

### Objective

Prove the concept works with a functional prototype that demonstrates:
- Causal graph consensus
- Self-sovereign identity
- Universal Basic Compute
- Settlement-agnostic ZK-rollup
- Operational node binary with REST API

### Development Milestones

#### Milestone 1: Foundation ✅ Completed
- ✅ Causal graph with vector clock ordering
- ✅ CRDT state convergence (GCounter, OrSet, LWWRegister)
- ✅ BFT finality mechanism
- ✅ libp2p gossip protocol
- ✅ Ed25519 signatures with replay protection

#### Milestone 2: Domain Shards ✅ Completed
- ✅ 6 domain shards: Financial, Identity, Physical, Computational, Biological, Economics
- ✅ Shard router with automatic dispatch (`EventProcessor` trait)
- ✅ Cross-shard messaging with causality proofs
- ✅ Replay protection via per-creator nonce tracking
- ✅ Fee enforcement (FeeSchedule + QuotaSystem)

#### Milestone 3: Binding & Identity ✅ Completed
- ✅ Provenance log (append-only CRDT)
- ✅ ProvenanceTracker lifecycle (create/transfer/verify/destroy)
- ✅ `did:omnia:` method with validation
- ✅ Shamir's Secret Sharing over GF(256)
- ✅ Biometric anchors (BLAKE3(salt || template))
- ✅ AI agent identity with capability types

#### Milestone 4: Economics ✅ Completed
- ✅ UBC token (soulbound, monthly quota)
- ✅ Quota system with epoch advancement
- ✅ Quadratic voting with exponential decay
- ✅ Fee enforcement with per-operation pricing
- ✅ Slashing engine with persistent storage
- ⚠️ Proof-of-useful-work stubs

#### Milestone 5: ZK-Rollup ✅ Architecture Complete
- ✅ Settlement-agnostic architecture (`SettlementLayer` trait)
- ✅ Ethereum adapter with Solidity contract
- ✅ Solana, Celestia, Bitcoin adapters
- ✅ L2 operator with batch builder
- ✅ Merkle state root + inclusion proofs
- ✅ Event pruning
- ✅ RollupCircuit with Groth16 proving/verification
- ✅ ExpandedRollupCircuit with Merkle path verification
- ✅ Powers of Tau trusted setup ceremony (CLI)
- ⚠️ SNARK-friendly hash placeholder (field-addition)

#### Milestone 6: Node Binary ✅ Completed
- ✅ CLI with clap (args + env vars + TOML config)
- ✅ HTTP server (axum) with health/metrics/API
- ✅ REST API with 9 endpoints + Swagger UI
- ✅ Prometheus metrics (6 node-level counters/gauges)
- ✅ Graceful shutdown (SIGINT/SIGTERM)
- ✅ Persistent slashing and nonce stores (sled)
- ✅ State snapshot/restore subcommands
- ✅ Trusted setup ceremony subcommands
- ✅ Structured logging with JSON support

#### Milestone 7: Testing & Verification ✅ Completed
- ✅ 278+ tests across 7 crates
- ✅ 7 fuzz targets
- ✅ TLA+ model checker (191-line spec, 5 invariants verified)
- ✅ TLA+ CRDT convergence spec (213 lines)
- ✅ Chaos testing framework (ChaosNetwork, 982 lines)
- ✅ Docker 5-node testnet + monitoring stack
- ✅ Reproducible build script
- ✅ SBOM generation

---

## Phase 1: The Root (Years 1-2) — Planned

*The following describes planned work. It has not been started.*

### Objective

Build standalone capabilities and expand the protocol's reach.

### Planned Work

| Feature | Priority | Status |
|---------|----------|--------|
| API authentication + rate limiting | P0 | 📋 Planned |
| Encrypted key storage | P0 | 📋 Planned |
| SNARK-friendly hash (Pedersen/Poseidon) | P0 | 📋 Planned |
| Sybil resistance / staking | P0 | 📋 Planned |
| sled → rocksdb migration | P1 | 📋 Planned |
| Mobile wallet | P1 | 📋 Planned |
| Validator network | P1 | 📋 Planned |
| Conviction voting | P2 | 📋 Planned |
| Delegation | P2 | 📋 Planned |
| JS/Python client libraries | P2 | 📋 Planned |

---

## Phase 2: The Trunk (Years 3-5) — Long-term Vision

*The following describes a long-term vision. It is not currently being developed.*

### Objective

Decommission legacy components, build quantum-resistant infrastructure, hardware mesh networks, and proof-of-useful-work.

### Key Initiatives

#### Quantum Resistance

```
Timeline: Year 3
Migration: Gradual, no hard fork

New Algorithms:
- Dilithium (signatures) — ✅ implemented
- Kyber (encryption) — 📋 planned
- SPHINCS+ (hash-based signatures) — 📋 planned

Process:
1. Implement quantum-resistant algorithms
2. Allow dual-signing (old + new)
3. Deprecate old algorithms
4. Full migration by Year 4
```

#### Hardware Mesh Networks

```
Devices:
- Smartphones (Omnia node)
- IoT devices (sensor nodes)
- Satellites (Starlink, Kuiper)
- Ground stations

Connectivity:
- Mesh networking
- Delay-tolerant routing
- Intermittent connectivity support
```

#### Proof-of-Useful-Work

```
Instead of burning energy on puzzles, validators prove they performed useful work:

Scientific Computation:
- Protein folding (Folding@home)
- Climate modeling (IPCC)
- Drug discovery

AI Training:
- Medical AI models
- Climate prediction
- Renewable energy optimization

Verification:
- Deterministic computation
- Reproducible results
- Hardware attestation
```

---

## Phase 3: The Canopy (Years 5-10) — Long-term Vision

*The following describes a long-term vision. It is not currently being developed.*

### Objective

Outlive us all. Build interplanetary operation and post-human governance.

### Interplanetary Operation

```
Relativistic Consensus:
- Mars operates independently
- Earth-Mars sync every 22 minutes
- Conflict resolution via causal ordering

Local Autonomy:
- Mars has its own validators
- Local finality in minutes
- Global finality in hours
```

### Post-Human Governance

```
AI Agents as Citizens:
- Full voting rights
- Quadratic voting applies
- Reputation system tracks behavior
```

---

## Development Best Practices

### Code Quality

```
Standards:
- Rust: clippy, fmt, audit
- Documentation: rustdoc
- API docs: utoipa (OpenAPI 3.0)

Coverage:
- Unit tests: >80%
- Integration tests: >60%
- End-to-end tests: critical paths
- Fuzz testing: 7 targets
- Chaos testing: partitions, crashes, Byzantine
- Formal verification: TLA+ (2 specs)

Run all tests:
cargo test --workspace
```

### Performance Optimization

```
Profiling:
- CPU: perf, flamegraph
- Memory: valgrind, heaptrack
- Network: tcpdump, wireshark

⚠️ Note: Performance benchmarking has not been done yet.
The consensus engine processes O(new_events) per round,
but TPS has not been measured at scale.
```

---

## Deployment Checklist

### Pre-Launch — Not Yet Applicable

- [ ] Security audit completed
- [ ] All tests passing (`cargo test --workspace`)
- [ ] API authentication implemented
- [ ] sled → rocksdb migration completed
- [ ] SNARK-friendly hash integrated into ZK circuit
- [ ] Encrypted key storage implemented
- [ ] Documentation complete
- [ ] Community feedback incorporated
- [ ] Validator network established (50+ validators)
- [ ] Disaster recovery plan in place

---

## References

- Lamport, L. (1978). "Time, Clocks, and the Ordering of Events in a Distributed System"
- Shapiro, M., & Preguiça, N. (2011). "Conflict-free Replicated Data Types"
- Pease, M., Shostak, R., & Lamport, L. (1980). "Reaching Agreement in the Presence of Faults"

**Status:** Implementation Guide — Phase 0 Nearly Complete
**Version:** 4.0.0

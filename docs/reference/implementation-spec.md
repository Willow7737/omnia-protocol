# Implementation Specification

> 🎯 Audience: Developers
> 🔗 Context: Protocol implementation specifications and protocol-level details
> 📅 Last Updated: 2026-06-24

**Version:** v0.1.68
**Last Updated:** 2026-03-05

> **This document describes the implementation status of the Omnia Protocol as of v0.1.68. The codebase uses a custom Rust implementation (not Parity Substrate). A REST API with Swagger UI is now available.**

---

## Current Implementation Status

```
[█████████████████████████░░░] 90% Complete
```

| Layer                  | Status                     | Tests         |
| ---------------------- | -------------------------- | ------------- |
| Layer 1: Substrate     | ✅ Implemented             | 120+          |
| Layer 2: Domain Shards | ✅ Implemented             | 60+           |
| Layer 3: Binding       | ✅ Implemented             | 41+           |
| Layer 4: Identity      | ✅ Implemented (in shards) | —             |
| Layer 5: Economics     | ✅ Implemented             | 40+           |
| Phase 0: ZK-Rollup     | ✅ Architecture            | 20+           |
| Node Binary            | ✅ Implemented             | 30+           |
| Chaos Tests            | ✅ Implemented             | ~15 scenarios |

**Total: Run `cargo test --workspace` for current count, all passing.**

---

## Actual Technology Stack

```
Language: Custom Rust (not Parity Substrate)
- Type-safe, memory-safe
- Excellent for cryptographic code

Core Crates (14 total):
- substrate/: Causal graph, consensus, gossip, crypto, CRDTs, slashing, snapshots
- shards/: 6 domain shards + cross-shard messaging + fee enforcement + nonce store
- binding/: Provenance log, RF stub, quantum commitments (real Dilithium)
- economics/: UBC token, quota, governance, useful work, fixed-point
- omnia-adapters/: Settlement-agnostic ZK-rollup, Ethereum/Solana/Celestia/Bitcoin adapters
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
- Redb-backed persistent slashing and nonce stores

HTTP/REST API (axum):
- 14 endpoints under /api/v1/
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
- Replay protection with persistent nonce store (RedbNonceStore)
- Slashing engine with persistent redb storage (RedbSlashingStore)
- Provenance tracking (full lifecycle)
- DID method (`did:omnia:`) with validation
- Shamir's Secret Sharing for social recovery
- Biometric anchors (BLAKE3-based)
- AI agent identity
- UBC token (soulbound quota with 10% decay, 1000 UBC/month)
- Quadratic voting with exponential decay
- Real Dilithium signature verification (not a stub)
- Real Groth16 ZK proving/verification with Poseidon hash (BLAKE3-derived round constants)
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

| Feature              | Status                  | What's Needed                                                                        |
| -------------------- | ----------------------- | ------------------------------------------------------------------------------------ |
| ZK circuit hash      | ⚠️ Poseidon implemented | Round constants use BLAKE3 derivation (not Filecoin/Neptune reference); needs review |
| RF fingerprinting    | ⚠️ Stub                 | SDR hardware (HackRF/USRP)                                                           |
| Proof-of-useful-work | ⚠️ Stub                 | Production verification                                                              |

### What Doesn't Exist 🌑

- ✅ API authentication (JWT + AuthorizedCallers + rate limiting + CORS) — Phase 0 FIND-001
- ✅ Encrypted key storage (AES-256-GCM + HKDF-SHA256) — Phase 0 FIND-010
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
- ✅ SNARK-friendly Poseidon hash (BLAKE3-derived round constants, needs audit against Filecoin/Neptune reference)

#### Milestone 6: Node Binary ✅ Completed

- ✅ CLI with clap (args + env vars + TOML config)
- ✅ HTTP server (axum) with health/metrics/API
- ✅ REST API with 14 endpoints + Swagger UI
- ✅ Prometheus metrics (11 node-level counters/gauges)
- ✅ Graceful shutdown (SIGINT/SIGTERM)
- ✅ Persistent slashing and nonce stores (redb)
- ✅ State snapshot/restore subcommands
- ✅ Trusted setup ceremony subcommands
- ✅ Structured logging with JSON support

#### Milestone 7: Testing & Verification ✅ Completed

- ✅ Run `cargo test --workspace` for current count across 14 crates
- ✅ 7 fuzz targets
- ✅ TLA+ model checker (191-line spec, 5 invariants verified)
- ✅ TLA+ CRDT convergence spec (213 lines)
- ✅ Chaos testing framework (ChaosNetwork, 982 lines)
- ✅ Docker 5-node testnet + monitoring stack
- ✅ Reproducible build script
- ✅ SBOM generation

---

## Phase 1: The Root (Years 1-2) — Complete

_The following describes planned work. It has not been started._

### Objective

Build standalone capabilities and expand the protocol's reach.

### Completed Work

| Feature                                 | Priority | Status                                                               |
| --------------------------------------- | -------- | -------------------------------------------------------------------- |
| API authentication + rate limiting      | P0       | ✅ Done (JWT + AuthorizedCallers + rate limiting + CORS — FIND-001)  |
| Encrypted key storage                   | P0       | ✅ Done (AES-256-GCM + HKDF-SHA256 — FIND-010)                       |
| SNARK-friendly hash (Pedersen/Poseidon) | P0       | ✅ Done (Poseidon with BLAKE3-derived round constants — needs audit) |
| Sybil resistance / staking              | P0       | 📋 Planned                                                           |
| redb persistence optimization           | P1       | 📋 Planned                                                           |
| Mobile wallet                           | P1       | 📋 Planned                                                           |
| Validator network                       | P1       | 📋 Planned                                                           |
| Conviction voting                       | P2       | 📋 Planned                                                           |
| Delegation                              | P2       | 📋 Planned                                                           |
| JS/Python client libraries              | P2       | 📋 Planned                                                           |

---

## Phase 2: The Trunk (Years 3-5) — Long-term Vision

_The following describes a long-term vision. It is not currently being developed._

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

_The following describes a long-term vision. It is not currently being developed._

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

⚠️ Note: Performance benchmarking is operational — 3-layer gate (IAI + multi-sample + single-sample) running in CI. See `docs/reference/benchmark-gates.md`.
The consensus engine processes O(new_events) per round,
but TPS has not been measured at scale.
```

---

## Deployment Checklist

### Pre-Launch — Not Yet Applicable

- [ ] Security audit completed
- [ ] All tests passing (`cargo test --workspace`)
- [ ] API authentication implemented
- [ ] redb persistence optimization
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

**Status:** Implementation Guide — Phase 0 Complete
**Version:** 0.1.68

---

## Phase 0 Security Additions

The following modules and features were added during Phase 0 to address critical security findings:

### New Source Files

| File                             | Lines | Purpose                                                                   |
| -------------------------------- | ----- | ------------------------------------------------------------------------- |
| `node/src/api/auth.rs`           | 645   | JWT authentication, AuthorizedCallers ACL, RateLimiter, CORS middleware   |
| `substrate/src/keystore.rs`      | 856   | EncryptedKeyStore with AES-256-GCM encryption, HKDF-SHA256 key derivation |
| `substrate/src/blake3_domain.rs` | 82    | BLAKE3 domain-separated hashing for cryptographic separation              |

### New Security Features

| Feature                  | Finding  | Description                                                                              |
| ------------------------ | -------- | ---------------------------------------------------------------------------------------- |
| JWT Authentication       | FIND-001 | REST API requires valid JWT tokens; configured via `OMNIA_JWT_SECRET`                    |
| AuthorizedCallers ACL    | FIND-001 | Only registered caller IDs can access the API; configured via `OMNIA_AUTHORIZED_CALLERS` |
| Rate Limiting            | FIND-001 | Per-IP request throttling; configured via `OMNIA_RATE_LIMIT_RPS`                         |
| CORS Middleware          | FIND-001 | Cross-origin resource sharing via `tower-http`                                           |
| Encrypted Key Storage    | FIND-010 | `EncryptedKeyStore` encrypts private keys with AES-256-GCM + HKDF-SHA256                 |
| Encrypted keygen         | FIND-010 | `keygen --passphrase` encrypts output with AES-256-GCM                                   |
| Creator-pubkey binding   | FIND-003 | Constant-time validation using `subtle` crate                                            |
| Slashing rollback        | FIND-011 | Snapshot-and-rollback pattern for slashing persistence failures                          |
| Governance quorum        | FIND-020 | `quorum_percentage` field (default 67%) for proposal passage                             |
| Governance time-lock     | FIND-020 | `time_lock_ms` field (default 24h) prevents flash-loan governance attacks                |
| Gossip payload limits    | FIND-021 | Early rejection of oversized gossip events via `MAX_PAYLOAD_SIZE`                        |
| BLAKE3 domain separation | FIND-022 | `blake3_hash_domain()` for context-specific hashing                                      |

### New Dependencies

| Dependency     | Version | Purpose                                        |
| -------------- | ------- | ---------------------------------------------- |
| `jsonwebtoken` | 9.x     | JWT token creation and validation              |
| `aes-gcm`      | 0.10.x  | AES-256-GCM encryption for private key storage |
| `hkdf`         | 0.12.x  | HKDF-SHA256 key derivation for key encryption  |
| `sha2`         | 0.10.x  | SHA-256 for HKDF key derivation                |
| `tower-http`   | 0.6.x   | CORS middleware for REST API                   |
| `subtle`       | 2.x     | Constant-time comparisons for creator binding  |

---

🔙 **Back**: [Reference Index](../) | 🔄 **Related**: [Blueprint Reference](./blueprint-reference.md)
🚀 **Next**: [Roadmap](./roadmap.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

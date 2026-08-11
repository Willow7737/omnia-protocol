# Phase Alignment & User Impact

> 🎯 Audience: All
> 🔗 Context: Maps each development phase to concrete user-facing capabilities and feature availability
> 📅 Last Updated: 2026-08-11

This document maps Omnia's development phases to the capabilities available to users, operators, and developers at each stage. Understanding phase alignment helps stakeholders know what they can do today and what's coming next.

## Phase 0: The Seed — Foundation Layer

**Status**: ✅ Complete

Phase 0 delivered the core protocol: a causal graph consensus engine, six domain shards, identity hardening, economics with UBC, and a settlement-agnostic ZK-rollup architecture. At this stage, users can interact with the protocol through the Rust API and the `omnia-node` REST binary.

### What Users Can Do

- **Developers**: Build on the Omnia Rust library — create events, submit shard operations, query state, use the REST API
- **Operators**: Run a single-node instance via Docker Compose (5-node testnet configuration available), monitor via Grafana/Prometheus
- **Architects**: Evaluate the six-layer architecture, review ADRs (001–009), audit the ZK circuit design
- **Laymen**: Read use cases and understand the protocol's vision — no direct interaction yet (no wallet or mobile app)

### Key Deliverables

| Capability             | Implementation | Notes                                                               |
| ---------------------- | -------------- | ------------------------------------------------------------------- |
| Causal graph consensus | ✅             | 454+ substrate tests, ~12,000 ops/s (v0.1.68 baseline; ~7,190 ops/s v0.1.48 historical) single-node (synchronous)   |
| 6 domain shards        | ✅             | Financial, Computational, Physical, Biological, Identity, Economics |
| DID identity system    | ✅             | `did:omnia:` method, Shamir recovery, biometric anchors             |
| UBC economics          | ✅             | 1,000 UBC/month soulbound quota, quadratic voting                   |
| ZK-rollup (arkworks)   | ✅             | Groth16 + Poseidon on BN254                                         |
| PQC signatures         | ✅             | ML-KEM-768 (FIPS-203) + Ed25519 hybrid                              |
| REST API               | ✅             | JWT auth, rate limiting, 14 endpoints                                |

---

## Phase 1: Hardening — Code Quality

**Status**: ✅ Complete

Phase 1 eliminated the most dangerous code quality issues: production `unwrap()` calls and string-typed errors. This phase also added E2E API tests and code coverage integration.

### What Changed for Users

- **Developers**: All 14 crates enforce `#![deny(clippy::unwrap_used)]` and use 34 typed error enums. E2E API test suite covers 14 endpoints across 4 auth states.
- **Operators**: API is now production-hardened with comprehensive auth testing. No functional changes to node operation.
- **Architects**: Rustdoc coverage improved for 7 security-critical modules (35 documentation items).

### Key Deliverables

| Capability                | Implementation | Notes                                          |
| ------------------------- | -------------- | ---------------------------------------------- |
| Typed errors              | ✅             | 34 `thiserror` enums, zero `Result<_, String>` |
| `unwrap()` removal        | ✅             | `#![deny(clippy::unwrap_used)]` on all crates  |
| E2E API tests             | ✅             | 19 test functions, 4 auth states               |
| Code coverage CI          | ✅             | `cargo llvm-cov`, 70% target                   |
| Solidity Groth16 verifier | ✅             | Pre-existing, production-quality               |

---

## Phase 2: Cryptographic Key Management & ZK Hardening

**Status**: ✅ Complete

Phase 2 closed critical security findings in SSS recovery, DKG share encryption, and the ZK trusted setup ceremony. This phase also introduced gradual slashing, DKG, BIP-39 mnemonics, and PQC key rotation.

### What Changed for Users

- **Developers**: SSS recovery now works end-to-end — shares are persisted with encryption, reconstruction produces a keypair that updates DID authentication. DKG produces valid threshold key shares. ZK circuit constrains event semantics (operation type, payload hash).
- **Operators**: Gradual slashing (3-tier: Warning → Jail → Ejection) replaces binary point system. Encrypted keystore supports BIP-39 mnemonic key derivation.
- **Architects**: ADRs 010–014 document key design decisions (encrypted keystore, slashing, VRF, DKG, Poseidon migration).

### Key Deliverables

| Capability              | Implementation | Notes                                                |
| ----------------------- | -------------- | ---------------------------------------------------- |
| SSS recovery flow       | ✅             | End-to-end: encrypt → persist → reconstruct → rotate |
| Trusted setup ceremony  | ✅             | Real EC operations on BN254 G1                       |
| ZK circuit constraints  | ✅             | Operation type + payload hash constraints            |
| Gradual slashing        | ✅             | ADR-011: Warning → Jail → Ejection                   |
| DKG                     | ✅             | Feldman VSS-based, AES-256-GCM share encryption      |
| BIP-39 mnemonics        | ✅             | SLIP-0010 HD derivation                              |
| PQC key rotation bridge | ✅             | KeyStoreBridge: persistent rotation across restarts  |
| sled removal            | ✅             | Fully migrated to redb                               |

---

## Phase 3: Network Optimization & Production Deployment

**Status**: ✅ Complete

Phase 3 closed all 5 open Phase 2 findings (3 critical, 1 high, 1 medium) and delivered network production readiness: leader selection, Kademlia DHT, GossipSub peer scoring, consensus persistence, fast-sync, and message compression.

### What Changed for Users

- **Operators**: Nodes can now discover peers via Kademlia DHT, traverse NATs with AutoNAT/Relay/DCutr, and persist consensus state across restarts. Fast-sync enables new nodes to bootstrap without full genesis replay. Gossip messages are compressed with Snappy.
- **Developers**: ML-KEM-768 (FIPS-203) replaces pqc_kyber, eliminating the KyberSlash vulnerability. Ethereum settlement adapter architecture is ready (live mode behind `ethereum-live` feature flag); Bitcoin settlement adapter is also live (`bitcoin-live` feature flag).
- **Architects**: Network layer is production-ready with peer scoring, graylisting, and NAT traversal.

### Key Deliverables

| Capability                    | Implementation | Notes                                           |
| ----------------------------- | -------------- | ----------------------------------------------- |
| SSS/DKG share encryption      | ✅             | XOR → AES-256-GCM with HKDF-SHA256              |
| Leader selection in consensus | ✅             | VRF-based, stake-weighted, mempool              |
| Kademlia DHT + NAT traversal  | ✅             | AutoNAT, Relay, DCutr, TCP fallback             |
| GossipSub peer scoring        | ✅             | Graylisting at -100, penalty weights            |
| Consensus state persistence   | ✅             | RedbConsensusStore, load_or_new                 |
| ML-KEM-768                    | ✅             | FIPS-203, replaces pqc_kyber                    |
| Fast-sync                     | ✅             | BLAKE3 checkpoints, supermajority, P2P download |
| Message compression           | ✅             | Snappy for >256 bytes                           |

---

## Phase 4: Mainnet Readiness & Settlement Integration

**Status**: ✅ Complete

Phase 4 delivered real Ethereum settlement via Alloy, wired gradual slashing per ADR-011, and added operational improvements: separate liveness/readiness probes, fast-sync P2P automation, and ceremony server/client.

### What Changed for Users

- **Operators**: Kubernetes liveness (`/healthz`) and readiness (`/readyz`) probes are now separate. Grafana admin password requires env var. Fast-sync has a full P2P download loop.
- **Developers**: Ethereum settlement adapter has real Alloy RPC integration (behind `ethereum-live` feature flag). Gradual slashing is wired into `record_offense_gradued()` per ADR-011 tiers. pqc_kyber fully replaced by ml-kem.
- **Architects**: ADRs 015–021 document leader selection, Kademlia, peer scoring, consensus persistence, fast-sync, ML-KEM, and message compression decisions.

### Key Deliverables

| Capability                | Implementation | Notes                                              |
| ------------------------- | -------------- | -------------------------------------------------- |
| Real Ethereum settlement  | ✅             | Alloy v1, feature-gated                            |
| Gradual slashing (wired)  | ✅             | ADR-011 tiers: Warning → Jail → Ejection           |
| KyberSlash fix            | ✅             | pqc_kyber → ml-kem migration                       |
| Fast-sync P2P automation  | ✅             | Full download loop with MockSyncNetwork            |
| Liveness/readiness probes | ✅             | `/healthz`, `/readyz`, `/health`                   |
| Ceremony server/client    | ✅             | Multi-party coordinator + CLI subcommands          |
| Supply chain hardening    | ✅             | First-party audits for ml-kem, snap, libp2p crates |

---

## Phase 5: Testnet Launch & Performance Validation

**Status**: ✅ Complete

Phase 5 captured real benchmark data (~12,000 ops/s (v0.1.68 baseline; ~7,190 ops/s v0.1.48 historical) synchronous single-node; a 13.6× improvement over initial tokio-based measurements), validated multi-node BFT consensus, reworked the VRF construction, added genesis tooling, and prepared the external audit package.

> **Correction (2026-07-20, AUDIT-2026-07 C1 / #339):** the Phase 5 "VRF"
> (both V1 and the V2 "ECVRF") was a **hash construction, not a VRF** — it
> performed no elliptic-curve operations and provided no VRF uniqueness
> guarantee. Leader selection was additionally a *public* function of a
> known seed, so leaders were predictable in advance. This is replaced by a
> real Edwards25519 EC-VRF plus an unpredictable beacon under ADR-026; see
> that ADR for the accurate description and RFC 9381 scope note.

### What Changed for Users

- **All Users**: Aspirational throughput claims have been replaced with honest measured data: ~12,000 ops/s (v0.1.68 baseline; ~7,190 ops/s v0.1.48 historical) synchronous single-node (a 13.6× improvement over the initial tokio-based measurement). This transparency allows realistic planning.
- **Operators**: Genesis tooling enables network bootstrapping with a validated initial validator set. (Leader selection was reworked again under ADR-026 to a real EC-VRF + unpredictable beacon after AUDIT-2026-07 C1 found the Phase 5 construction was neither a VRF nor unpredictable.)
- **Developers**: Poseidon dual-hash foundation enables future migration to Filecoin/Neptune reference parameters. Bug bounty program is active ($100–$50,000).
- **Architects**: External audit package assembled. Side-channel audit for ZK and binding crates completed.

### Key Deliverables

| Capability             | Implementation | Notes                                                                       |
| ---------------------- | -------------- | --------------------------------------------------------------------------- |
| Real benchmarks        | ✅             | ~12,000 ops/s (v0.1.68 baseline; ~7,190 ops/s v0.1.48 historical) sync single-node (13.6× improvement over initial measurements) |
| Multi-node BFT         | ✅             | 4-node test validated                                                       |
| VRF construction       | ⚠️ Superseded  | Phase 5 V1/V2 were hash constructions, not VRFs — replaced by a real EC-VRF (ADR-026, #339) |
| Genesis tooling        | ✅             | GenesisConfig, ValidatorInfo, TOML templates                                |
| Poseidon dual-hash     | ✅             | Custom + Reference, LazyLock-based                                          |
| Bug bounty program     | ✅             | $100–$50,000, 90-day embargo                                                |
| External audit package | ✅             | AUDIT_PACKAGE.md + findings template                                        |
| Side-channel audit     | ✅             | ZK + binding crates, 5 findings                                             |

---

## Post-Phase 5: What's Coming Next

### Before Public Testnet

1. Docker Compose multi-node verification (5-node network validation)
2. External security audit (commission professional audit firm)
3. Anvil E2E test (deploy → submit → verify on local Ethereum)

### Before Mainnet

4. Sybil resistance / stake-weighted validator registry
5. Causal graph garbage collection with pruning
6. Comprehensive rustdoc coverage (100% of public API)
7. Formal Dilithium timing side-channel audit
8. Poseidon standard parameter migration (Phase B)

### Long-Term Vision

9. Multi-party trusted setup ceremony over network
10. Extended formal verification (unbounded TLA+, Rust verification)
11. RF fingerprint hardware integration
12. Bitcoin settlement adapter (bitcoin-live feature, now live; Solana/Celestia adapters remain stubs)
13. Mobile wallet
14. Throughput optimization (multi-threaded, sharded consensus)

### Timeline

| Milestone        | Target  | Blockers                                  |
| ---------------- | ------- | ----------------------------------------- |
| Public Testnet   | Q2 2026 | External audit completion                 |
| Mainnet          | Q4 2026 | Audit findings, Sybil resistance, GC      |
| Hardened Mainnet | Q1 2027 | Multi-party ceremony, formal verification |

---

🔙 **Back**: [use-cases/](./) | 🔄 **Related**: [../reference/roadmap.md](../reference/roadmap.md)  
🚀 **Next**: [faq.md](./faq.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

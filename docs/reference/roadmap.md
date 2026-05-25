# Implementation Roadmap
> 🎯 Audience: All
> 🔗 Context: Consolidated roadmap from Phase 0 through Phase 5, with remaining milestones
> 📅 Last Updated: 2026-05-25

## Phase 0: The Seed ✅ Complete

*Goal: Proof of Concept*

- ✅ Causal graph consensus (Rust, 454+ tests)
- ✅ Self-sovereign identity system (DIDs, Shamir, biometrics)
- ✅ Universal Basic Compute (UBC)
- ✅ 6 domain shards with cross-shard messaging
- ✅ Settlement-agnostic ZK-rollup architecture
- ✅ Full ZK circuit (arkworks R1CS + Groth16 + Poseidon)
- ✅ Real PQC signatures (ML-KEM-768 / FIPS-203)
- ✅ REST API with JWT auth, rate limiting, CORS
- ✅ Encrypted keystore (AES-256-GCM)
- ✅ Gradual slashing (3-tier: Warning → Jail → Ejection)
- ✅ BFT consensus with VRF leader selection
- ✅ Docker 5-node testnet + monitoring stack

## Phase 1: Hardening ✅ Complete

- ✅ FIND-024: Typed Error Migration — 34 typed error enums with `thiserror`
- ✅ FIND-023: `unwrap()` Replacement — `#![deny(clippy::unwrap_used)]` on all 7 crates
- ✅ E2E REST API Integration Tests — 19 test functions
- ✅ Code Coverage Integration — `cargo llvm-cov` in CI
- ✅ FIND-033: RUSTSEC Advisory Review — stale ignores removed
- ✅ FIND-034: Documentation Sprint — 50+ discrepancy fixes
- ✅ Solidity Groth16 Verifier — production-quality with BN254 precompiles
- ✅ FIND-026: Rustdoc Coverage (partial) — 35 documentation items

## Phase 2: Cryptographic Key Management & ZK Hardening ✅ Complete

- ✅ C-1: Fix SSS recovery flow with encrypted shares and key derivation
- ✅ C-2: Fix trusted setup ceremony with real EC scalar multiplication
- ✅ H-1: Populate ZK circuit dummy fields with event semantics constraints
- ✅ H-2: ZK-SNARK benchmark suite
- ✅ H-3: Groth16 batch verification
- ✅ H-4: Integrate PQC key rotation with encrypted keystore
- ✅ H-5: Gradual slashing with jail/suspension and events
- ✅ M-1: BIP-39 mnemonic support to keystore
- ✅ M-2: DKG for threshold signatures (Feldman VSS)
- ✅ M-3: Complete sled removal
- ✅ M-4: ADRs 010–014
- ✅ M-5: Project dashboard and status documentation

## Phase 3: Network Optimization & Security Closure ✅ Complete

- ✅ C-1: SSS share encryption — XOR to AES-256-GCM
- ✅ C-2: DKG share encryption — XOR to AES-256-GCM
- ✅ C-3: SSS recovery DID authentication update
- ✅ H-1: ZK circuit trusted setup dummy values
- ✅ H-2: Trusted setup transcript hash initialization
- ✅ H-3: Wire leader selection into consensus block production
- ✅ H-4: Kademlia DHT + NAT Traversal
- ✅ H-5: GossipSub peer scoring configuration
- ✅ H-6: Consensus state persistence across restarts
- ✅ H-7: Real Ethereum settlement adapter (architecture + feature flag)
- ✅ M-1: ML-KEM-768 key encapsulation
- ✅ M-2: Fast-sync protocol
- ✅ M-3: Message compression (Snappy)
- ✅ M-4: Load testing infrastructure
- ✅ M-5: RUSTSEC advisory cleanup

## Phase 4: Mainnet Readiness ✅ Complete

- ✅ C-1: Real Ethereum settlement with Alloy
- ✅ H-1: Gradual slashing implementation — close ADR-011
- ✅ H-2: Migrate pqc_kyber → ml-kem (fix KyberSlash)
- ✅ H-3: Fast-sync P2P automation
- ✅ H-4: Separate liveness and readiness probes
- ✅ M-1: Multi-party trusted setup ceremony automation
- ✅ M-2: Documentation sprint — dashboard, ADRs, FAQ
- ✅ M-3: Load testing baseline capture
- ✅ M-4: Supply chain hardening

## Phase 5: Testnet Launch & Validation ✅ Complete

- ✅ C-1: Real performance benchmarking (~527 events/sec single-node)
- ✅ H-1: Multi-node BFT testnet validation
- ✅ H-2: VRF migration to ECVRF per RFC 9381
- ✅ H-3: Genesis tooling — network bootstrap procedure
- ✅ H-4: Ethereum testnet integration test
- ✅ M-1: Poseidon dual-hash transition foundation
- ✅ M-2: Bug bounty program setup
- ✅ M-3: External audit preparation package
- ✅ M-4: Side-channel audit for ZK and binding crates

## Remaining Work (Post-Phase 5)

### v0.1.56 Audit Findings — High Priority (Fix Before Testnet)
1. `BatchCrdtMerger::apply_batch` rollback implementation — claims atomic but doesn't roll back on partial failure
2. `GCounter::value()` overflow — sum of all counters can panic/wrap; use `checked_add`
3. Non-constant-time comparisons — `vrf.rs:190` and `bls12_381_scalar.rs:507` leak timing; use `subtle::ConstantTimeEq`
4. Deprecated `DkgSession` — uses wrong participant ID (`participants.first()` not `own_id`) and distributes identical secret to all participants; should be removed
5. GossipSub topic name mismatch — gossip uses `"omnia_events"` but scoring configures `"omnia-events"`; peer scoring is effectively a no-op
6. `KeyStoreBridge::rotate()` — generates new keypair but doesn't persist to keystore; wrong key after restart

### v0.1.56 Audit Findings — Medium Priority (Track for Resolution)
7. `CmRDT` trait is dead code (never implemented)
8. `AccountBalance::merge` doesn't merge `vector_clock`
9. Mixed SHA-256/BLAKE3 in CRDT `state_hash()`
10. `aes_gcm.rs` uses `expect()` instead of `Result`
11. `combine_signatures` doesn't deduplicate partials by participant
12. PQC feature declared but no implementation code
13. PriorityGossipQueue / GossipBloomFilter / CompactEncoder implemented but not integrated
14. Pipeline workers are stubs (log only, no actual processing)
15. Event submission uses ephemeral keypairs instead of node's persistent key
16. `UsefulWorkProof::verify_stub()` is the only verification method
17. `UsefulWorkType` variant fields are private (can't construct externally)
18. `TimeLockVoting` lacks `Serialize`/`Deserialize`
19. Docker Prometheus scraping wrong ports (host vs container)
20. Docker healthcheck endpoint mismatch (`/health` vs `/healthz`)
21. Deprecated `substrate` facade crate with heavy `node` dependency
22. Quorum check uses base weights instead of effective (decayed) weights

### Before Public Testnet
1. Docker Compose multi-node verification
2. External security audit
3. Anvil E2E test execution

### Before Mainnet
4. Sybil resistance / stake-weighted validator registry
5. Causal graph GC with pruning
6. Comprehensive rustdoc coverage (100% public API)
7. Formal Dilithium timing side-channel audit
8. Poseidon standard parameter migration (Phase B of dual-hash transition)

### Long-Term
9. Multi-party trusted setup ceremony over network
10. Extended formal verification (unbounded TLA+, Rust verification)
11. RF fingerprint hardware integration
12. Bitcoin/Solana/Celestia settlement adapters
13. Mobile wallet
14. Throughput optimization (multi-threaded, sharded consensus)

### Timeline

| Milestone | Target | Blockers |
|-----------|--------|-----------|
| Public Testnet | Q2 2026 | External audit |
| Mainnet | Q4 2026 | Audit findings, Sybil resistance, GC |
| Hardened Mainnet | Q1 2027 | Multi-party ceremony, formal verification |

---
🔙 **Back**: [reference/](./) | 🔄 **Related**: [benchmark-gates.md](./benchmark-gates.md)
🚀 **Next**: [benchmark-gates.md](./benchmark-gates.md) | 📜 **Source of Truth**: [Restructuring Blueprint](./blueprint-reference.md)

# Requirements and Status

> 🎯 Audience: All
> 🔗 Context: Granular tracking of technical requirements and completion
> 📅 Last Updated: 2026-07-09

This document tracks the granular requirements for the Omnia Protocol and their current implementation status.

## 1. Core Protocol (The Substrate)

| ID          | Requirement                        | Priority | Status       |
| :---------- | :--------------------------------- | :------- | :----------- |
| **REQ-1.1** | Causal Graph Data Structure        | P0       | ✅ Completed |
| **REQ-1.2** | Vector Clock Implementation        | P0       | ✅ Completed |
| **REQ-1.3** | CRDT State Merge Logic             | P0       | ✅ Completed |
| **REQ-1.4** | P2P Networking (libp2p)            | P1       | ✅ Completed |
| **REQ-1.5** | Finality Gadget (BFT)              | P1       | ✅ Completed |
| **REQ-1.6** | Replay Protection (nonce tracking) | P0       | ✅ Completed |
| **REQ-1.7** | State Root / Merkle Commitment     | P1       | ✅ Completed |
| **REQ-1.8** | Event Pruning                      | P2       | ✅ Completed |

## 2. Identity Layer

| ID          | Requirement                            | Priority | Status       |
| :---------- | :------------------------------------- | :------- | :----------- |
| **REQ-2.1** | DID Creation (`did:omnia:` method)     | P0       | ✅ Completed |
| **REQ-2.2** | Public Key Registry                    | P0       | ✅ Completed |
| **REQ-2.3** | Verifiable Credentials (VCs)           | P1       | ✅ Completed |
| **REQ-2.4** | Social Recovery Mechanism              | P2       | ✅ Completed |
| **REQ-2.5** | Shamir's Secret Sharing (GF(256))      | P1       | ✅ Completed |
| **REQ-2.6** | Biometric Anchors (BLAKE3)             | P2       | ✅ Completed |
| **REQ-2.7** | AI Agent Identity (5 capability types) | P1       | ✅ Completed |

## 3. Financial & Settlement

| ID          | Requirement                            | Priority | Status       |
| :---------- | :------------------------------------- | :------- | :----------- |
| **REQ-3.1** | Financial Shard (balances, transfers)  | P0       | ✅ Completed |
| **REQ-3.2** | UBC Quota Distribution                 | P0       | ✅ Completed |
| **REQ-3.3** | Cross-Shard Transactions               | P1       | ✅ Completed |
| **REQ-3.4** | Replay Protection (per-creator nonces) | P0       | ✅ Completed |
| **REQ-3.5** | Settlement-Agnostic ZK-Rollup          | P0       | ✅ Completed |
| **REQ-3.6** | Ethereum Adapter (OmniaRollup.sol)     | P1       | ✅ Completed |

## 4. Physical Binding

| ID          | Requirement                                | Priority | Status       |
| :---------- | :----------------------------------------- | :------- | :----------- |
| **REQ-4.1** | Provenance Log (append-only CRDT)          | P0       | ✅ Completed |
| **REQ-4.2** | Provenance Tracker (lifecycle)             | P1       | ✅ Completed |
| **REQ-4.3** | RF Fingerprint Hashing                     | P2       | 🔄 Stub      |
| **REQ-4.4** | Quantum Commitment (ML-KEM-768 / FIPS-203) | P2       | ✅ Completed |

## 5. Economics & Governance

| ID          | Requirement                                        | Priority | Status       |
| :---------- | :------------------------------------------------- | :------- | :----------- |
| **REQ-5.1** | UBC Token (soulbound quota)                        | P0       | ✅ Completed |
| **REQ-5.2** | Quadratic Voting + Integer Decay (PPM fixed-point) | P1       | ✅ Completed |
| **REQ-5.3** | Proof-of-Useful-Work                               | P2       | 🔄 Stub      |

## 6. Phase 1: Hardening

| ID           | Requirement                                              | Priority | Status          |
| :----------- | :------------------------------------------------------- | :------- | :-------------- |
| **REQ-P1.1** | Typed Error Migration (34 `thiserror` enums)             | P1       | ✅ Completed    |
| **REQ-P1.2** | `unwrap()` Replacement (`#![deny(clippy::unwrap_used)]`) | P1       | ✅ Completed    |
| **REQ-P1.3** | E2E REST API Integration Tests                           | P1       | ✅ Completed    |
| **REQ-P1.4** | Code Coverage Integration (`cargo llvm-cov`)             | P2       | ✅ Completed    |
| **REQ-P1.5** | RUSTSEC Advisory Review                                  | P2       | ✅ Completed    |
| **REQ-P1.6** | Documentation Sprint (50+ discrepancy fixes)             | P2       | ✅ Completed    |
| **REQ-P1.7** | Solidity Groth16 Verifier                                | P1       | ✅ Pre-existing |
| **REQ-P1.8** | Rustdoc Coverage (35 items, 7 security modules)          | P2       | ✅ Completed    |

## 7. Phase 2: Security Audit & Hardening

| ID            | Requirement                                                   | Priority | Status       |
| :------------ | :------------------------------------------------------------ | :------- | :----------- |
| **REQ-P2.1**  | Architecture Decision Records 010-014                         | P1       | ✅ Completed |
| **REQ-P2.2**  | Project Dashboard & Findings Update                           | P1       | ✅ Completed |
| **REQ-P2.3**  | SSS Recovery Authentication Update (FIND-P2-001)              | P0       | ✅ Completed |
| **REQ-P2.4**  | SSS Share Encryption Upgrade: XOR → AES-256-GCM (FIND-P2-002) | P0       | ✅ Completed |
| **REQ-P2.5**  | DKG Share Encryption Upgrade: XOR → AES-256-GCM (FIND-P2-003) | P0       | ✅ Completed |
| **REQ-P2.6**  | ZK Circuit Dummy Field Remediation (FIND-P2-010)              | P1       | ✅ Completed |
| **REQ-P2.7**  | Trusted Setup Transcript Hash Initialization (FIND-P2-011)    | P1       | ✅ Completed |
| **REQ-P2.8**  | Gradual Slashing Implementation (ADR-011)                     | P1       | ✅ Completed |
| **REQ-P2.9**  | VRF Spec Compliance Evaluation (ADR-012)                      | P2       | 📋 Deferred  |
| **REQ-P2.10** | Poseidon Parameter Migration Planning (ADR-014)               | P2       | 📋 Deferred  |

## 8. Future Requirements

| ID          | Requirement                                          | Priority | Status         |
| :---------- | :--------------------------------------------------- | :------- | :------------- |
| **REQ-6.1** | Real ZK Circuit (arkworks R1CS + Groth16)            | P0       | ✅ Completed   |
| **REQ-6.2** | Real PQC Signatures (Dilithium)                      | P0       | ✅ Completed   |
| **REQ-6.3** | Fee Mechanism (FeeSchedule + QuotaSystem)            | P1       | ✅ Completed   |
| **REQ-6.4** | Mobile Wallet                                        | P1       | ✅ Completed   |
| **REQ-6.5** | Validator Network                                    | P0       | 🌑 Not Started |
| **REQ-6.6** | Slashing (3-tier Gradual: Warning → Jail → Ejection) | P1       | ✅ Completed   |
| **REQ-6.7** | Conviction Voting                                    | P2       | 🌑 Not Started |
| **REQ-6.8** | Delegation                                           | P2       | 🌑 Not Started |

## 9. Phase 0 Sprint 3: Testnet Readiness

| ID          | Requirement                                           | Priority | Status       |
| :---------- | :---------------------------------------------------- | :------- | :----------- |
| **REQ-7.1** | Expanded ZK Circuit (Merkle path + event constraints) | P1       | ✅ Completed |
| **REQ-7.2** | TLA+ Formal Verification (consensus safety/liveness)  | P1       | ✅ Completed |
| **REQ-7.3** | Persistent Slashing State (redb)                      | P1       | ✅ Completed |
| **REQ-7.4** | Binary Entrypoint (omnia-node)                        | P2       | ✅ Completed |
| **REQ-7.5** | REST API (axum + utoipa Swagger UI)                   | P2       | ✅ Completed |
| **REQ-7.6** | Chaos Testing Framework                               | P2       | ✅ Completed |
| **REQ-7.7** | Security Audit Preparation                            | P3       | ✅ Completed |
| **REQ-7.8** | Production ZK Hash Gadget (Pedersen/Poseidon)         | P1       | ✅ Completed |

## 10. Phase 3: Network Optimization & Production Deployment

| ID            | Requirement                                | Priority | Status       |
| :------------ | :----------------------------------------- | :------- | :----------- |
| **REQ-P3.1**  | Leader Selection in Consensus Loop (H-3)   | P0       | ✅ Completed |
| **REQ-P3.2**  | Kademlia DHT + NAT Traversal (H-4)         | P0       | ✅ Completed |
| **REQ-P3.3**  | GossipSub Peer Scoring Configuration (H-5) | P1       | ✅ Completed |
| **REQ-P3.4**  | Consensus State Persistence (H-6)          | P0       | ✅ Completed |
| **REQ-P3.5**  | Real Ethereum Settlement Adapter (H-7)     | P1       | ✅ Completed |
| **REQ-P3.6**  | ML-KEM-768 Key Encapsulation (M-1)         | P0       | ✅ Completed |
| **REQ-P3.7**  | Fast-Sync Protocol (M-2)                   | P1       | ✅ Completed |
| **REQ-P3.8**  | Gossip Message Compression (M-3)           | P1       | ✅ Completed |
| **REQ-P3.9**  | Load Testing Infrastructure (M-4)          | P2       | ✅ Completed |
| **REQ-P3.10** | RUSTSEC Advisory Cleanup (M-5)             | P2       | ✅ Completed |

## 11. Phase 4: Production Hardening & Mainnet Preparation

| ID           | Requirement                                 | Priority | Status         |
| :----------- | :------------------------------------------ | :------- | :------------- |
| **REQ-P4.1** | Bitcoin Settlement Adapter                  | P1       | 🔄 Stub        |
| **REQ-P4.2** | Solana Settlement Adapter                   | P1       | 🔄 Stub        |
| **REQ-P4.3** | Celestia Settlement Adapter                 | P1       | 🔄 Stub        |
| **REQ-P4.4** | Validator Network (multi-node)              | P0       | 🌑 Not Started |
| **REQ-P4.5** | Public Testnet Launch                       | P0       | 🔄 Live (single node) |
| **REQ-P4.6** | Dynamic Fee Mechanism (EIP-1559-style)      | P1       | 📋 Planned     |
| **REQ-P4.7** | Documentation Sprint (Dashboard, ADRs, FAQ) | P2       | ✅ Completed   |
| **REQ-P4.8** | VRF Spec Compliance (ADR-012)               | P2       | 📋 Deferred    |
| **REQ-P4.9** | Poseidon Parameter Migration (ADR-014)      | P2       | 📋 Deferred    |

## 12. Phase 4: Mainnet Readiness (Completed)

| ID           | Requirement                                          | Priority | Status       |
| :----------- | :--------------------------------------------------- | :------- | :----------- |
| **REQ-P4.1** | Real Ethereum Settlement with Alloy                  | P1       | ✅ Completed |
| **REQ-P4.2** | Gradual Slashing Implementation (ADR-011)            | P0       | ✅ Completed |
| **REQ-P4.3** | Migrate pqc_kyber → ml-kem (FIPS-203)                | P0       | ✅ Completed |
| **REQ-P4.4** | Fast-Sync P2P Automation                             | P1       | ✅ Completed |
| **REQ-P4.5** | Separate Liveness and Readiness Probes               | P2       | ✅ Completed |
| **REQ-P4.6** | Multi-party Trusted Setup Ceremony Automation        | P1       | ✅ Completed |
| **REQ-P4.7** | Documentation Sprint (Dashboard, ADRs, FAQ)          | P2       | ✅ Completed |
| **REQ-P4.8** | Load Testing Baseline Capture                        | P2       | ✅ Completed |
| **REQ-P4.9** | Supply Chain Hardening (cargo-vet, cargo-deny, SBOM) | P2       | ✅ Completed |

---

## 13. Post-Phase 5: Audit Findings (v0.1.56 + v0.1.68)

| ID           | Finding                                                               | Severity  | Status        |
| :----------- | :-------------------------------------------------------------------- | :-------- | :------------ |
| **AUDIT-1**  | `BatchCrdtMerger::apply_batch` lacks real rollback on partial failure | 🔴 High   | ✅ Remediated |
| **AUDIT-2**  | `GCounter::value()` can overflow/panic on sum of all counters         | 🔴 High   | ✅ Remediated |
| **AUDIT-3**  | Non-constant-time comparisons in VRF + Feldman share verification     | 🔴 High   | ✅ Remediated |
| **AUDIT-4**  | Deprecated `DkgSession::finalize()` uses wrong participant ID         | 🔴 High   | ✅ Remediated |
| **AUDIT-5**  | Deprecated `DkgSession` distributes identical secret key to all       | 🔴 High   | ✅ Remediated |
| **AUDIT-6**  | GossipSub topic name mismatch (peer scoring is a no-op)               | 🔴 High   | ✅ Remediated |
| **AUDIT-7**  | `KeyStoreBridge::rotate()` doesn't persist new keypair                | 🔴 High   | ✅ Remediated |
| **AUDIT-8**  | `CmRDT` trait is dead code (never implemented)                        | 🟡 Medium | 📋 Tracked    |
| **AUDIT-9**  | `AccountBalance::merge` doesn't merge vector_clock                    | 🟡 Medium | 📋 Tracked    |
| **AUDIT-10** | Mixed SHA-256/BLAKE3 in CRDT state_hash                               | 🟡 Medium | 📋 Tracked    |
| **AUDIT-11** | `aes_gcm.rs` uses `expect()` instead of returning `Result`            | 🟡 Medium | 📋 Tracked    |
| **AUDIT-12** | `combine_signatures` doesn't deduplicate partials                     | 🟡 Medium | ✅ Remediated |
| **AUDIT-13** | PQC feature declared but no implementation                            | 🟡 Medium | 📋 Tracked    |
| **AUDIT-14** | PriorityGossipQueue/BloomFilter/CompactEncoder not integrated         | 🟡 Medium | ✅ Remediated (ADR-025 Stage 1: bloom dedup + priority queue wired into `GossipProtocol`; compact wire format v2 for broadcast) |
| **AUDIT-15** | Pipeline workers are stubs (log only, no processing)                  | 🟡 Medium | 📋 Tracked — superseded per ADR-025 Stage 3 (Lane 0 ack-aggregation workers replace the stub pipeline) |
| **AUDIT-16** | Event submission uses ephemeral keypairs                              | 🟡 Medium | ✅ Remediated |
| **AUDIT-17** | `UsefulWorkProof::verify_stub()` is only verification                 | 🟡 Medium | 📋 Tracked    |
| **AUDIT-18** | `UsefulWorkType` fields are private (can't construct externally)      | 🟡 Medium | 📋 Tracked    |
| **AUDIT-19** | `TimeLockVoting` lacks `Serialize`/`Deserialize`                      | 🟡 Medium | 📋 Tracked    |
| **AUDIT-20** | Docker Prometheus scraping wrong ports                                | 🟡 Medium | 📋 Tracked    |
| **AUDIT-21** | Docker healthcheck endpoint mismatch                                  | 🟡 Medium | 📋 Tracked    |
| **AUDIT-22** | Deprecated `substrate` facade crate (heavy `node` dependency)         | 🟡 Medium | 📋 Tracked    |
| **AUDIT-23** | Quorum check uses base weights instead of effective weights           | 🟡 Medium | 📋 Tracked    |

---

## 16. Live Testnet & Wallet Ecosystem (July 2026)

The protocol is now **running in public** with a full client ecosystem. Node: `https://78.47.43.136.sslip.io` (single node, v0.1.76+, protocol `/omnia/4.0.0`).

| ID          | Requirement                                                        | Priority | Status       |
| :---------- | :----------------------------------------------------------------- | :------- | :----------- |
| **REQ-W.1** | Wallet challenge/signature auth (`/auth/challenge`, `/auth/login`) | P0       | ✅ Completed |
| **REQ-W.2** | Idempotent DID registration for external JWTs (`/auth/register`)   | P0       | ✅ Completed |
| **REQ-W.3** | SHA-256 DID derivation shared with clients (pinned test vector)    | P0       | ✅ Completed |
| **REQ-W.4** | Mobile wallet v1 ([Omnia-Wallet](https://github.com/Willow7737/Omnia-Wallet)): balance, send, history, governance | P1 | ✅ Completed |
| **REQ-W.5** | Dual-mode auth (self-custody + Supabase via `mint-node-jwt`)       | P1       | ✅ Completed |
| **REQ-W.6** | Public single-node testnet deployment                              | P0       | ✅ Completed |

---

## 📊 Summary of Completion

| Category                    | Total   | ✅ Done | ⚠️ Partial | 🔄 Stub/Open | 🌑 Not Started | 📋 Planned | Progress        |
| :-------------------------- | :------ | :------ | :--------- | :----------- | :------------- | :--------- | :-------------- |
| **Core Protocol**           | 8       | 8       | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Identity**                | 7       | 7       | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Financial & Settlement**  | 6       | 6       | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Physical Binding**        | 4       | 3       | 0          | 1            | 0              | 0          | ████████░░ 75%  |
| **Economics**               | 3       | 2       | 0          | 1            | 0              | 0          | ███████░░░ 67%  |
| **Phase 1**                 | 8       | 8       | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Phase 2**                 | 10      | 10      | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Phase 0 (Sprint 3)**      | 8       | 8       | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Phase 0 (Sprint 4)**      | 18      | 18      | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Phase 3**                 | 10      | 10      | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Phase 4**                 | 9       | 9       | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Future**                  | 8       | 5       | 0          | 0            | 3              | 0          | ██████░░░░ 63%  |
| **v0.1.56 + v0.1.68 Audit** | 23      | 9       | 0          | 0            | 0              | 14         | ████░░░░░░ 39%  |
| **v0.1.69 Critical Audit**  | 16      | 16      | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Limit Verification**      | 39      | 39      | 0          | 0            | 0              | 0          | ██████████ 100% |
| **Wallet & Live Testnet**   | 6       | 6       | 0          | 0            | 0              | 0          | ██████████ 100% |
| **TOTAL**                   | **192** | **164** | **0**      | **1**        | **3**          | **14**     | █████████░ 85%  |

---

_Legend:_

- 🌑 **Not Started:** No work done.
- 🔄 **Stub/Open:** Code structure exists but needs real implementation; or finding identified but not yet remediated.
- ⚠️ **Partial:** Implementation exists but has known gaps or security findings.
- 📋 **Planned:** Designed (ADR or spec) but not yet implemented.
- ✅ **Completed:** Fully implemented and tested.

---

## 14. Verified Protocol Limits (Tested 2026-05-26)

> The following limits were verified by running `cargo test --workspace` and
> examining source code constants. L-17 fix (audit v0.1.68): the hardcoded
> test count previously listed here goes stale as tests are added/removed —
> run `cargo test --workspace` for the current count.

| Category       | Limit                              | Value                         | Source                                           |
| :------------- | :--------------------------------- | :---------------------------- | :----------------------------------------------- |
| **Consensus**  | Max concurrent graph tips          | 10,000                        | `causal_graph.rs:MAX_TIPS`                       |
| **Consensus**  | Max ancestry depth                 | 1,000,000 events              | `causal_graph.rs:MAX_ANCESTRY_DEPTH`             |
| **Consensus**  | Max ancestry visited per traversal | 100,000 events                | `causal_graph.rs:MAX_ANCESTRY_VISITED`           |
| **Consensus**  | Pruned metadata cap                | 50,000 entries                | `causal_graph.rs:MAX_PRUNED_EVENTS`              |
| **Consensus**  | Max batch size                     | 100 events                    | `batch.rs:MAX_BATCH_SIZE`                        |
| **Consensus**  | Default batch size                 | 50 events                     | `batch.rs:DEFAULT_BATCH_SIZE`                    |
| **Consensus**  | Batch flush timeout                | 100 ms                        | `batch.rs:DEFAULT_BATCH_TIMEOUT_MS`              |
| **Consensus**  | CRDT batch merge max               | 1,000 ops                     | `batch_crdt_merge.rs:MAX_CRDT_BATCH_SIZE`        |
| **Consensus**  | Committed round retention          | 10,000 rounds                 | `consensus.rs:DEFAULT_COMMITTED_ROUND_THRESHOLD` |
| **Consensus**  | Slash threshold                    | 500 points                    | `slashing.rs:DEFAULT_SLASH_THRESHOLD`            |
| **Consensus**  | Ejection threshold                 | 2,000 points                  | `slashing.rs:DEFAULT_EJECTION_THRESHOLD`         |
| **Consensus**  | Internal state shards              | 256                           | `sharded_state.rs:NUM_SHARDS`                    |
| **Network**    | Protocol version                   | `/omnia/4.0.0`                | `lib.rs:PROTOCOL_IDENTIFIER`                     |
| **Network**    | Max event payload                  | 1,048,576 bytes (1 MB)        | `event.rs:MAX_PAYLOAD_SIZE`                      |
| **Network**    | Max events per gossip              | 100                           | `gossip.rs:MAX_EVENTS_PER_GOSSIP`                |
| **Network**    | Max pending events                 | 100,000                       | `gossip.rs:MAX_PENDING_EVENTS`                   |
| **Network**    | Max seen events (dedup)            | 100,000                       | `gossip.rs:MAX_SEEN_EVENTS`                      |
| **Network**    | Max batch gossip size              | 1 MB                          | `gossip_batch.rs:MAX_BATCH_GOSSIP_SIZE`          |
| **Network**    | Gossip fanout                      | 3 peers                       | `gossip.rs`                                      |
| **Network**    | Partition threshold                | 30 seconds                    | `gossip.rs:DEFAULT_PARTITION_THRESHOLD_MS`       |
| **Network**    | Compression threshold              | 256 bytes                     | `gossip.rs:COMPRESSION_THRESHOLD`                |
| **Crypto**     | BLS12-381 secret key               | 32 bytes                      | `bls.rs:SECRET_KEY_SIZE`                         |
| **Crypto**     | BLS12-381 public key               | 96 bytes                      | `bls.rs:PUBLIC_KEY_SIZE`                         |
| **Crypto**     | BLS12-381 signature                | 48 bytes                      | `bls.rs:SIGNATURE_SIZE`                          |
| **Crypto**     | Ed25519 key/sig                    | 32 / 64 bytes                 | `crypto_schemes.rs`                              |
| **Crypto**     | Dilithium3 key/sig                 | 1,952 / 3,293 bytes           | `crypto_schemes.rs`                              |
| **Crypto**     | BLAKE3-256 digest                  | 32 bytes                      | `crypto_schemes.rs`                              |
| **Crypto**     | Poseidon params (BN254)            | t=3, R_F=8, R_P=57, alpha=5   | `poseidon.rs`                                    |
| **ZK**         | Merkle tree depth                  | 32 (4.29B leaves)             | `merkle.rs:MERKLE_DEPTH`                         |
| **ZK**         | SRS default degree                 | 2^16 = 65,536                 | `powers_of_tau.rs:DEFAULT_TAU_DEGREE`            |
| **ZK**         | Batch proof target                 | 100 events                    | `batch_proof_circuit.rs:BATCH_PROOF_TARGET_SIZE` |
| **Economics**  | UBC default quota                  | 1,000 units/month             | `quota.rs:DEFAULT_UBC_QUOTA`                     |
| **Economics**  | Epoch duration                     | 30 days                       | `quota.rs:DEFAULT_EPOCH_DURATION_MS`             |
| **Economics**  | Quorum percentage                  | 67%                           | `governance.rs:DEFAULT_QUORUM_PERCENTAGE`        |
| **Economics**  | Time-lock duration                 | 24 hours                      | `governance.rs:DEFAULT_TIME_LOCK_MS`             |
| **Economics**  | Decay precision                    | PPM fixed-point (0-1,000,000) | `fixed_point.rs:BASIS_PPM`                       |
| **Primitives** | Timestamp drift tolerance          | 120 seconds                   | `event.rs:MAX_TIMESTAMP_DRIFT_MS`                |
| **Primitives** | Max event age                      | 1 year                        | `event.rs:MAX_EVENT_AGE_MS`                      |
| **Primitives** | Node ID size                       | 32 bytes                      | `vector_clock.rs:NodeId`                         |
| **Shards**     | Domain shard count                 | 6                             | `ShardId` well-known IDs                         |
| **Settlement** | Ethereum adapter                   | Simulated + Live (alloy)      | `settlement/ethereum/`                           |
| **Settlement** | Multi-chain support                | Ethereum, Solana (stub), FFI  | `settlement/`                                    |

### Throughput Estimates (Verified by Limit Verification Tests)

| Scenario                                 | Measured                                      | Notes                                                     |
| :--------------------------------------- | :-------------------------------------------- | :-------------------------------------------------------- |
| CausalGraph insertion (10K events)       | ~1,400 evt/s (full cycle: create+sign+insert) | Release build, linear chain, 0B payload; includes signing |
| ConsensusEngine processing (1K events)   | ~9,000 evt/s                                  | Release build, linear chain; total_nodes=4                |
| Ed25519 signature verification (1K sigs) | ~27,000 sig/s (est.)                          | Test timing, not Criterion benchmark                      |
| VRF leader selection (150 candidates)    | ~10,000 sel/s (est.)                          | 100% unique leader distribution                           |
| CRDT batch merge (1K ops/batch)          | ~100K ops/s (est., no dedicated benchmark)    | 100 batches                                               |

> See [docs/reference/LIMITS.md](./LIMITS.md) for the complete verified limits reference.

## 15. v0.1.69 Critical Security Audit (16 findings, all closed)

> The following 16 critical security vulnerabilities were identified and fixed in v0.1.69.
> See SECURITY.md for the full list.

| ID            | Finding                                       | Status        |
| :------------ | :-------------------------------------------- | :------------ |
| **C-1**       | Identity recovery secret_commitment           | ✅ Remediated |
| **C-2**       | Biological ZK non-empty public_inputs         | ✅ Remediated |
| **C-3**       | Cross-shard causal proof verification         | ✅ Remediated |
| **C-4**       | Nonce store fail-closed                       | ✅ Remediated |
| **C-5**       | Economics verifier_pubkey required            | ✅ Remediated |
| **C-6**       | Ethereum verify_proof_with_root               | ✅ Remediated |
| **C-7**       | Per-client rate limiting                      | ✅ Remediated |
| **C-8**       | /readyz peer tracking                         | ✅ Remediated |
| **C-9**       | Validator registration                        | ✅ Remediated |
| **C-10**      | Persistent node keypair                       | ✅ Remediated |
| **C-11**      | Dual EconomicsState                           | ✅ Remediated |
| **C-12**      | Shard ops bypass                              | ✅ Remediated |
| **C-13**      | Helm chart                                    | ✅ Remediated |
| **C-14**      | Substrate fallback                            | ✅ Remediated |
| **C-15**      | Genesis hex                                   | ✅ Remediated |
| **C-16**      | Phase 2 ceremony                              | ✅ Remediated |

---

🔙 **Back**: [Reference Index](../) | 🔄 **Related**: [Roadmap](./roadmap.md)

> **Accuracy note**: Numbers marked (est.) are approximate and environment-dependent. They come from simple test timings (`omnia-limit-verification`), not from rigorous Criterion benchmarks. For reproducible benchmark results, run the Criterion suite: `cargo bench --bench baseline_bench`, `cargo bench --bench throughput`.

---

🔙 **Back**: [Reference Index](../) | 🔄 **Related**: [Roadmap](./roadmap.md)
🚀 **Next**: [Blueprint Reference](./blueprint-reference.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

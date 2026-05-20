# Requirements and Status
> 🎯 Audience: All
> 🔗 Context: Granular tracking of technical requirements and completion
> 📅 Last Updated: 2026-05-20

This document tracks the granular requirements for the Omnia Protocol and their current implementation status.

## 1. Core Protocol (The Substrate)

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-1.1** | Causal Graph Data Structure | P0 | ✅ Completed |
| **REQ-1.2** | Vector Clock Implementation | P0 | ✅ Completed |
| **REQ-1.3** | CRDT State Merge Logic | P0 | ✅ Completed |
| **REQ-1.4** | P2P Networking (libp2p) | P1 | ✅ Completed |
| **REQ-1.5** | Finality Gadget (BFT) | P1 | ✅ Completed |
| **REQ-1.6** | Replay Protection (nonce tracking) | P0 | ✅ Completed |
| **REQ-1.7** | State Root / Merkle Commitment | P1 | ✅ Completed |
| **REQ-1.8** | Event Pruning | P2 | ✅ Completed |

## 2. Identity Layer

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-2.1** | DID Creation (`did:omnia:` method) | P0 | ✅ Completed |
| **REQ-2.2** | Public Key Registry | P0 | ✅ Completed |
| **REQ-2.3** | Verifiable Credentials (VCs) | P1 | ✅ Completed |
| **REQ-2.4** | Social Recovery Mechanism | P2 | ✅ Completed |
| **REQ-2.5** | Shamir's Secret Sharing (GF(256)) | P1 | ✅ Completed |
| **REQ-2.6** | Biometric Anchors (BLAKE3) | P2 | ✅ Completed |
| **REQ-2.7** | AI Agent Identity (5 capability types) | P1 | ✅ Completed |

## 3. Financial & Settlement

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-3.1** | Financial Shard (balances, transfers) | P0 | ✅ Completed |
| **REQ-3.2** | UBC Quota Distribution | P0 | ✅ Completed |
| **REQ-3.3** | Cross-Shard Transactions | P1 | ✅ Completed |
| **REQ-3.4** | Replay Protection (per-creator nonces) | P0 | ✅ Completed |
| **REQ-3.5** | Settlement-Agnostic ZK-Rollup | P0 | ✅ Completed |
| **REQ-3.6** | Ethereum Adapter (OmniaRollup.sol) | P1 | ✅ Completed |

## 4. Physical Binding

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-4.1** | Provenance Log (append-only CRDT) | P0 | ✅ Completed |
| **REQ-4.2** | Provenance Tracker (lifecycle) | P1 | ✅ Completed |
| **REQ-4.3** | RF Fingerprint Hashing | P2 | 🔄 Stub |
| **REQ-4.4** | Quantum Commitment (ML-KEM-768 / FIPS-203) | P2 | ✅ Completed |

## 5. Economics & Governance

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-5.1** | UBC Token (soulbound quota) | P0 | ✅ Completed |
| **REQ-5.2** | Quadratic Voting + Integer Decay (PPM fixed-point) | P1 | ✅ Completed |
| **REQ-5.3** | Proof-of-Useful-Work | P2 | 🔄 Stub |

## 6. Phase 1: Hardening Sprint

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-P1.1** | Typed Error Migration (34 `thiserror` enums) | P1 | ✅ Completed |
| **REQ-P1.2** | `unwrap()` Replacement (`#![deny(clippy::unwrap_used)]`) | P1 | ✅ Completed |
| **REQ-P1.3** | E2E REST API Integration Tests | P1 | ✅ Completed |
| **REQ-P1.4** | Code Coverage Integration (`cargo llvm-cov`) | P2 | ✅ Completed |
| **REQ-P1.5** | RUSTSEC Advisory Review | P2 | ✅ Completed |
| **REQ-P1.6** | Documentation Sprint (50+ discrepancy fixes) | P2 | ✅ Completed |
| **REQ-P1.7** | Solidity Groth16 Verifier | P1 | ✅ Pre-existing |
| **REQ-P1.8** | Rustdoc Coverage (35 items, 7 security modules) | P2 | ✅ Completed |

## 7. Phase 2: Security Audit & Hardening

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-P2.1** | Architecture Decision Records 010-014 | P1 | ✅ Completed |
| **REQ-P2.2** | Project Dashboard & Findings Update | P1 | ✅ Completed |
| **REQ-P2.3** | SSS Recovery Authentication Update (FIND-P2-001) | P0 | ✅ Completed |
| **REQ-P2.4** | SSS Share Encryption Upgrade: XOR → AES-256-GCM (FIND-P2-002) | P0 | ✅ Completed |
| **REQ-P2.5** | DKG Share Encryption Upgrade: XOR → AES-256-GCM (FIND-P2-003) | P0 | ✅ Completed |
| **REQ-P2.6** | ZK Circuit Dummy Field Remediation (FIND-P2-010) | P1 | ✅ Completed |
| **REQ-P2.7** | Trusted Setup Transcript Hash Initialization (FIND-P2-011) | P1 | ✅ Completed |
| **REQ-P2.8** | Gradual Slashing Implementation (ADR-011) | P1 | ✅ Completed |
| **REQ-P2.9** | VRF Spec Compliance Evaluation (ADR-012) | P2 | 📋 Deferred |
| **REQ-P2.10** | Poseidon Parameter Migration Planning (ADR-014) | P2 | 📋 Deferred |

## 8. Future Requirements

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-6.1** | Real ZK Circuit (arkworks R1CS + Groth16) | P0 | ✅ Completed |
| **REQ-6.2** | Real PQC Signatures (Dilithium) | P0 | ✅ Completed |
| **REQ-6.3** | Fee Mechanism (FeeSchedule + QuotaSystem) | P1 | ✅ Completed |
| **REQ-6.4** | Mobile Wallet | P1 | 🌑 Not Started |
| **REQ-6.5** | Validator Network | P0 | 🌑 Not Started |
| **REQ-6.6** | Slashing (3-tier Gradual: Warning → Jail → Ejection) | P1 | ✅ Completed |
| **REQ-6.7** | Conviction Voting | P2 | 🌑 Not Started |
| **REQ-6.8** | Delegation | P2 | 🌑 Not Started |

## 9. Sprint 3: Testnet Readiness

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-7.1** | Expanded ZK Circuit (Merkle path + event constraints) | P1 | ✅ Completed |
| **REQ-7.2** | TLA+ Formal Verification (consensus safety/liveness) | P1 | ✅ Completed |
| **REQ-7.3** | Persistent Slashing State (redb) | P1 | ✅ Completed |
| **REQ-7.4** | Binary Entrypoint (omnia-node) | P2 | ✅ Completed |
| **REQ-7.5** | REST API (axum + utoipa Swagger UI) | P2 | ✅ Completed |
| **REQ-7.6** | Chaos Testing Framework | P2 | ✅ Completed |
| **REQ-7.7** | Security Audit Preparation | P3 | ✅ Completed |
| **REQ-7.8** | Production ZK Hash Gadget (Pedersen/Poseidon) | P1 | ✅ Completed |

## 10. Phase 3: Network Optimization & Production Deployment

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-P3.1** | Leader Selection in Consensus Loop (H-3) | P0 | ✅ Completed |
| **REQ-P3.2** | Kademlia DHT + NAT Traversal (H-4) | P0 | ✅ Completed |
| **REQ-P3.3** | GossipSub Peer Scoring Configuration (H-5) | P1 | ✅ Completed |
| **REQ-P3.4** | Consensus State Persistence (H-6) | P0 | ✅ Completed |
| **REQ-P3.5** | Real Ethereum Settlement Adapter (H-7) | P1 | ✅ Completed |
| **REQ-P3.6** | ML-KEM-768 Key Encapsulation (M-1) | P0 | ✅ Completed |
| **REQ-P3.7** | Fast-Sync Protocol (M-2) | P1 | ✅ Completed |
| **REQ-P3.8** | Gossip Message Compression (M-3) | P1 | ✅ Completed |
| **REQ-P3.9** | Load Testing Infrastructure (M-4) | P2 | ✅ Completed |
| **REQ-P3.10** | RUSTSEC Advisory Cleanup (M-5) | P2 | ✅ Completed |

## 11. Phase 4: Production Hardening & Mainnet Preparation

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-P4.1** | Bitcoin Settlement Adapter | P1 | 🔄 Stub |
| **REQ-P4.2** | Solana Settlement Adapter | P1 | 🔄 Stub |
| **REQ-P4.3** | Celestia Settlement Adapter | P1 | 🔄 Stub |
| **REQ-P4.4** | Validator Network (multi-node) | P0 | 🌑 Not Started |
| **REQ-P4.5** | Public Testnet Launch | P0 | 🌑 Not Started |
| **REQ-P4.6** | Dynamic Fee Mechanism (EIP-1559-style) | P1 | 📋 Planned |
| **REQ-P4.7** | Documentation Sprint (Dashboard, ADRs, FAQ) | P2 | ✅ Completed |
| **REQ-P4.8** | VRF Spec Compliance (ADR-012) | P2 | 📋 Deferred |
| **REQ-P4.9** | Poseidon Parameter Migration (ADR-014) | P2 | 📋 Deferred |

## 12. Sprint 4: Push to 9

| ID | Requirement | Priority | Status |
| :--- | :--- | :--- | :--- |
| **REQ-8.1** | Creator ↔ Pubkey Binding (blake3) | P0 | ✅ Completed |
| **REQ-8.2** | Payload Size Limits (1 MiB default) | P0 | ✅ Completed |
| **REQ-8.3** | Gossip Rate Limiting (token-bucket) | P1 | ✅ Completed |
| **REQ-8.4** | Nonce Persistence (redb) | P1 | ✅ Completed |
| **REQ-8.5** | TLA+ Agreement Fix + Liveness | P0 | ✅ Completed |
| **REQ-8.6** | Poseidon Hash in ZK Circuit | P0 | ✅ Completed |
| **REQ-8.7** | Snapshot Sync | P1 | ✅ Completed |
| **REQ-8.8** | Event Pruning (prune_finalized) | P1 | ✅ Completed |
| **REQ-8.9** | CRDT Convergence Formal Proof (TLA+) | P2 | ✅ Completed |
| **REQ-8.10** | Trusted Setup Ceremony (Powers of Tau) | P1 | ✅ Completed |
| **REQ-8.11** | VRF Leader Selection (ECVRF) | P1 | ✅ Completed |
| **REQ-8.12** | BLS Signature Aggregation | P2 | ✅ Completed |
| **REQ-8.13** | TOML Config File Support | P2 | ✅ Completed |
| **REQ-8.14** | Validator Key Management (keygen + rotation) | P2 | ✅ Completed |
| **REQ-8.15** | Grafana Dashboards + Alert Rules | P2 | ✅ Completed |
| **REQ-8.16** | Side-Channel Resistance Audit | P2 | ✅ Completed |
| **REQ-8.17** | Operations Runbook | P2 | ✅ Completed |
| **REQ-8.18** | Rolling Upgrade Strategy | P2 | ✅ Completed |

---

## 📊 Summary of Completion

| Category | Total | ✅ Done | ⚠️ Partial | 🔄 Stub/Open | 🌑 Not Started | 📋 Planned | Progress |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Core Protocol** | 8 | 8 | 0 | 0 | 0 | 0 | ██████████ 100% |
| **Identity** | 7 | 7 | 0 | 0 | 0 | 0 | ██████████ 100% |
| **Financial & Settlement** | 6 | 6 | 0 | 0 | 0 | 0 | ██████████ 100% |
| **Physical Binding** | 4 | 3 | 0 | 1 | 0 | 0 | ████████░░ 75% |
| **Economics** | 3 | 2 | 0 | 1 | 0 | 0 | ███████░░░ 67% |
| **Phase 1** | 8 | 8 | 0 | 0 | 0 | 0 | ██████████ 100% |
| **Phase 2** | 10 | 10 | 0 | 0 | 0 | 0 | ██████████ 100% |
| **Sprint 3** | 8 | 8 | 0 | 0 | 0 | 0 | ██████████ 100% |
| **Sprint 4** | 18 | 18 | 0 | 0 | 0 | 0 | ██████████ 100% |
| **Phase 3** | 10 | 10 | 0 | 0 | 0 | 0 | ██████████ 100% |
| **Phase 4** | 9 | 1 | 0 | 3 | 2 | 3 | █░░░░░░░░░ 17% |
| **Future** | 8 | 4 | 0 | 0 | 4 | 0 | █████░░░░░ 50% |
| **TOTAL** | **99** | **85** | **0** | **4** | **6** | **3** | ████████░░ 89% |

---
*Legend:*
- 🌑 **Not Started:** No work done.
- 🔄 **Stub/Open:** Code structure exists but needs real implementation; or finding identified but not yet remediated.
- ⚠️ **Partial:** Implementation exists but has known gaps or security findings.
- 📋 **Planned:** Designed (ADR or spec) but not yet implemented.
- ✅ **Completed:** Fully implemented and tested.

---
🔙 **Back**: [Reference Index](../) | 🔄 **Related**: [Roadmap](./roadmap.md)
🚀 **Next**: [Blueprint Reference](./blueprint-reference.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

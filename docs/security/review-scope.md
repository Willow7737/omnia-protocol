# Security Review Scope Document

**Project**: Omnia Protocol  
**Phase**: Phase 0 — Throughput Optimization  
**Date**: 2026-08-11  
**Last Updated**: 2026-08-11  
**Status**: Scope Defined — Engagement Pending

---

## 1. Review Objectives

The external security review will assess the correctness and safety of the Omnia Protocol's cryptographic and consensus implementations, with particular focus on the Phase 0 throughput optimization code that introduces concurrent state management, batch processing, and new network protocol logic.

## 2. Crates In Scope

### Primary (Full Review)

| Crate             | Lines of Code | Description                                                                         | Risk Level |
| ----------------- | ------------- | ----------------------------------------------------------------------------------- | ---------- |
| `omnia-crypto`    | ~3,200        | Ed25519, BLS12-381, VRF, AES-GCM keystore, PQC (Dilithium/ML-KEM)                   | Critical   |
| `omnia-consensus` | ~5,800        | Causal graph, BFT finality gadget, CRDTs, slashing, sharded state, batch processing | Critical   |
| `omnia-network`   | ~3,400        | libp2p GossipSub, compact encoding, bloom filters, priority queue                   | High       |

### Secondary (Differential Review)

| Crate              | Description                                  | Risk Level |
| ------------------ | -------------------------------------------- | ---------- |
| `omnia-primitives` | Core types (Event, VectorClock, NodeId)      | Medium     |
| `omnia-adapters`   | ZK circuits, batch proof circuit, settlement | High       |

### Out of Scope

- `omnia-node` (HTTP API surface — separate review)
- `shards`, `binding`, `economics` (application layer)
- Third-party dependencies (audited upstream)
- Infrastructure security (Docker, Kubernetes, CI/CD)

## 3. Specific Areas of Concern

### 3.1 Sharded Consensus State Concurrency

The `ShardedConsensusState` introduces 256 `RwLock`-protected shards with a separate global `RwLock` for cross-shard state. Review should verify:

- **Deadlock freedom**: No circular lock acquisition patterns across shards + global lock
- **Memory safety**: RwLock poisoning recovery doesn't introduce data inconsistency
- **Linearizability**: Cross-shard operations (witness registration, equivocation tracking) maintain correct ordering
- **Race conditions**: Concurrent reads and writes to different shards don't violate consensus invariants

### 3.2 Batch Proof Verification

The `BatchProof` uses BLAKE3 binary Merkle trees for batch integrity. Review should verify:

- **Second preimage resistance**: Cannot craft a different batch with the same Merkle root
- **Batch rejection correctness**: Invalid proofs, malformed batches, and state root mismatches are correctly rejected
- **Batch proof circuit**: The ZK batch proof circuit correctly verifies Merkle paths in-circuit using Poseidon hash

### 3.3 Bloom Filter Edge Cases

The `GossipBloomFilter` uses a rotating filter pair for duplicate suppression. Review should verify:

- **False positive impact**: Correctly handles the case where a new event is incorrectly flagged as duplicate
- **Rotation safety**: No events are lost during filter rotation
- **Adversarial resistance**: An attacker cannot craft events that consistently bypass the filter

### 3.4 Event Pool Memory Safety

The `EventPool` and `PruningAwarePool` manage event storage with slot reuse. Review should verify:

- **Use-after-free prevention**: Freed slots cannot be accessed through stale references
- **Double-free prevention**: A slot cannot be freed twice
- **Pruning correctness**: Pruned events maintain metadata integrity for ancestry queries

### 3.5 Compact Encoding Security

The `CompactEncoder` uses delta-compressed vector clocks. Review should verify:

- **Decoding validation**: Malformed delta clocks cannot corrupt local state
- **Truncation safety**: Truncated event IDs don't enable collision attacks
- **Replay protection**: Old compact-encoded messages cannot be replayed

## 4. Review Methodology

### 4.1 Automated Analysis

- `cargo audit` — dependency vulnerability scanning
- `cargo clippy` — lint-based static analysis
- `cargo deny` — license and security policy compliance
- `cargo vet` — supply chain audit verification
- Fuzz testing of all new parsing/decoding code (12 existing fuzz targets + new targets for batch/compact encoding)

### 4.2 Manual Review

- Line-by-line review of all `unsafe` code (currently: zero instances, enforced by `#![deny(unsafe_code) (see SAFETY.md)]`)
- Thread safety analysis of all concurrent code paths
- Cryptographic protocol review (VRF, BLS aggregation, ZK circuit constraints)
- State machine review (consensus state transitions, CRDT merge properties)

### 4.3 Formal Verification

The existing TLA+ specifications should be updated to cover:

- Sharded consensus state concurrent operations
- Batch proof verification state machine
- Bloom filter rotation protocol

## 5. Timeline

| Phase                     | Duration  | Deliverables                            |
| ------------------------- | --------- | --------------------------------------- |
| Kickoff & scope review    | Week 1    | Signed SOW, access to repository        |
| Automated analysis        | Week 2    | Tool reports, dependency audit          |
| Manual review (crypto)    | Weeks 2-4 | Crypto crate findings                   |
| Manual review (consensus) | Weeks 3-5 | Consensus crate findings                |
| Manual review (network)   | Weeks 4-5 | Network crate findings                  |
| TLA+ spec update          | Week 5    | Updated formal specs                    |
| Interim report            | Week 3    | Early findings for Critical/High issues |
| Final report              | Week 6    | Complete findings + recommendations     |
| Remediation support       | Weeks 7-8 | Resolution verification                 |

## 6. Deliverables

1. **Interim Report** (Week 3): Critical and High severity findings for early remediation
2. **Final Report** (Week 6): Complete findings with severity ratings, reproduction steps, and recommended fixes
3. **Remediation Verification** (Week 8): Confirmation that all Critical/High findings have been resolved
4. **TLA+ Spec Updates** (Week 5): Formal specifications covering new Phase 0 components

## 7. Severity Classification

| Severity      | Criteria                                                   | SLA                |
| ------------- | ---------------------------------------------------------- | ------------------ |
| Critical      | Remote exploit, consensus safety violation, key compromise | Fix within 48h     |
| High          | Local exploit, data corruption, liveness degradation       | Fix within 1 week  |
| Medium        | DoS vector, information leak, misconfiguration             | Fix within 2 weeks |
| Low           | Code quality, documentation, best practice                 | Fix within 4 weeks |
| Informational | Suggestion, improvement                                    | Discretionary      |

## 8. Access and Environment

- Repository: `https://github.com/Willow7737/omnia-protocol`
- Branch: `sprint/phase0-throughput-optimization` (review target)
- Test environment: Docker Compose 3-node testnet (`docker/docker-compose.testnet.yml`)
- Build: `cargo build --release -p omnia-node --features full`
- Test: `cargo test --workspace`
- Fuzz: `cargo fuzz run <target>` (12 existing fuzz targets)

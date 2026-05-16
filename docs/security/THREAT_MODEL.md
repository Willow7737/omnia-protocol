# Omnia Protocol — Threat Model

**Version**: 4.0.0
**Date**: 2026-05-16
**Classification**: Public
**Review Cadence**: Quarterly

## Document Control
| Field | Value |
|-------|-------|
| Version | 4.0.0 |
| Date | 2026-05-16 |
| Classification | Public |
| Review Cadence | Quarterly |

---

## 1. System Overview

Omnia Protocol is a settlement-agnostic coordination protocol using causal
graph consensus (DAG + vector clocks + CRDTs) to replace sequential blockchain
architectures. The system comprises:

- **Substrate layer**: Consensus engine, causal graph, gossip protocol, slashing
- **Shard layer**: Event routing, fee enforcement, nonce replay protection
- **Binding layer**: Quantum-resistant commitments (Ed25519 + Dilithium), RF
  fingerprinting, append-only provenance logs, PQC key rotation
- **Economics layer**: Governance with quadratic voting, fee scheduling
- **ZK layer**: Groth16 rollup proofs with Poseidon hash on BN254, Powers of Tau
  trusted setup ceremony, expanded circuit with Merkle path verification
- **Node layer**: REST API (axum), networking (libp2p QUIC), storage (sled)

### Trust Assumptions
- At most f Byzantine nodes out of N >= 3f + 1
- Network is partially synchronous (messages eventually delivered)
- Cryptographic assumptions: discrete log in BN254 groups, collision
  resistance of BLAKE3, EUF-CMA of Ed25519/Dilithium, binding property of
  Poseidon hash

---

## 2. Attack Surface Inventory

### 2.1 Network Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Gossip flood (DoS) | High | Token-bucket rate limiter (200 burst/100s) | Implemented |
| Replay attack | High | Nonce-based replay protection + sled persistence | Implemented |
| Eclipse attack | High | Bootstrap nodes + peer diversity requirements | Partial - no geographic diversity enforcement |
| Sybil attack | Medium | Stake-weighted VRF leader selection | Implemented |
| Network partition | High | BFT assumption (3f+1), partition recovery in runbook | Documented |
| QUIC handshake DoS | Medium | libp2p connection limits | Default limits only |
| Peer impersonation | High | Ed25519 authentication on gossip | Implemented |
| Large payload attack | Medium | MAX_PAYLOAD_SIZE = 1 MiB | Implemented |
| Metadata analysis | Low | Gossip broadcasts (no targeted messages) | By design |

### 2.2 Consensus Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Equivocation | Critical | Slashing engine with auto-detection | Implemented |
| Nothing-at-stake | High | BFT finality gadget (2/3 commit required) | Implemented |
| Long-range attack | High | Finality gadget prevents history rewrite past finality | Partial - No checkpoint mechanism yet |
| Leader DOS | High | VRF leader is deterministic - can be predicted | Partial - No leader privacy or fallback |
| Round stall | High | Round timeout + view-change | Implemented (Sprint 6) |
| Censorship by supermajority | Medium | - | No censorship resistance mechanism |
| Witness grinding | Medium | VRF-based witness selection | Implemented |
| Commit withholding | Medium | Optimistic confirmation + commit delay | Implemented |

### 2.3 Cryptographic Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Side-channel (timing) | High | subtle::ct_eq/ct_ne on all secret comparisons | Implemented |
| Quantum computer (future) | Critical | Hybrid Ed25519+Dilithium commitments | Partial - Default is ClassicalOnly |
| BN254 curve break | Critical | Migration playbook (CRYPTO_MIGRATION.md) with BLS12-381 fallback | Planned |
| BLAKE3 collision | Critical | Migration playbook with SHA3-256 fallback | Planned |
| Dilithium key compromise | High | PQC key rotation (binding/src/key_rotation.rs) | Implemented |
| Groth16 trusted setup subversion | High | PoK-verified ceremony (zk/src/setup/contribution.rs) | Implemented |
| Poseidon hash collision | High | Cauchy MDS + BLAKE3-derived RC (non-standard) | Risk - differs from reference constants |
| VRF output manipulation | High | Stake-weighted modular selection | Implemented |
| BLS rogue-key attack | Medium | Proof-of-possession required for aggregation | Implemented (Sprint 6) |

### 2.4 ZK Proof System Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Malicious trusted setup | Critical | Multi-party ceremony with PoK (zk/src/setup/) | Implemented |
| Proof replay across chains | High | L1Anchor with chain_id + block_height + timestamp in ProofBundle | Implemented |
| Invalid state root in bundle | High | verify_integrity() checks version, proof non-empty, root differ | Implemented |
| Poseidon parameter manipulation | High | Cauchy MDS determinism + BLAKE3-derived RC | Implemented but non-standard |
| ExpandedRollupCircuit witness manipulation | High | Merkle path + event commitment constraints | Implemented |
| Empty batch proof acceptance | Medium | Empty batch constraint: old_root == new_root | Implemented |
| Proof deserialization attack | High | Canonical deserialization with error handling | Implemented |

### 2.5 Binding Layer Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| RF fingerprint forgery | High | Hamming distance threshold + confidence scoring | Stub - needs real hardware |
| Quantum commitment forgery (classical) | High | Ed25519 signature verification | Implemented |
| Quantum commitment forgery (PQC) | High | Dilithium signature verification | Implemented |
| PQC key rotation downgrade | Medium | Phase downgrade rejected by PqcKeyRotationManager | Implemented |
| Provenance chain tampering | High | Append-only log with commitment links_to() verification | Implemented |
| Provenance log version downgrade | Medium | Version byte check in from_bytes() | Implemented |
| Authorization signature bypass | Medium | Empty authorization sig rejected in key rotation | Implemented |

### 2.6 Economic Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Spam (griefing) | Medium | Fee schedule + quota system | Implemented |
| Fee manipulation | Medium | Fixed fee schedule (governance-updated) | Implemented |
| Governance capture | High | Quadratic voting | Partial - still plutocratic at extreme wealth |
| Flash loan governance attack | Medium | Time-locked voting | Implemented (Sprint 6) |
| Stake grinding | Medium | VRF-based leader selection | Implemented |
| Nothing-at-stake (economic) | Medium | Slashing + slashing threshold | Implemented |

### 2.7 Data Integrity Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Snapshot tampering | High | BLAKE3 integrity hash + verify() | Implemented |
| Event graph corruption | Medium | Causal graph consistency checks | Implemented |
| CRDT divergence | High | TLA+-proven convergence | Proven |
| Nonce reset (disk corruption) | Medium | sled durability guarantees | Partial - No nonce backup |
| Pruning data loss | Medium | Archive mode default + PrunedEventMetadata | Implemented |
| ProofBundle tampering | High | bincode serialization + verify_integrity() | Implemented |

### 2.8 Supply Chain Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Compromised dependency | High | cargo-audit + cargo-vet + dependency policy | Implemented |
| Typosquatting | Medium | Cargo.lock pinning + manual review | Implemented |
| Build system compromise | Medium | Reproducible builds (partial) | Partial - Non-deterministic |
| Insider threat (maintainer) | Medium | SBOM generation + audit trail | Partial - No multi-signature for releases |

---

## 3. Unmitigated Risks

The following risks have NO mitigation and should be prioritized:

| # | Risk | Impact | Likelihood | Priority |
|---|------|--------|------------|----------|
| R1 | BN254 curve compromise (no alternative) | All ZK invalid | Low | P1 |
| R2 | Poseidon non-standard parameters | Hash divergence from reference | Medium | P2 |
| R3 | Long-range attack (no checkpoints) | History rewrite | Low | P2 |
| R4 | Censorship by supermajority | Transaction exclusion | Low | P2 |
| R5 | No release multi-signature | Supply chain compromise | Low | P3 |
| R6 | RF fingerprint forgery (stub implementation) | Physical identity bypass | High | P1 |

---

## 4. Threat Actors

| Actor | Capability | Target | Mitigation |
|-------|-----------|--------|------------|
| Solo validator | 1 vote, limited stake | Minor griefing | Fees + rate limiting |
| Colluding minority (< 1/3) | Coordinated voting | Censorship, equivocation | Slashing + BFT |
| Colluding supermajority (>= 2/3) | Full consensus control | Censorship, history rewrite | Social layer only |
| Network adversary | Packet manipulation | DOS, eclipse, partition | QUIC + rate limiting |
| Cryptanalytic researcher | Math breakthroughs | Curve/hash breaks | Crypto agility (CRYPTO_MIGRATION.md) |
| Quantum computer | Break Ed25519/ECDSA | Key compromise | Hybrid PQC mode |
| Nation-state | All of the above | Targeted takedown | Geographic distribution |
| Malicious dependency maintainer | Supply chain | Backdoor in node binary | cargo-vet + reproducible builds |
| ZK operator | Submit fraudulent proofs | State root manipulation | Groth16 proof verification on L1 |

---

## 5. Attack Trees

### 5.1 Network Halt Attack Tree

```
Network Halts
+-- Consensus stalls
|   +-- Leader goes offline [P1 - no fallback]
|   +-- Round never advances [Mitigated - timeout/view-change implemented]
|   +-- >1/3 validators offline [By design - BFT bound]
+-- Gossip flood
|   +-- New peer sends 10k msgs/sec [Mitigated - rate limiter]
|   +-- Compromised peer sends crafted msgs [Mitigated - validation]
|   +-- Amplification via cross-shard routing [Mitigated - fee enforcement]
+-- State corruption
    +-- Corrupt snapshot loaded [Mitigated - BLAKE3 verify]
    +-- sled corruption [Low probability - ACID]
    +-- Memory exhaustion via large events [Mitigated - 1 MiB limit]
```

### 5.2 Key Compromise Attack Tree

```
Validator Key Compromised
+-- Ed25519 key stolen
|   +-- Memory dump [Mitigated - constant-time ops]
|   +-- Disk access [Partial - EncryptedKeyStore]
|   +-- Side-channel [Mitigated - subtle crate]
+-- Dilithium key stolen
|   +-- Same as Ed25519
|   +-- Key rotation available [Mitigated - PqcKeyRotationManager]
+-- BLS key stolen
|   +-- Can forge aggregate signatures
|   +-- Proof-of-possession required [Mitigated - Sprint 6]
+-- Social recovery not available [UNMITIGATED - no threshold scheme]
```

### 5.3 ZK Proof System Attack Tree

```
Fraudulent ZK Proof Accepted
+-- Trusted setup compromised
|   +-- All ceremony participants collude [Unlikely - multi-party PoK]
|   +-- PoK verification bypassed [Not possible - verify_contribution() enforced]
+-- Proof for wrong state root
|   +-- Circuit constraint bypassed [Not possible - R1CS enforcement]
|   +-- Public input mismatch [Mitigated - verify_proof checks inputs]
+-- Replay attack
|   +-- Same proof on different chain [Mitigated - L1Anchor chain_id]
|   +-- Same proof at different block [Mitigated - L1Anchor block_height]
+-- Prover key stolen
    +-- Can create valid proofs for arbitrary transitions [UNMITIGATED - no key encryption]
```

### 5.4 Binding Layer Attack Tree

```
Physical Identity Forged
+-- RF fingerprint bypassed
|   +-- Clone RF emission [Stub - no real hardware verification]
|   +-- Replay old measurement [Partial - VectorClock timestamp]
|   +-- Brute-force spectral hash [Mitigated - 256-bit hash space]
+-- Quantum commitment forged
|   +-- Break Ed25519 [ClassicalOnly phase - verify_ed25519()]
|   +-- Break Dilithium [Hybrid/PostQuantum phase - verify_dilithium()]
|   +-- Key rotation downgrade [Mitigated - PqcKeyRotationManager rejects]
+-- Provenance chain broken
    +-- Modify past event [Mitigated - links_to() chain verification]
    +-- Remove event [Mitigated - append-only CRDT]
    +-- Version downgrade [Mitigated - from_bytes() version check]
```

---

## 6. Recommendations Summary

| # | Recommendation | Sprint | Status |
|---|---------------|--------|--------|
| 1 | Implement consensus timeout/view-change | Sprint 6 (Phase C) | Implemented |
| 2 | Add BLS proof-of-possession | Sprint 6 (Phase C) | Implemented |
| 3 | Implement crypto scheme versioning | Sprint 6 (Phase D) | Implemented |
| 4 | Add genesis replay tool | Sprint 6 (Phase E) | Implemented |
| 5 | Add slashing appeals/undo process | Sprint 6 (Phase E) | Implemented |
| 6 | Add time-locked voting | Sprint 6 (Phase F) | Implemented |
| 7 | Migrate Poseidon to reference constants | Future | Planned |
| 8 | Side-channel audit for ZK and binding crates | Future | Not started |
| 9 | Schedule external security audit | Post-Sprint 6 | Pending |
| 10 | Implement leader privacy (threshold encryption) | Future | Not planned |
| 11 | Real RF fingerprint hardware integration | Future | Stub only |

# Omnia Protocol — Threat Model

## Document Control
| Field | Value |
|-------|-------|
| Version | 1.0 |
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
- **Binding layer**: Quantum-resistant commitments (Ed25519 + Dilithium)
- **Economics layer**: Governance with quadratic voting, fee scheduling
- **ZK layer**: Groth16 rollup proofs with Poseidon hash on BN254
- **Node layer**: REST API (axum), networking (libp2p QUIC), storage (sled)

### Trust Assumptions
- At most f Byzantine nodes out of N >= 3f + 1
- Network is partially synchronous (messages eventually delivered)
- Cryptographic assumptions: discrete log in BN254/BLS12-381 groups, collision
  resistance of BLAKE3, EUF-CMA of Ed25519/Dilithium

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
| BN254 curve break | Critical | ZkScheme versioning with BLS12-381 fallback | Planned (crypto_schemes.rs) |
| BLAKE3 collision | Critical | HashScheme versioning with SHA3-256 fallback | Planned (crypto_schemes.rs) |
| Dilithium key compromise | High | PQC key rotation (binding/src/key_rotation.rs) | Planned (Sprint 6) |
| Groth16 trusted setup subversion | High | PoK-verified ceremony | Implemented |
| VRF output manipulation | High | Stake-weighted modular selection | Implemented |
| BLS rogue-key attack | Medium | Proof-of-possession required for aggregation | Implemented (Sprint 6) |

### 2.4 Economic Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Spam (griefing) | Medium | Fee schedule + quota system | Implemented |
| Fee manipulation | Medium | Fixed fee schedule (governance-updated) | Implemented |
| Governance capture | High | Quadratic voting | Partial - still plutocratic at extreme wealth |
| Flash loan governance attack | Medium | Time-locked voting | Implemented (Sprint 6) |
| Stake grinding | Medium | VRF-based leader selection | Implemented |
| Nothing-at-stake (economic) | Medium | Slashing + slashing threshold | Implemented |

### 2.5 Data Integrity Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Snapshot tampering | High | BLAKE3 integrity hash + verify() | Implemented |
| Event graph corruption | Medium | Causal graph consistency checks | Implemented |
| CRDT divergence | High | TLA+-proven convergence | Proven |
| Nonce reset (disk corruption) | Medium | sled durability guarantees | Partial - No nonce backup |
| Pruning data loss | Medium | Archive mode default + PrunedEventMetadata | Implemented |

### 2.6 Supply Chain Attack Surface

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
| R1 | Consensus round stall (no timeout) | Network halts | Medium | P0 |
| R2 | BN254 curve compromise (no alternative) | All ZK invalid | Low | P1 |
| R3 | BLS rogue-key attack | Invalid aggregate sigs | Medium | P1 |
| R4 | Leader DOS (VRF predicts leader) | Round stalls | Medium | P1 |
| R5 | Long-range attack (no checkpoints) | History rewrite | Low | P2 |
| R6 | Censorship by supermajority | Transaction exclusion | Low | P2 |
| R7 | Flash loan governance attack | Governance manipulation | Medium | P2 |
| R8 | No release multi-signature | Supply chain compromise | Low | P3 |

---

## 4. Threat Actors

| Actor | Capability | Target | Mitigation |
|-------|-----------|--------|------------|
| Solo validator | 1 vote, limited stake | Minor griefing | Fees + rate limiting |
| Colluding minority (< 1/3) | Coordinated voting | Censorship, equivocation | Slashing + BFT |
| Colluding supermajority (>= 2/3) | Full consensus control | Censorship, history rewrite | Social layer only |
| Network adversary | Packet manipulation | DOS, eclipse, partition | QUIC + rate limiting |
| Cryptanalytic researcher | Math breakthroughs | Curve/hash breaks | Crypto agility (Phase D) |
| Quantum computer | Break Ed25519/ECDSA | Key compromise | Hybrid PQC mode |
| Nation-state | All of the above | Targeted takedown | Geographic distribution |
| Malicious dependency maintainer | Supply chain | Backdoor in node binary | cargo-vet + reproducible builds |

---

## 5. Attack Trees

### 5.1 Network Halt Attack Tree

```
Network Halts
+-- Consensus stalls
|   +-- Leader goes offline [P1 - no fallback]
|   +-- Round never advances [P0 - no timeout]
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
|   +-- No rotation mechanism [UNMITIGATED - R9]
+-- BLS key stolen
|   +-- Can forge aggregate signatures
|   +-- No proof-of-possession [UNMITIGATED - R3]
+-- Social recovery not available [UNMITIGATED - no threshold scheme]
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
| 7 | Schedule external security audit | Post-Sprint 6 | Pending |
| 8 | Implement leader privacy (threshold encryption) | Future | Not planned |

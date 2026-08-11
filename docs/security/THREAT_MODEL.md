# Omnia Protocol — Threat Model

> 🎯 Audience: Security Researchers
> 🔗 Context: Part of the security documentation section
> 📅 Last Updated: 2026-08-11

**Version**: 4.0.0
**Date**: 2026-05-16
**Classification**: Public
**Review Cadence**: Quarterly

## Document Control

| Field          | Value      |
| -------------- | ---------- |
| Version        | 4.0.0      |
| Date           | 2026-05-16 |
| Classification | Public     |
| Review Cadence | Quarterly  |

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
- **Node layer**: REST API (axum), networking (libp2p QUIC), storage (redb)

### Trust Assumptions

- At most f Byzantine nodes out of N >= 3f + 1
- Network is partially synchronous (messages eventually delivered)
- Cryptographic assumptions: discrete log in BN254 groups, collision
  resistance of BLAKE3, EUF-CMA of Ed25519/Dilithium, binding property of
  Poseidon hash

---

## 2. Attack Surface Inventory

### 2.1 Network Attack Surface

| Attack               | Severity | Mitigation                                           | Status                                        |
| -------------------- | -------- | ---------------------------------------------------- | --------------------------------------------- |
| Gossip flood (DoS)   | High     | Token-bucket rate limiter (200 burst/100s)           | Implemented                                   |
| Replay attack        | High     | Nonce-based replay protection + redb persistence     | Implemented                                   |
| Eclipse attack       | High     | Bootstrap nodes + peer diversity requirements        | Partial - no geographic diversity enforcement |
| Sybil attack         | Medium   | Stake-weighted VRF leader selection                  | Implemented                                   |
| Network partition    | High     | BFT assumption (3f+1), partition recovery in runbook | Documented                                    |
| QUIC handshake DoS   | Medium   | libp2p connection limits                             | Default limits only                           |
| Peer impersonation   | High     | Ed25519 authentication on gossip                     | Implemented                                   |
| Large payload attack | Medium   | MAX_PAYLOAD_SIZE = 1 MiB                             | Implemented                                   |
| Metadata analysis    | Low      | Gossip broadcasts (no targeted messages)             | By design                                     |

### 2.2 Consensus Attack Surface

| Attack                      | Severity | Mitigation                                             | Status                                  |
| --------------------------- | -------- | ------------------------------------------------------ | --------------------------------------- |
| Equivocation                | Critical | Slashing engine with auto-detection                    | Implemented                             |
| Nothing-at-stake            | High     | BFT finality gadget (2/3 commit required)              | Implemented                             |
| Long-range attack           | High     | Finality gadget prevents history rewrite past finality | Partial - No checkpoint mechanism yet   |
| Leader DOS                  | High     | VRF leader is deterministic - can be predicted         | Partial - No leader privacy or fallback |
| Round stall                 | High     | Round timeout + view-change                            | Implemented (Sprint 6)                  |
| Censorship by supermajority | Medium   | -                                                      | No censorship resistance mechanism      |
| Witness grinding            | Medium   | VRF-based witness selection                            | Implemented                             |
| Commit withholding          | Medium   | Optimistic confirmation + commit delay                 | Implemented                             |

### 2.3 Cryptographic Attack Surface

| Attack                           | Severity | Mitigation                                                       | Status                                  |
| -------------------------------- | -------- | ---------------------------------------------------------------- | --------------------------------------- |
| Side-channel (timing)            | High     | subtle::ct_eq/ct_ne on all secret comparisons                    | Implemented                             |
| Quantum computer (future)        | Critical | Hybrid Ed25519+Dilithium commitments                             | Partial - Default is ClassicalOnly      |
| BN254 curve break                | Critical | Migration playbook (CRYPTO_MIGRATION.md) with BLS12-381 fallback | Planned                                 |
| BLAKE3 collision                 | Critical | Migration playbook with SHA3-256 fallback                        | Planned                                 |
| Dilithium key compromise         | High     | PQC key rotation (binding/src/key_rotation.rs)                   | Implemented                             |
| Groth16 trusted setup subversion | High     | PoK-verified ceremony (zk/src/setup/contribution.rs)             | Implemented                             |
| Poseidon hash collision          | High     | Cauchy MDS + BLAKE3-derived RC (non-standard)                    | Risk - differs from reference constants |
| VRF output manipulation          | High     | Stake-weighted modular selection                                 | Implemented                             |
| BLS rogue-key attack             | Medium   | Proof-of-possession required for aggregation                     | Implemented (Sprint 6)                  |

### 2.4 ZK Proof System Attack Surface

| Attack                                     | Severity | Mitigation                                                       | Status                       |
| ------------------------------------------ | -------- | ---------------------------------------------------------------- | ---------------------------- |
| Malicious trusted setup                    | Critical | Multi-party ceremony with PoK (zk/src/setup/)                    | Implemented                  |
| Proof replay across chains                 | High     | L1Anchor with chain_id + block_height + timestamp in ProofBundle | Implemented                  |
| Invalid state root in bundle               | High     | verify_integrity() checks version, proof non-empty, root differ  | Implemented                  |
| Poseidon parameter manipulation            | High     | Cauchy MDS determinism + BLAKE3-derived RC                       | Implemented but non-standard |
| ExpandedRollupCircuit witness manipulation | High     | Merkle path + event commitment constraints                       | Implemented                  |
| Empty batch proof acceptance               | Medium   | Empty batch constraint: old_root == new_root                     | Implemented                  |
| Proof deserialization attack               | High     | Canonical deserialization with error handling                    | Implemented                  |

### 2.5 Binding Layer Attack Surface

| Attack                                 | Severity | Mitigation                                              | Status                     |
| -------------------------------------- | -------- | ------------------------------------------------------- | -------------------------- |
| RF fingerprint forgery                 | High     | Hamming distance threshold + confidence scoring         | Stub - needs real hardware |
| Quantum commitment forgery (classical) | High     | Ed25519 signature verification                          | Implemented                |
| Quantum commitment forgery (PQC)       | High     | Dilithium signature verification                        | Implemented                |
| PQC key rotation downgrade             | Medium   | Phase downgrade rejected by PqcKeyRotationManager       | Implemented                |
| Provenance chain tampering             | High     | Append-only log with commitment links_to() verification | Implemented                |
| Provenance log version downgrade       | Medium   | Version byte check in from_bytes()                      | Implemented                |
| Authorization signature bypass         | Medium   | Empty authorization sig rejected in key rotation        | Implemented                |

### 2.6 Economic Attack Surface

| Attack                       | Severity | Mitigation                              | Status                                        |
| ---------------------------- | -------- | --------------------------------------- | --------------------------------------------- |
| Spam (griefing)              | Medium   | Fee schedule + quota system             | Implemented                                   |
| Fee manipulation             | Medium   | Fixed fee schedule (governance-updated) | Implemented                                   |
| Governance capture           | High     | Quadratic voting                        | Partial - still plutocratic at extreme wealth |
| Flash loan governance attack | Medium   | Time-locked voting                      | Implemented (Sprint 6)                        |
| Stake grinding               | Medium   | VRF-based leader selection              | Implemented                                   |
| Nothing-at-stake (economic)  | Medium   | Slashing + slashing threshold           | Implemented                                   |

### 2.7 Data Integrity Attack Surface

| Attack                        | Severity | Mitigation                                  | Status                    |
| ----------------------------- | -------- | ------------------------------------------- | ------------------------- |
| Snapshot tampering            | High     | BLAKE3 integrity hash + verify()            | Implemented               |
| Event graph corruption        | Medium   | Causal graph consistency checks             | Implemented               |
| CRDT divergence               | High     | TLA+-proven convergence                     | Proven                    |
| Nonce reset (disk corruption) | Medium   | redb durability guarantees                  | Partial - No nonce backup |
| Pruning data loss             | Medium   | Archive mode default + PrunedEventMetadata  | Implemented               |
| ProofBundle tampering         | High     | postcard serialization + verify_integrity() | Implemented               |

### 2.8 Supply Chain Attack Surface

| Attack                      | Severity | Mitigation                                  | Status                                    |
| --------------------------- | -------- | ------------------------------------------- | ----------------------------------------- |
| Compromised dependency      | High     | cargo-audit + cargo-vet + dependency policy | Implemented                               |
| Typosquatting               | Medium   | Cargo.lock pinning + manual review          | Implemented                               |
| Build system compromise     | Medium   | Reproducible builds (partial)               | Partial - Non-deterministic               |
| Insider threat (maintainer) | Medium   | SBOM generation + audit trail               | Partial - No multi-signature for releases |

---

## 3. Unmitigated Risks

The following risks have NO mitigation and should be prioritized:

| #   | Risk                                         | Impact                         | Likelihood | Priority |
| --- | -------------------------------------------- | ------------------------------ | ---------- | -------- |
| R1  | BN254 curve compromise (no alternative)      | All ZK invalid                 | Low        | P1       |
| R2  | Poseidon non-standard parameters             | Hash divergence from reference | Medium     | P2       |
| R3  | Long-range attack (no checkpoints)           | History rewrite                | Low        | P2       |
| R4  | Censorship by supermajority                  | Transaction exclusion          | Low        | P2       |
| R5  | No release multi-signature                   | Supply chain compromise        | Low        | P3       |
| R6  | RF fingerprint forgery (stub implementation) | Physical identity bypass       | High       | P1       |

---

## 4. Threat Actors

| Actor                            | Capability               | Target                      | Mitigation                           |
| -------------------------------- | ------------------------ | --------------------------- | ------------------------------------ |
| Solo validator                   | 1 vote, limited stake    | Minor griefing              | Fees + rate limiting                 |
| Colluding minority (< 1/3)       | Coordinated voting       | Censorship, equivocation    | Slashing + BFT                       |
| Colluding supermajority (>= 2/3) | Full consensus control   | Censorship, history rewrite | Social layer only                    |
| Network adversary                | Packet manipulation      | DOS, eclipse, partition     | QUIC + rate limiting                 |
| Cryptanalytic researcher         | Math breakthroughs       | Curve/hash breaks           | Crypto agility (CRYPTO_MIGRATION.md) |
| Quantum computer                 | Break Ed25519/ECDSA      | Key compromise              | Hybrid PQC mode                      |
| Nation-state                     | All of the above         | Targeted takedown           | Geographic distribution              |
| Malicious dependency maintainer  | Supply chain             | Backdoor in node binary     | cargo-vet + reproducible builds      |
| ZK operator                      | Submit fraudulent proofs | State root manipulation     | Groth16 proof verification on L1     |

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
    +-- redb corruption [Low probability - ACID transactions]
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

| #   | Recommendation                                  | Sprint             | Status      |
| --- | ----------------------------------------------- | ------------------ | ----------- |
| 1   | Implement consensus timeout/view-change         | Sprint 6 (Phase C) | Implemented |
| 2   | Add BLS proof-of-possession                     | Sprint 6 (Phase C) | Implemented |
| 3   | Implement crypto scheme versioning              | Sprint 6 (Phase D) | Implemented |
| 4   | Add genesis replay tool                         | Sprint 6 (Phase E) | Implemented |
| 5   | Add slashing appeals/undo process               | Sprint 6 (Phase E) | Implemented |
| 6   | Add time-locked voting                          | Sprint 6 (Phase F) | Implemented |
| 7   | Migrate Poseidon to reference constants         | Future             | Planned     |
| 8   | Side-channel audit for ZK and binding crates    | Future             | Not started |
| 9   | Schedule external security audit                | Post-Sprint 6      | Pending     |
| 10  | Implement leader privacy (threshold encryption) | Future             | Not planned |
| 11  | Real RF fingerprint hardware integration        | Future             | Stub only   |

---

## 7. STRIDE Threat Classification

The following STRIDE analysis provides detailed attack vectors, impact
assessments, current mitigations, and remaining gaps for each threat category.

### 7.1 Spoofing

Spoofing attacks involve an adversary pretending to be another entity — a node, a validator, or an event creator.

#### 7.1.1 Fake Events (Event Spoofing)

**Attack Vector**: An adversary creates events with a forged `creator` field, claiming they originated from a different node. For example, creating a `FinancialOp::Mint` event that appears to come from the treasury node.

**Impact**: HIGH. If accepted, a spoofed mint event would inflate the money supply. A spoofed transfer event would drain a victim's account.

**Current Mitigation**: Every event carries an Ed25519 signature (`event.signature`) over the event hash (`event.id`), which includes the `creator_pubkey`. The `Event::verify_signature()` method (in `substrate/src/event.rs`) checks that the signature is valid for the claimed public key. The `Event::validate()` method enforces both hash integrity and signature validity before any event enters the causal graph.

**Gap**: The `creator` ↔ `creator_pubkey` binding has been implemented (Sprint 4, Task A1). `Event::sign_with_keypair()` now sets `creator = blake3(creator_pubkey)`, and `validate_creator_binding()` enforces this invariant in constant time.

**Priority**: HIGH (mitigated).

#### 7.1.2 Fake Identities (Identity Spoofing)

**Attack Vector**: An adversary creates a DID that impersonates a real-world entity (e.g., `did:omnia:bank-of-america`).

**Impact**: MEDIUM. Could enable social engineering or unauthorized access to identity-bound resources.

**Current Mitigation**: The Identity shard (`shards/src/identity/`) uses Ed25519 keypairs bound to DIDs. A DID document includes the controller's public key, which must sign any updates.

**Gap**: No DID verification registry or certificate authority. Any node can create any DID string.

**Priority**: MEDIUM. Add DID namespace reservations or a verification registry.

#### 7.1.3 Fake Validators (Validator Impersonation)

**Attack Vector**: An adversary joins the network claiming to be a validator node and participates in consensus.

**Impact**: HIGH. A fake validator could prevent events from reaching the >2/3 supermajority threshold.

**Current Mitigation**: `ConsensusConfig::total_nodes` defines the validator set size. The `supermajority()` function computes the >2/3 threshold.

**Gap**: No validator authentication or on-chain validator registry.

**Priority**: MEDIUM. Validator authentication is needed when the network grows.

#### 7.1.4 RF Fingerprint Spoofing (Binding Layer)

**Attack Vector**: An adversary clones or forges an RF fingerprint to impersonate a physical device.

**Impact**: HIGH. A spoofed RF fingerprint could allow creation of fraudulent provenance events.

**Current Mitigation**: `RfFingerprint::verify()` uses Hamming distance comparison with a confidence threshold.

**Gap**: The RF fingerprinting implementation is a **stub** — it uses raw byte arrays instead of real RF spectral features.

**Priority**: HIGH. Real RF fingerprint capture requires hardware integration.

### 7.2 Tampering

#### 7.2.1 Event Payload Modification

**Attack Vector**: An adversary intercepts a gossip message, modifies the event payload, and re-broadcasts it.

**Impact**: HIGH. A modified transfer amount could redirect funds or create tokens.

**Current Mitigation**: The `Event::id` is a SHA-256 hash of all event fields. Any modification invalidates the hash and signature.

**Gap**: None significant. The hash-then-sign pattern provides strong tamper protection.

**Priority**: LOW. Current mitigation is strong.

#### 7.2.2 State Root Manipulation

**Attack Vector**: An adversary (ZK operator) posts a fraudulent state root to L1.

**Impact**: CRITICAL. A fraudulent state root would allow the operator to steal all bridged assets.

**Current Mitigation**: Groth16 proofs with `ExpandedRollupCircuit` add Merkle path verification and Poseidon-based state transition constraints. `ProofBundle::verify_integrity()` rejects bundles with missing or malformed data.

**Gap**: The Ethereum adapter's `verify_proof()` and Solidity contract's `verifyProof()` are still stubs.

**Priority**: CRITICAL. Implement real Groth16 verification in `OmniaRollup.sol` before mainnet.

#### 7.2.3 Provenance Chain Tampering (Binding Layer)

**Attack Vector**: An adversary attempts to modify or remove events from a provenance chain.

**Impact**: HIGH. Tampered provenance could obscure the chain of custody.

**Current Mitigation**: `ProvenanceLog` is append-only. `verify_chain()` checks `links_to()` relationships.

**Gap**: `links_to()` only verifies consecutive commitments have different data hashes; it does NOT verify cryptographic embedding of the previous commitment hash.

**Priority**: MEDIUM. Strengthen `links_to()` to verify cryptographic embedding.

#### 7.2.4 Shard State Mutation Outside `process_event()`

**Attack Vector**: A bug allows state mutation through `validate()` or `state_snapshot()`.

**Impact**: MEDIUM. Would break determinism guarantees.

**Current Mitigation**: Rust type system enforces `&self` on `validate()` and `state_snapshot()`.

**Gap**: No runtime enforcement. Relies on code review.

**Priority**: MEDIUM. Add property-based tests for purity.

### 7.3 Repudiation

#### 7.3.1 Denial of Event Creation

**Attack Vector**: A node creates an event and later denies having created it.

**Impact**: MEDIUM. Could undermine auditability.

**Current Mitigation**: Every event is signed with Ed25519 (`Event::sign_with_keypair()`). Quantum commitments provide additional non-repudiation via `QuantumCommitment::sign_classical()` and `QuantumCommitment::sign_hybrid()`.

**Gap**: No mechanism for key revocation or rotation in the substrate.

**Priority**: LOW. Key rotation is a Phase 1+ feature.

#### 7.3.2 Denial of Provenance Event

**Attack Vector**: A participant denies having transferred or verified an item.

**Impact**: MEDIUM. Could undermine supply chain accountability.

**Current Mitigation**: Each `ProvenanceEvent` contains a `QuantumCommitment` signed by the participant's key.

**Gap**: No integration between provenance event signatures and the causal graph.

**Priority**: LOW. Current design provides strong non-repudiation within the binding layer.

### 7.4 Information Disclosure

#### 7.4.1 ZK Proof Data Visibility

**Attack Vector**: An adversary observes ZK proof data posted to L1.

**Impact**: MEDIUM. Could reveal transaction data.

**Current Mitigation**: Groth16 proofs are zero-knowledge by construction. `ProofBundle::transition_proof` contains only the proof, not transaction data.

**Gap**: The `post_batch()` method posts full `batch_data` bytes to L1 for data availability.

**Priority**: MEDIUM. For full privacy, commit only via the Merkle root.

#### 7.4.2 Trusted Setup Secret Leakage

**Attack Vector**: An adversary compromises the secret randomness (`tau`) from the Powers of Tau ceremony.

**Impact**: CRITICAL. A compromised trusted setup allows generation of fake proofs.

**Current Mitigation**: Multi-party protocol with Proof of Knowledge (PoK) on BN254 G1.

**Gap**: The current ceremony is simulated (deterministic seeds).

**Priority**: HIGH. Implement production-grade ceremony before mainnet.

#### 7.4.3 Shard State Exposure

**Attack Vector**: An adversary queries `state_snapshot()` and extracts sensitive information.

**Impact**: MEDIUM. Financial privacy exposure.

**Current Mitigation**: `Shard::state_snapshot()` is only available locally, not via RPC.

**Gap**: No encryption of state snapshots.

**Priority**: MEDIUM. Encrypted state snapshots are a Phase 2+ feature.

#### 7.4.4 Private Key Compromise

**Attack Vector**: An adversary extracts the Ed25519 or Dilithium private key.

**Impact**: CRITICAL. A compromised key allows signing arbitrary events or forging commitments.

**Current Mitigation**: Keys are generated using `OsRng` and stored in memory only.

**Gap**: No HSM integration. No key encryption at rest. No multi-signature support.

**Priority**: HIGH. Add HSM support and key encryption before mainnet.

### 7.5 Denial of Service

#### 7.5.1 Gossip Flooding

**Attack Vector**: An adversary floods the gossip network with events.

**Impact**: HIGH. Could prevent timely event propagation.

**Current Mitigation**: Token-bucket rate limiter (200 burst/100s), `max_pending`, `max_events_per_message`, deduplication.

**Gap**: No peer reputation or blacklisting system.

**Priority**: MEDIUM. Add peer reputation scoring.

#### 7.5.2 Consensus Stall

**Attack Vector**: A Byzantine validator refuses to create witness events.

**Impact**: HIGH. Events would never achieve finality.

**Current Mitigation**: BFT tolerates up to f Byzantine nodes. View-change and round timeout implemented (Sprint 6).

**Gap**: If >1/3 of validators are offline or Byzantine, the network cannot make progress.

**Priority**: LOW. BFT bounds are by design.

#### 7.5.3 ZK Proving DoS

**Attack Vector**: An adversary submits many events requiring expensive ZK proof generation.

**Impact**: MEDIUM. Could delay batch finalization.

**Current Mitigation**: Configurable `batch_size` limit, cached trusted setup keys.

**Gap**: No per-event proving cost limit. No proving timeout.

**Priority**: MEDIUM. Add maximum circuit size and proving timeout.

### 7.6 Elevation of Privilege

#### 7.6.1 Validator Takeover

**Attack Vector**: An adversary gains control of a validator node.

**Impact**: CRITICAL. A compromised validator can create arbitrary events and influence finality.

**Current Mitigation**: BFT tolerates up to f Byzantine validators. Slashing implemented.

**Gap**: No validator rotation or ejection mechanism.

**Priority**: HIGH. Implement validator rotation before mainnet.

#### 7.6.2 Governance Manipulation

**Attack Vector**: An adversary accumulates governance tokens and passes self-serving proposals.

**Impact**: MEDIUM. Could centralize control.

**Current Mitigation**: Quadratic voting reduces large holder influence. Time-locked voting prevents flash loans.

**Gap**: No delegation mechanism. No minimum quorum.

**Priority**: MEDIUM. Strengthen governance mechanisms.

#### 7.6.3 PQC Key Rotation Downgrade

**Attack Vector**: An adversary attempts to downgrade from Hybrid to ClassicalOnly.

**Impact**: MEDIUM. Would make commitments vulnerable to quantum attacks.

**Current Mitigation**: `PqcKeyRotationManager` rejects phase downgrades.

**Gap**: No cryptographic verification of the `authorization_sig` — only emptiness check.

**Priority**: MEDIUM. Add proper signature verification for rotation authorization.

#### 7.6.4 Shard-Level Privilege Escalation

**Attack Vector**: An adversary exploits a bug to escalate privileges within a shard.

**Impact**: HIGH. Could create unlimited tokens or modify balances.

**Current Mitigation**: `FinancialState::apply()` enforces business rules. `Shard::validate()` provides pre-flight validation.

**Gap**: No ACL for shard operations. Any event can trigger any operation.

**Priority**: HIGH. Add ACL checks in `Shard::process_event()`.

### 7.7 STRIDE Threat Summary

| Category        | Threat                     | Impact   | Priority | Current Status                                                  |
| --------------- | -------------------------- | -------- | -------- | --------------------------------------------------------------- |
| Spoofing        | Fake Events                | HIGH     | HIGH     | Mitigated by Ed25519 + creator binding                          |
| Spoofing        | Fake Identities            | MEDIUM   | MEDIUM   | Mitigated by key-bound DIDs; gap in DID verification            |
| Spoofing        | Fake Validators            | HIGH     | MEDIUM   | Mitigated by BFT threshold; gap in validator authentication     |
| Spoofing        | RF Fingerprint Spoofing    | HIGH     | HIGH     | Stub implementation provides no real physical security          |
| Tampering       | Event Payload Mod.         | HIGH     | LOW      | Strongly mitigated by hash-then-sign                            |
| Tampering       | State Root Manip.          | CRITICAL | CRITICAL | Groth16 proofs implemented; Solidity verifier still stub        |
| Tampering       | Provenance Chain Tampering | HIGH     | MEDIUM   | Append-only log with links_to(); gap in cryptographic embedding |
| Tampering       | Shard State Mutation       | MEDIUM   | MEDIUM   | Mitigated by Rust type system; gap in runtime enforcement       |
| Repudiation     | Denial of Event Creation   | MEDIUM   | LOW      | Non-repudiable Ed25519 + Dilithium signatures                   |
| Repudiation     | Denial of Provenance Event | MEDIUM   | LOW      | Quantum commitments provide non-repudiation                     |
| Info Disclosure | ZK Proof Data Visibility   | MEDIUM   | MEDIUM   | ZK proofs are zero-knowledge; batch data may be posted to L1    |
| Info Disclosure | Trusted Setup Compromise   | CRITICAL | HIGH     | Multi-party PoK ceremony; production ceremony needed            |
| Info Disclosure | Shard State Exposure       | MEDIUM   | MEDIUM   | No external API; gap in encryption at rest                      |
| Info Disclosure | Private Key Compromise     | CRITICAL | HIGH     | Keys in memory only; no HSM                                     |
| DoS             | Gossip Flooding            | HIGH     | MEDIUM   | Rate limiting implemented                                       |
| DoS             | Consensus Stall            | HIGH     | LOW      | View-change implemented                                         |
| DoS             | ZK Proving DoS             | MEDIUM   | MEDIUM   | No circuit size limit or proving timeout                        |
| Elevation       | Validator Takeover         | CRITICAL | HIGH     | BFT + slashing; no validator rotation                           |
| Elevation       | Governance Manip.          | MEDIUM   | MEDIUM   | Quadratic voting + time-locks                                   |
| Elevation       | PQC Key Rotation Downgrade | MEDIUM   | MEDIUM   | Downgrade rejected; authorization sig not verified              |
| Elevation       | Shard Privilege Esc.       | HIGH     | HIGH     | No ACL for shard operations                                     |

### 7.8 Priority Action Items

1. **CRITICAL — Implement Solidity Groth16 verifier**: Replace stub `verifyProof()` in `OmniaRollup.sol` with a real Groth16 verifier contract.
2. **HIGH — Fix `creator` ↔ `creator_pubkey` binding**: ✅ Implemented (Sprint 4, Task A1).
3. **HIGH — Production trusted setup ceremony**: Implement real multi-party ceremony with secure randomness before mainnet.
4. **HIGH — RF fingerprint hardware integration**: Replace stub with real RF-DNA feature extraction.
5. **HIGH — Shard ACL**: Add authorization checks for privileged shard operations (minting, burning).
6. **HIGH — HSM support**: Add hardware security module integration for key protection.
7. **MEDIUM — Strengthen `links_to()` verification**: Verify cryptographic embedding of previous commitment hash.
8. **MEDIUM — Verify rotation authorization signatures**: Add cryptographic verification in `PqcKeyRotationManager`.
9. **MEDIUM — ZK proving DoS protection**: Add circuit size limits and proving timeouts.

### v0.1.69 Hardened Attack Surfaces

The v0.1.69 audit cycle hardened the following attack surfaces (see `SECURITY.md` and `AUDIT_FIX_NOTES.md` for details):

1. **Identity recovery** — SSS recovery path now uses real AES-256-GCM and properly updates DID authentication
2. **Biological ZK** — ZK proof paths over biometric/biological shard data hardened against witness malleability
3. **Cross-shard causality** — Causal ordering and replay protection across shards hardened
4. **Nonce store** — Nonce replay store hardened against race conditions and replay across shards
5. **Economics verifier** — Economics operation verifier hardened against fee bypass and unauthorized minting
6. **Ethereum settlement** — Live Ethereum RPC settlement adapter hardened against reorg, replay, and proof-binding gaps
7. **Rate limiting** — Gossip/REST rate limiting hardened against token-bucket bypass and per-peer abuse

---

🔙 **Back**: [Security](./) | 🔄 **Related**: [Threat Model](./THREAT_MODEL.md)
🚀 **Next**: [Security Audit](../reference/security-audit.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

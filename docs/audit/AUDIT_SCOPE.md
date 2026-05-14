# Omnia Protocol — Security Audit Scope

**Commit:** `SPRINT_3_COMMIT`
**Date:** 2026-03-05
**Document version:** 1.0

---

## 1. Purpose

This document defines the precise boundaries of the external security audit for the Omnia Protocol. It enumerates every component, file, and contract that the auditors are expected to review, together with explicit exclusions. The goal is to maximize audit coverage on consensus-critical and cryptography-dependent code while avoiding wasted effort on non-security-relevant artifacts.

---

## 2. In-Scope Components

### 2.1 Substrate Consensus Layer (`substrate/`)

The substrate crate implements the core causal graph, consensus engine, gossip protocol, and slashing mechanism. It is the most security-sensitive component: any bug here can lead to consensus divergence, equivocation, or network partition.

| Sub-component | Key files | Lines |
|---|---|---|
| Causal Graph | `substrate/src/causal_graph.rs` | 1,233 |
| Consensus Engine | `substrate/src/consensus.rs` | 777 |
| Gossip Protocol | `substrate/src/gossip.rs` | 896 |
| Slashing Engine | `substrate/src/slashing.rs` | 1,079 |
| Event Model | `substrate/src/event.rs` | 757 |
| Vector Clock | `substrate/src/vector_clock.rs` | 569 |
| Network (libp2p) | `substrate/src/network.rs` | 265 |
| CRDT primitives | `substrate/src/crdt/mod.rs`, `g_counter.rs`, `lww_register.rs`, `or_set.rs` | 1,373 |
| Crypto utilities | `substrate/src/crypto.rs` | 39 |
| Crate root | `substrate/src/lib.rs` | 493 |
| **Subtotal** | | **7,481** |

**Audit focus areas:**
- Hash-then-sign integrity in `Event::compute_hash()` / `Event::verify_signature()`
- Causal graph insertion correctness (`CausalGraph::insert()`) — no orphan events, no cycles
- Consensus finality thresholds (`supermajority()`) — BFT safety under f < n/3
- Gossip deduplication and queue bounds (`seen_events`, `max_pending`)
- Slashing point accumulation and threshold logic (saturating arithmetic, no overflow)
- Equivocation detection (`check_equivocation()`) — correct creator+sequence+hash comparison
- Liveness monitoring (`check_liveness()`) — threshold boundary conditions
- Vector clock merge correctness (partial order preservation)

### 2.2 ZK Circuit (`zk/`)

The zero-knowledge proof system provides L2 state transition verification using arkworks R1CS constraints and Groth16 proofs on the BN254 curve. The circuit is currently minimal — auditors should evaluate both the existing constraints and the implications of the limited constraint set.

| Sub-component | Key files | Lines |
|---|---|---|
| R1CS Circuit | `zk/src/circuit.rs` | 242 |
| Groth16 Prover | `zk/src/prover.rs` | 174 |
| Proof Verification | `zk/src/proof.rs` | 119 |
| Proof Bundle | `zk/src/proof_bundle.rs` | 304 |
| ZK Operator | `zk/src/operator.rs` | 228 |
| Settlement Layer | `zk/src/settlement/mod.rs`, `ethereum.rs`, `solana.rs`, `celestia.rs`, `bitcoin.rs` | 415 |
| Crate root | `zk/src/lib.rs` | 62 |
| **Subtotal** | | **1,544** |

**Audit focus areas:**
- Circuit soundness: `RollupCircuit` currently enforces a single `enforce_equal` constraint (`new_state_root == expected_new_state_root`). This does not prevent a prover from claiming any state transition — the old state root and event count are unconstrained witnesses.
- Trusted setup correctness and uniqueness for Groth16 (circuit-specific, not universal)
- Proof serialization/deserialization integrity in `ProofBundle`
- Settlement layer trait contracts — L1-agnostic verification correctness
- Public input extraction and verification (`RollupCircuit::public_input()`)
- Legacy stub circuit (`RollupCircuitLegacy`) — confirm it is test-only and gated by `#[cfg(test)]`

### 2.3 PQC Verification — Binding Layer (`binding/`)

The binding crate provides quantum-resistant cryptographic commitments using a hybrid Ed25519 + CRYSTALS-Dilithium approach, along with provenance tracking and physical shard binding.

| Sub-component | Key files | Lines |
|---|---|---|
| Quantum Commitments | `binding/src/quantum_commit.rs` | 478 |
| Provenance Chain | `binding/src/provenance.rs` | 482 |
| Physical Shard | `binding/src/physical_shard.rs` | 462 |
| Anchor | `binding/src/anchor.rs` | 251 |
| RF Fingerprint (stub) | `binding/src/rf_fingerprint.rs` | 174 |
| Crate root | `binding/src/lib.rs` | 69 |
| **Subtotal** | | **1,916** |

**Audit focus areas:**
- Hybrid verification correctness in `QuantumCommitment::verify()` — both Ed25519 and Dilithium must pass in `Hybrid` phase
- Phase transition logic (`CommitmentPhase`): ensure `ClassicalOnly` does not accept Dilithium-only commitments and `PostQuantum` does not accept classical-only commitments
- `verify_dilithium()` — empty signature/key rejection (was previously returning `true` unconditionally in Sprint 1)
- `verify_ed25519()` — proper error handling on deserialization failures
- Provenance chain integrity — `links_to()` logic correctness
- RF fingerprint stub — confirm it is explicitly out of scope for production use

### 2.4 Slashing (`substrate/src/slashing.rs`)

Listed separately for emphasis due to its economic impact. The slashing engine tracks validator offenses and determines slash/ejection outcomes.

**Key audit points:**
- Point accumulation uses `saturating_add` — no overflow, but does this allow infinite accumulation without ejection?
- Equivocation detection relies on `EventId` comparison — are there hash collision scenarios?
- `SlashingEngine` now supports persistent storage via the `SlashingStore` trait (`SledSlashingStore` for disk, `InMemorySlashingStore` for tests). The default `new()` constructor still uses `InMemorySlashingStore`; production nodes must use `with_store(SledSlashingStore::open(...))`.
- `persist_state()` is called after every mutation but only logs a warning on failure (does not rollback)
- `check_liveness()` threshold boundary: `inactive_rounds > threshold` uses strict greater-than; is this correct?
- No slashing decay or forgiveness mechanism — is this intentional?

### 2.5 Fee Enforcement (`shards/src/fee_schedule.rs`, `economics/src/quota.rs`)

The fee enforcement system prevents spam by deducting UBC tokens from the caller's quota before processing shard operations.

| Sub-component | Key files | Lines |
|---|---|---|
| Fee Schedule | `shards/src/fee_schedule.rs` | 187 |
| Quota System | `economics/src/quota.rs` | 141 |
| Shard Router (fee deduction) | `shards/src/router.rs` | 206 |
| UBC Token | `economics/src/ubc.rs` | 84 |
| **Subtotal** | | **618** |

**Audit focus areas:**
- Fee deduction happens before shard dispatch — cannot bypass by crashing mid-operation
- `QuotaSystem::spend()` returns error on insufficient balance — no negative balances
- Replay protection via nonce tracking in `ShardRouter::route_event()` — strictly increasing nonces per `creator_pubkey`
- `FeeSchedule::zero()` exists for testing — verify it cannot be used in production paths
- Epoch reset (`advance_epoch()`) forfeits unspent balance — no balance carry-over exploits

### 2.6 Shard Routing (`shards/src/router.rs`)

The shard router is the central dispatch point that deserializes payloads, enforces fees, checks nonces, and routes operations to the appropriate shard.

**Key audit points:**
- `route_event()` processing order: nonce check → fee deduction → route — is this order correct?
- Cross-shard message deserialization uses `bincode::deserialize()` — is this a deserialization-of-unevicted-data risk?
- `pubkey_to_did()` is a simple hex encoding — no collision resistance beyond the 32-byte Ed25519 key space
- `ShardRouter::new_without_fees()` bypasses fee enforcement — verify it is test-only

---

## 3. Out-of-Scope

The following components and artifacts are explicitly excluded from the audit scope. Auditors should not spend time reviewing these unless they directly impact an in-scope component.

| Exclusion | Location | Rationale |
|---|---|---|
| Docker configuration | `docker/` | Deployment infrastructure, not protocol logic |
| CI pipeline | `.github/` (if present) | Build/deployment automation |
| Documentation | `docs/` (except this audit directory) | Not executable code |
| Diagrams | `diagrams/`, `assets/` | Visual aids only |
| RF Fingerprinting stub | `binding/src/rf_fingerprint.rs` | Explicitly a stub — uses Hamming distance, not real RF capture; uses `f64` for similarity which is acceptable outside consensus |
| Shell scripts | `apply-fixes.sh` | Build/tooling helpers |
| Ethereum smart contract | `zk/contracts/ethereum/OmniaRollup.sol` | Not yet integrated; separate audit when activated |
| Benchmarks | `substrate/benches/` | Performance measurement, not security |
| Legacy test stubs | `zk/src/circuit.rs` (RollupCircuitLegacy) | Gated by `#[cfg(test)]`, never compiled in production |

---

## 4. Key Assumptions

The audit is conducted under the following assumptions. If any assumption is violated, the audit findings may not be valid.

1. **Stable Rust toolchain**: The code is compiled with a stable Rust compiler. Unsafe code blocks, if any, are individually reviewed.
2. **Trusted setup for Groth16**: The Groth16 trusted setup (powers of tau and circuit-specific phase 2) is assumed to be conducted honestly. A malicious setup can generate false proofs. This is a known limitation of Groth16 (circuit-specific, not universal like PLONK's universal setup).
3. **No hardware attacks**: Side-channel attacks (timing, power analysis, EM emanation) are out of scope. The audit assumes attackers cannot physically access validator hardware.
4. **Dependency trust**: Third-party crates (arkworks, ed25519-dalek, pqc-dilithium, blake3, etc.) are assumed to be correctly implemented. The audit reviews how they are *used*, not their internal correctness.
5. **Network assumptions**: The network is partially synchronous — messages are eventually delivered but may be delayed or reordered. The BFT model assumes f < n/3 Byzantine nodes.
6. **Single-developer codebase**: The protocol has been primarily developed by a single engineer. This increases the risk of blind spots and implicit assumptions that multi-person review would catch.

---

## 5. Trust Boundaries

Trust boundaries define where data crosses from one trust domain to another. Every crossing is a potential attack surface.

### 5.1 Network Boundary (libp2p)

All data arriving over the libp2p gossip network is untrusted. This includes:
- Gossip messages containing events from remote nodes
- Bootstrap peer multiaddresses
- QUIC connection initiation

**Crossing point**: `substrate/src/network.rs` → `substrate/src/gossip.rs` → `substrate/src/causal_graph.rs`

### 5.2 User Input Boundary (Event Payloads)

Events submitted by users (or external systems) contain arbitrary payloads that are deserialized and routed to shards.

**Crossing point**: `Event.payload` bytes → `ShardPayload::from_bytes()` → `ShardRouter::route_event()` → `Shard::process_event()`

### 5.3 Cryptographic Boundary (Signatures and Proofs)

Cryptographic artifacts — Ed25519 signatures, Dilithium signatures, Groth16 proofs — cross from the "asserted" domain to the "verified" domain at specific verification points.

**Crossing points**:
- `Event::verify_signature()` — Ed25519 signature check
- `QuantumCommitment::verify()` — Ed25519 + Dilithium hybrid check
- `Groth16::verify()` (via arkworks) — ZK proof verification
- `blake3::hash()` — data integrity verification

### 5.4 Economic Boundary (Fee and Slashing)

Economic state transitions — fee deduction, UBC minting, slashing confiscation — cross from "claimed" to "settled" domains.

**Crossing points**:
- `QuotaSystem::spend()` — fee deduction from UBC balance
- `SlashingEngine::record_offense()` — slash point accumulation
- `UbcToken::mint_monthly()` — epoch-based UBC issuance

---

## 6. Commit Reference

The audit covers the codebase at commit `SPRINT_3_COMMIT`. All file paths and line counts referenced in this document correspond to this commit. Auditors must verify they are reviewing the correct commit before beginning work.

---

## 7. Deliverables Expected from Auditors

1. **Vulnerability report** — each finding with severity (Critical / High / Medium / Low / Informational), description, affected code, and remediation recommendation
2. **Code quality observations** — architectural concerns, unsafe patterns, missing error handling
3. **Formal verification cross-reference** — comparison of TLA+ model assumptions with Rust implementation
4. **Summary assessment** — overall security posture and recommended next steps

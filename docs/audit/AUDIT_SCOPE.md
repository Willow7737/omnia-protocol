# Omnia Protocol — Security Audit Scope

**Version:** v4.0.0
**Date:** 2026-03-05
**Document version:** 2.0

---

## 1. Purpose

This document defines the precise boundaries of the external security audit for the Omnia Protocol. It enumerates every component, file, and contract that the auditors are expected to review, together with explicit exclusions. The goal is to maximize audit coverage on consensus-critical and cryptography-dependent code while avoiding wasted effort on non-security-relevant artifacts.

---

## 2. In-Scope Components

### 2.1 Substrate Consensus Layer (`substrate/`)

The substrate crate implements the core causal graph, consensus engine, gossip protocol, and slashing mechanism. It is the most security-sensitive component: any bug here can lead to consensus divergence, equivocation, or network partition.

| Sub-component | Key files | Lines |
|---|---|---|
| Causal Graph | `substrate/src/causal_graph.rs` | ~1,233 |
| Consensus Engine | `substrate/src/consensus.rs` | ~777 |
| Gossip Protocol | `substrate/src/gossip.rs` | ~896 |
| Slashing Engine | `substrate/src/slashing.rs` | ~1,079 |
| Event Model | `substrate/src/event.rs` | ~757 |
| Vector Clock | `substrate/src/vector_clock.rs` | ~569 |
| Network (libp2p) | `substrate/src/network.rs` | ~265 |
| State Snapshot | `substrate/src/snapshot.rs` | — |
| CRDT primitives | `substrate/src/crdt/mod.rs`, `g_counter.rs`, `lww_register.rs`, `or_set.rs` | ~1,373 |
| Crypto utilities | `substrate/src/crypto.rs` | ~39 |
| Crate root | `substrate/src/lib.rs` | ~493 |

**Audit focus areas:**
- Hash-then-sign integrity in `Event::compute_hash()` / `Event::verify_signature()`
- Causal graph insertion correctness (`CausalGraph::insert()`) — no orphan events, no cycles
- Consensus finality thresholds (`supermajority()`) — BFT safety under f < n/3
- Gossip deduplication and queue bounds (`seen_events`, `max_pending`)
- Slashing point accumulation and threshold logic (saturating arithmetic, no overflow)
- Equivocation detection (`check_equivocation()`) — correct creator+sequence+hash comparison
- Liveness monitoring (`check_liveness()`) — threshold boundary conditions
- Vector clock merge correctness (partial order preservation)
- State snapshot serialization and integrity verification

### 2.2 Node Binary and HTTP API (`node/`)

The node crate provides the binary entrypoint, CLI subcommands, HTTP server, and REST API. This is a new attack surface with significant security implications.

| Sub-component | Key files | Lines |
|---|---|---|
| Binary entrypoint | `node/src/main.rs` | ~483 |
| Library root | `node/src/lib.rs` | ~18 |
| Configuration | `node/src/config.rs` | ~577 |
| HTTP router | `node/src/http.rs` | ~78 |
| Application state | `node/src/state.rs` | ~145 |
| API router + OpenAPI | `node/src/api/mod.rs` | ~83 |
| API authentication | `node/src/api/auth.rs` | ~645 |
| API error types | `node/src/api/errors.rs` | ~70 |
| Node API | `node/src/api/node.rs` | ~91 |
| Events API | `node/src/api/events.rs` | ~187 |
| Shards API | `node/src/api/shards.rs` | ~197 |
| Governance API | `node/src/api/governance.rs` | ~196 |
| Economics API | `node/src/api/economics.rs` | ~169 |

**Audit focus areas:**
- **JWT authentication** — All 9 API endpoints require valid JWT tokens; configured via `OMNIA_JWT_SECRET` (FIND-001). Review the `auth.rs` implementation for correctness and timing attacks.
- **Rate limiting** — Per-IP token-bucket rate limiter; configured via `OMNIA_RATE_LIMIT_RPS` (FIND-001)
- **ACL authorization** — Only authorized callers can access the API; configured via `OMNIA_AUTHORIZED_CALLERS`. Privileged operations (mint, advance_epoch) require admin JWT (FIND-001)
- **Encrypted key storage** — `run_keygen()` supports `--passphrase` for AES-256-GCM encryption; `EncryptedKeyStore` provides encrypted storage (FIND-010)
- **Trusted setup ceremony** — `setup-contribute` and `setup-verify` subcommands with no multi-party coordination
- **TOML config parsing** — `node_id` now `Option<u64>` in both TOML and runtime config (FIND-013 fixed the previous `Option<u16>` mismatch)
- **redb persistence** — `RedbSlashingStore` and `RedbNonceStore` provide production-quality persistence with ACID transactions
- **Nonce persistence** — `RedbNonceStore` for replay protection across restarts
- **Slashing persistence** — `RedbSlashingStore` configured automatically with `SlashingUndoManager` for rollback (FIND-011)
- **Payload size enforcement** — `MAX_PAYLOAD_SIZE` check at both HTTP and gossip layers (FIND-021)
- **Event signing** — `generate_keypair()` creates a fresh keypair for each API-submitted event; no key reuse or identity binding

### 2.3 ZK Circuit (`zk/`)

The zero-knowledge proof system provides L2 state transition verification using arkworks R1CS constraints and Groth16 proofs on the BN254 curve.

| Sub-component | Key files | Lines |
|---|---|---|
| R1CS Circuit | `zk/src/circuit.rs` | ~242 |
| Groth16 Prover | `zk/src/prover.rs` | ~174 |
| Proof Verification | `zk/src/proof.rs` | ~119 |
| Proof Bundle | `zk/src/proof_bundle.rs` | ~304 |
| ZK Operator | `zk/src/operator.rs` | ~228 |
| Trusted Setup | `zk/src/setup.rs` | — |
| Settlement Layer | `zk/src/settlement/mod.rs`, `ethereum.rs`, `solana.rs`, `celestia.rs`, `bitcoin.rs` | ~415 |
| Crate root | `zk/src/lib.rs` | ~62 |

**Audit focus areas:**
- Circuit soundness: `RollupCircuit` enforces a single `enforce_equal` constraint; `ExpandedRollupCircuit` uses a simplified field-addition hash placeholder
- Trusted setup correctness and uniqueness for Groth16 (circuit-specific, not universal)
- Proof serialization/deserialization integrity in `ProofBundle`
- Settlement layer trait contracts — L1-agnostic verification correctness
- Public input extraction and verification
- Legacy stub circuit (`RollupCircuitLegacy`) — confirm it is test-only and gated by `#[cfg(test)]`

### 2.4 PQC Verification — Binding Layer (`binding/`)

The binding crate provides quantum-resistant cryptographic commitments using a hybrid Ed25519 + CRYSTALS-Dilithium approach, along with provenance tracking and physical shard binding.

| Sub-component | Key files | Lines |
|---|---|---|
| Quantum Commitments | `binding/src/quantum_commit.rs` | ~478 |
| Provenance Chain | `binding/src/provenance.rs` | ~482 |
| Physical Shard | `binding/src/physical_shard.rs` | ~462 |
| Anchor | `binding/src/anchor.rs` | ~251 |
| RF Fingerprint (stub) | `binding/src/rf_fingerprint.rs` | ~174 |
| Crate root | `binding/src/lib.rs` | ~69 |

**Audit focus areas:**
- Hybrid verification correctness in `QuantumCommitment::verify()` — both Ed25519 and Dilithium must pass in `Hybrid` phase
- Phase transition logic (`CommitmentPhase`): ensure `ClassicalOnly` does not accept Dilithium-only commitments and `PostQuantum` does not accept classical-only commitments
- `verify_dilithium()` — empty signature/key rejection (was previously returning `true` unconditionally)
- `verify_ed25519()` — proper error handling on deserialization failures
- Provenance chain integrity — `links_to()` logic correctness
- RF fingerprint stub — confirm it is explicitly out of scope

### 2.5 Slashing (`substrate/src/slashing.rs`)

Listed separately for emphasis due to its economic impact.

**Key audit points:**
- Point accumulation uses `saturating_add` — no overflow, but does this allow infinite accumulation without ejection?
- Equivocation detection relies on `EventId` comparison — are there hash collision scenarios?
- `SlashingEngine` supports persistent storage via `SlashingStore` trait (`RedbSlashingStore` for disk, `InMemorySlashingStore` for tests). The `omnia-node` binary configures redb persistence automatically.
- `persist_state()` is called after every mutation but only logs a warning on failure (does not rollback)
- `check_liveness()` threshold boundary: `inactive_rounds > threshold` uses strict greater-than; is this correct?
- No slashing decay or forgiveness mechanism — is this intentional?

### 2.6 Fee Enforcement (`shards/src/fee_schedule.rs`, `economics/src/quota.rs`)

The fee enforcement system prevents spam by deducting UBC tokens from the caller's quota before processing shard operations.

| Sub-component | Key files | Lines |
|---|---|---|
| Fee Schedule | `shards/src/fee_schedule.rs` | ~187 |
| Quota System | `economics/src/quota.rs` | ~141 |
| Shard Router (fee deduction) | `shards/src/router.rs` | ~206 |
| Nonce Store | `shards/src/nonce_store.rs` (NonceStore trait, RedbNonceStore) | ~289 |
| UBC Token | `economics/src/ubc.rs` | ~84 |

**Audit focus areas:**
- Fee deduction happens before shard dispatch — cannot bypass by crashing mid-operation
- `QuotaSystem::spend()` returns error on insufficient balance — no negative balances
- Replay protection via nonce tracking in `ShardRouter::route_event()` — strictly increasing nonces per `creator_pubkey`
- `RedbNonceStore` provides persistent replay protection across restarts
- `FeeSchedule::zero()` exists for testing — verify it cannot be used in production paths
- Epoch reset (`advance_epoch()`) forfeits unspent balance — no balance carry-over exploits

### 2.7 Shard Routing (`shards/src/router.rs`)

The shard router is the central dispatch point that deserializes payloads, enforces fees, checks nonces, and routes operations to the appropriate shard.

**Key audit points:**
- `route_event()` processing order: nonce check → fee deduction → route — is this order correct?
- Cross-shard message deserialization uses `postcard::from_bytes()` — is this a deserialization-of-unevicted-data risk?
- `pubkey_to_did()` is a simple hex encoding — no collision resistance beyond the 32-byte Ed25519 key space
- `ShardRouter::new_without_fees()` bypasses fee enforcement — verify it is test-only

### 2.8 Chaos Testing Framework (`chaos-tests/`)

The chaos testing framework provides simulation-based validation of protocol safety and liveness under adverse conditions. While not a runtime component, it validates the same invariants as the TLA+ model and should be reviewed for correctness.

| Sub-component | Key files | Lines |
|---|---|---|
| ChaosNode + ChaosNetwork | `chaos-tests/src/lib.rs` | ~982 |

**Key audit points:**
- Node ID derivation uses `blake3(pubkey)` — matches substrate's `Event::sign_with_keypair()` behavior
- `check_safety()` correctly detects conflicting commits by comparing `(creator, sequence)` → `EventId` uniqueness
- `check_liveness()` only checks if *some* events are committed — not comprehensive
- `collect_missing_ancestors()` uses recursive traversal — potential stack overflow for deep event chains
- `sync_all()` runs up to 10 rounds — is this sufficient for convergence?

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
| Shell scripts | `scripts/`, `apply-fixes.sh` | Build/tooling helpers |
| Ethereum smart contract | `zk/contracts/ethereum/OmniaRollup.sol` | Not yet integrated; separate audit when activated |
| Benchmarks | `substrate/benches/` | Performance measurement, not security |
| Legacy test stubs | `zk/src/circuit.rs` (RollupCircuitLegacy) | Gated by `#[cfg(test)]`, never compiled in production |

---

## 4. Key Assumptions

The audit is conducted under the following assumptions. If any assumption is violated, the audit findings may not be valid.

1. **Stable Rust toolchain**: The code is compiled with a stable Rust compiler (1.85+). Unsafe code blocks, if any, are individually reviewed.
2. **Trusted setup for Groth16**: The Groth16 trusted setup (powers of tau and circuit-specific phase 2) is assumed to be conducted honestly. A malicious setup can generate false proofs. This is a known limitation of Groth16. The `omnia-node` binary includes `setup-contribute` and `setup-verify` subcommands for local ceremony simulation, but no multi-party coordination protocol exists yet.
3. **No hardware attacks**: Side-channel attacks (timing, power analysis, EM emanation) are out of scope. The audit assumes attackers cannot physically access validator hardware.
4. **Dependency trust**: Third-party crates (arkworks, ed25519-dalek, pqc-dilithium, blake3, etc.) are assumed to be correctly implemented. The audit reviews how they are *used*, not their internal correctness.
5. **Network assumptions**: The network is partially synchronous — messages are eventually delivered but may be delayed or reordered. The BFT model assumes f < n/3 Byzantine nodes.
6. **Single-developer codebase**: The protocol has been primarily developed by a single engineer. This increases the risk of blind spots and implicit assumptions.
7. **Docker compose is for development only**: The `docker-compose.yml` uses valid `OMNIA_NODE_ID` numeric values and `OMNIA_HTTP_PORT=8080` for all containers, with host port mapping (9090-9094 → 8080). It is not a production deployment configuration.

---

## 5. Trust Boundaries

Trust boundaries define where data crosses from one trust domain to another. Every crossing is a potential attack surface.

### 5.1 Network Boundary (libp2p)

All data arriving over the libp2p gossip network is untrusted. This includes:
- Gossip messages containing events from remote nodes
- Bootstrap peer multiaddresses
- QUIC connection initiation

**Crossing point**: `substrate/src/network.rs` → `substrate/src/gossip.rs` → `substrate/src/causal_graph.rs`

### 5.2 HTTP API Boundary (NEW)

All data arriving via the HTTP REST API is untrusted. This includes:
- Event submissions with arbitrary payloads
- Shard operations (including mint, spend)
- Governance proposals and votes
- Economics transfers

**Crossing point**: `node/src/http.rs` → `node/src/api/*.rs` → substrate/shards/economics crates

### 5.3 User Input Boundary (Event Payloads)

Events submitted by users (or external systems) contain arbitrary payloads that are deserialized and routed to shards.

**Crossing point**: `Event.payload` bytes → `ShardPayload::from_bytes()` → `ShardRouter::route_event()` → `Shard::process_event()`

### 5.4 Cryptographic Boundary (Signatures and Proofs)

Cryptographic artifacts — Ed25519 signatures, Dilithium signatures, Groth16 proofs — cross from the "asserted" domain to the "verified" domain at specific verification points.

**Crossing points**:
- `Event::verify_signature()` — Ed25519 signature check
- `QuantumCommitment::verify()` — Ed25519 + Dilithium hybrid check
- `Groth16::verify()` (via arkworks) — ZK proof verification
- `blake3::hash()` — data integrity verification

### 5.5 Economic Boundary (Fee and Slashing)

Economic state transitions — fee deduction, UBC minting, slashing confiscation — cross from "claimed" to "settled" domains.

**Crossing points**:
- `QuotaSystem::spend()` — fee deduction from UBC balance
- `SlashingEngine::record_offense()` — slash point accumulation
- `UbcToken::mint_monthly()` — epoch-based UBC issuance

### 5.6 Persistence Boundary (redb)

Data persisted to redb databases crosses from volatile to durable storage. redb provides ACID transactions and crash-safe durability.

**Crossing points**:
- `RedbSlashingStore::persist_state()` — slashing state durability
- `RedbNonceStore::set_nonce()` — nonce state durability

---

## 6. Deliverables Expected from Auditors

1. **Vulnerability report** — each finding with severity (Critical / High / Medium / Low / Informational), description, affected code, and remediation recommendation
2. **Code quality observations** — architectural concerns, unsafe patterns, missing error handling
3. **Formal verification cross-reference** — comparison of TLA+ model assumptions with Rust implementation
4. **Summary assessment** — overall security posture and recommended next steps

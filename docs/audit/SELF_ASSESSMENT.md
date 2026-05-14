# Omnia Protocol — Security Self-Assessment

**Commit:** `SPRINT_3_COMMIT`
**Date:** 2026-03-05
**Document version:** 1.0

---

## 1. Purpose

This document provides an honest and transparent assessment of the Omnia Protocol's current security posture as of Sprint 3. It catalogs known issues that have been fixed, remaining risks that have not yet been addressed, test coverage status, fuzzing efforts, and dependency audit results. The intent is to give auditors a complete picture of where the protocol stands — both strengths and weaknesses — so they can focus their efforts on the areas that need the most attention.

---

## 2. Known Issues and Mitigations Applied

The following issues were identified during Sprint 1 and Sprint 2 and have been resolved. Each entry describes the original vulnerability, the mitigation applied, and the residual risk (if any).

### 2.1 f64 → Fixed-Point (Sprint 2)

**Original issue:** Consensus-critical calculations used IEEE 754 floating-point (`f64`) arithmetic. Floating-point operations are non-deterministic across platforms due to different FPU rounding modes, register widths (80-bit x87 vs. 64-bit SSE), and compiler optimizations. Non-determinism in consensus leads to divergence — different nodes computing different results for the same inputs, which violates the agreement invariant.

**Mitigation:** All consensus-related arithmetic was migrated to fixed-point integers (`u64` with explicit scaling factors). The `economics/src/fixed_point.rs` module (486 lines) implements a `FixedPoint<U>` type with deterministic multiplication, division, and square root operations. All fee calculations, governance voting power, and UBC token operations now use integer arithmetic exclusively.

**Residual risk:** The RF fingerprinting module (`binding/src/rf_fingerprint.rs`) still uses `f64` for Hamming distance similarity calculation. This is acceptable because RF fingerprinting is a stub module that does not participate in consensus — it is explicitly out of scope for production use and documented as such.

### 2.2 PQC verify() Returning True Unconditionally (Sprint 2)

**Original issue:** The `QuantumCommitment::verify()` method in `binding/src/quantum_commit.rs` contained a stub implementation that returned `true` regardless of the input. This meant that any commitment — even one with a forged or empty Dilithium signature — would pass verification in the `Hybrid` and `PostQuantum` phases. This completely negated the post-quantum security guarantees.

**Mitigation:** The `verify_dilithium()` method was replaced with a real verification call to `pqc_dilithium::verify()`. The method now:
1. Checks that the Dilithium public key is non-empty (returns `false` if empty)
2. Checks that the Dilithium signature is non-empty (returns `false` if empty)
3. Calls `pqc_dilithium::verify(&self.dilithium_sig, hash.as_bytes(), &public_key.dilithium)` and returns the result

**Residual risk:** No constant-time guarantee for the `pqc_dilithium::verify()` function. If the underlying crate leaks timing information, it could enable side-channel attacks. This is an upstream dependency concern, not a protocol implementation issue.

### 2.3 No Fee Enforcement (Sprint 2)

**Original issue:** The `ShardRouter` did not enforce fees before processing shard operations. Any event with a valid signature and nonce could trigger arbitrary shard operations without paying UBC tokens. This allowed spam attacks at zero cost — an attacker could flood the network with shard operations, consuming CPU and storage without any economic penalty.

**Mitigation:** A two-layer fee enforcement system was added:
1. **FeeSchedule** (`shards/src/fee_schedule.rs`, 187 lines): Maps each `ShardOp` variant to a fixed `u64` fee in UBC units. Standard fees range from 2 UBC (identity operations) to 15 UBC (cross-shard operations).
2. **QuotaSystem** (`economics/src/quota.rs`, 141 lines): Manages per-DID UBC balances with atomic `spend()` and `reward()` operations. Insufficient balance returns `EconomicsError`.

The `ShardRouter::route_event()` method now: (a) checks the nonce for replay protection, (b) looks up the fee for the operation, (c) deducts the fee from the caller's quota, and (d) routes the operation only if the fee deduction succeeds. Fee deduction happens before shard dispatch — if the operation fails, the fee is not refunded.

**Residual risk:** `ShardRouter::new_without_fees()` creates a router with `FeeSchedule::zero()`, bypassing all fee enforcement. This is intended for testing but could be accidentally used in production. The `Default` impl for `ShardRouter` also uses `new_without_fees()`.

### 2.4 No Slashing (Sprint 2)

**Original issue:** The protocol had no mechanism to penalize Byzantine validators. A validator could equivocate, go offline, or attest to invalid data with no economic consequences. This violated the BFT security model's requirement that misbehavior be economically disincentivized.

**Mitigation:** A `SlashingEngine` (`substrate/src/slashing.rs`, 1,079 lines) was implemented with:
- Three offense types: Equivocation (500 points), LivenessViolation (100 points), InvalidAttestation (300 points)
- Two thresholds: Slash (500 points — stake forfeited), Ejection (2000 points — removed from validator set)
- Equivocation detection via `check_equivocation()` — compares `creator + sequence + event_id`
- Liveness monitoring via `check_liveness()` — compares inactive rounds against a threshold
- Point accumulation uses `saturating_add` to prevent overflow
- Persistent storage via `SlashingStore` trait with `SledSlashingStore` backend (added in Sprint 3)

**Residual risk:** The `SlashingEngine` now supports persistent storage via the `SlashingStore` trait with a `SledSlashingStore` backend (added in Sprint 3). However, the default `new()` constructor still uses `InMemorySlashingStore` — production nodes must explicitly use `with_store(SledSlashingStore::open(...))` to get persistence. Additionally, `persist_state()` logs a warning on failure but does not rollback the in-memory state. Finally, `record_offense()` returns a `SlashOutcome` but does not actually confiscate stake or emit a slashing event to the network.

### 2.5 ZK Hash-Chain Stub (Sprint 2)

**Original issue:** The ZK proof system used a hash-chain stub (`RollupCircuitLegacy`) that computed a BLAKE3 hash chain over the old state root, events, and new state root. This is not a zero-knowledge proof — it provides no privacy, no soundness, and no succinctness. Anyone can forge a "proof" by computing the hash chain with arbitrary data.

**Mitigation:** A real R1CS circuit (`RollupCircuit` in `zk/src/circuit.rs`, 242 lines) was implemented using arkworks on the BN254 curve with Groth16 proving and verification. The circuit:
- Takes old state root, new state root, and event count as witnesses (private inputs)
- Takes expected new state root as a public input
- Enforces `new_state_root == expected_new_state_root`
- Supports `from_state_roots()` for creating circuits from byte-level state roots
- Supports `empty()` for trusted setup key generation

**Residual risk:** The circuit is **minimal** — it enforces only one constraint (equality check on state roots). It does NOT verify:
- That the old state root corresponds to the actual previous state
- That the state transition is valid (correct event application)
- Merkle path inclusion proofs
- Event count correctness

A malicious prover can generate a valid proof for ANY state transition, as long as they know the new state root. The old state root and event count are unconstrained witnesses. The legacy stub circuit is retained under `#[cfg(test)]` for backward-compatible testing.

---

## 3. Remaining Risks

These risks are known but not yet mitigated. They are listed in approximate order of severity.

### 3.1 ZK Circuit Is Minimal (Critical) → 🟡 Partially Addressed (Sprint 3)

The original `RollupCircuit` enforced only `new_state_root == expected_new_state_root`. Sprint 3 added the `ExpandedRollupCircuit` which includes:
- Merkle path inclusion verification (proving specific events were applied to produce the new state root)
- Per-event state transition constraints (proving each event's application is valid)
- Old state root binding (proving the transition starts from the correct previous state)

**Remaining gap:** The `ExpandedRollupCircuit` uses a **simplified field-addition hash** as a placeholder for a proper SNARK-friendly hash function (Pedersen or Poseidon). This means the hash constraint is not cryptographically binding — a real hash gadget is needed for production soundness. The circuit structure (3 public inputs: old_root, new_root, event_commitment) is correct and ready for a production hash upgrade.

### 3.2 No Formal Verification Beyond TLA+ Model Checking (High)

The protocol has a TLA+ specification (`formal-verification/OmniaConsensus.tla`, 123 lines) that verifies the `Agreement`, `NoEquivocation`, and `Validity` invariants for N=4 nodes, f=1 Byzantine, and 3 rounds. This is bounded model checking — it does not constitute a proof for arbitrary configurations.

There is no formal verification of the Rust implementation against the TLA+ spec. The property-based tests in `substrate/tests/property_tests.rs` provide some coverage, but they are not exhaustive.

**Planned mitigation:** Consider using tools like `hax` (Rust → F* extraction) or `Prusti` (Rust verification) for formal verification of critical invariants. This is a long-term goal, not a Sprint 3 deliverable.

### 3.3 Single Primary Developer (High)

The Omnia Protocol has been primarily developed by a single engineer. This increases the risk of:
- Blind spots — one person's assumptions go unchallenged
- Implicit mental models that are not documented
- Consistent coding patterns (both good and bad) that a second reviewer would question
- Bus factor of 1 — the project is vulnerable to the developer becoming unavailable

**Mitigation:** The external audit is a step toward independent review. Ongoing code review from additional contributors is planned for Sprint 4+.

### 3.4 Groth16 Trusted Setup (Medium)

Groth16 requires a circuit-specific trusted setup (phase 2). If the setup ceremony is compromised, a participant can generate false proofs. Unlike universal-setup systems (e.g., PLONK with Kate commitments), Groth16's setup must be repeated for every circuit change.

**Current status:** The trusted setup is assumed honest (see AUDIT_SCOPE.md §4). In production, a multi-party computation (MPC) ceremony will be required. No such ceremony has been conducted yet.

**Planned mitigation:** When the circuit is finalized (post-Sprint 4), a public MPC ceremony will be organized. Consider migrating to a universal-setup proving system (e.g., PLONK, Halo2) in a future sprint to eliminate the trusted setup requirement.

### 3.5 Slashing Persistence Default Is In-Memory (Medium)

The `SlashingEngine` now supports persistent storage via the `SlashingStore` trait with `SledSlashingStore` (Sprint 3). However, the default `new()` constructor still uses `InMemorySlashingStore`. If a production node forgets to call `with_store(SledSlashingStore::open(...))`, all slashing state is lost on restart. This means:
- A malicious validator can reset their slash points by restarting their node (if the node operator uses the default constructor)
- `persist_state()` logs a warning on failure but does not rollback — a persistence failure leaves the in-memory and on-disk states inconsistent
- There is still no on-chain record of slashing events for other nodes to verify

**Current mitigation:** The `with_store()` constructor is available and documented. The `SledSlashingStore` backend persists state to a sled embedded database. The persistence gap is now a deployment/configuration issue rather than a code issue.

### 3.6 No Binary Entrypoint (Medium) → ✅ Resolved (Sprint 3)

The `omnia-node` binary target has been added in Sprint 3. It provides:
- CLI with clap (configurable via args + env vars)
- HTTP health endpoint (`/health`) and Prometheus metrics (`/metrics`)
- REST API with Swagger UI for events, shards, governance, economics, and node endpoints
- Graceful shutdown on SIGINT/SIGTERM
- Full node lifecycle (substrate init, slashing init with sled fallback, shard router, economics)

---

## 4. Test Coverage Summary

### 4.1 Unit and Integration Tests

The protocol has 278+ tests across 5 crates (substrate, shards, economics, zk, binding). These cover:

| Crate | Test categories |
|---|---|
| `substrate` | Event creation/signing/verification, causal graph insertion/traversal, consensus finality, gossip simulation, slashing offense detection, vector clock merge, CRDT convergence, property-based tests |
| `shards` | Fee enforcement, replay protection, cross-shard routing, financial adversarial tests, identity hardening, layer 2 integration |
| `economics` | UBC lifecycle (mint/spend/reward), governance determinism, fixed-point arithmetic, quota management |
| `zk` | Circuit construction, Groth16 proof generation and verification, settlement layer abstraction |
| `binding` | Quantum commitment (Ed25519 + Dilithium hybrid), provenance chain construction, physical shard binding |

### 4.2 Property-Based Tests

The `substrate/tests/property_tests.rs` file contains property-based tests (using `proptest` or manual property definitions) that test:
- Causal graph invariants under random insertion sequences
- Consensus behavior under Byzantine conditions
- Vector clock merge associativity and commutativity
- Event hash determinism across serialization/deserialization

### 4.3 Adversarial Tests

- `shards/tests/financial_adversarial.rs` — Tests the financial shard against adversarial inputs (negative amounts, overflow attempts, unauthorized minting)
- `shards/tests/identity_hardening.rs` — Tests identity operations against forgery and replay attacks
- `shards/tests/replay_protection.rs` — Tests the nonce-based replay protection in the shard router

### 4.4 Coverage Limitations

- No code coverage measurement (no `cargo tarpaulin` or `cargo llvm-cov` integration)
- No mutation testing
- Integration tests do not cover the full gossip → consensus → shard pipeline end-to-end
- No chaos testing framework (planned for Sprint 3+)

---

## 5. Fuzzing

The protocol has 4 fuzz targets in the `fuzz/` directory, using `cargo-fuzz` / `libFuzzer`:

| Fuzz Target | File | What it fuzzes |
|---|---|---|
| `causal_graph_insert` | `fuzz/fuzz_targets/causal_graph_insert.rs` | Random event insertion into the causal graph — tests for panics, hash collisions, and graph invariant violations |
| `event_validate` | `fuzz/fuzz_targets/event_validate.rs` | Random byte sequences fed to `Event::from_bytes()` and `Event::validate()` — tests for deserialization panics and verification bypass |
| `shard_route` | `fuzz/fuzz_targets/shard_route.rs` | Random event payloads fed to `ShardRouter::route_event()` — tests for deserialization panics, fee enforcement bypass, and shard state corruption |
| `vector_clock_merge` | `fuzz/fuzz_targets/vector_clock_merge.rs` | Random vector clock states merged together — tests for partial order violations and merge non-commutativity |

### Fuzzing Limitations

- Fuzz targets are defined but there is no evidence of sustained fuzzing campaigns (no corpus directory, no coverage reports)
- No fuzzing of the ZK circuit (random witness generation could uncover constraint satisfaction bugs)
- No fuzzing of the `QuantumCommitment::verify()` method (malformed signatures, oversized keys)
- No fuzzing of the gossip protocol (malformed gossip messages, oversized event batches)

---

## 6. Dependency Audit

### 6.1 cargo-audit Configuration

The `cargo-audit` tool is configured for the workspace. The audit configuration (referenced in `.cargo/audit.toml` or similar) contains 5 ignored advisories, each documented with a justification for why the advisory does not affect the Omnia Protocol.

### 6.2 Key Dependencies

| Dependency | Version | Purpose | Notes |
|---|---|---|---|
| `ed25519-dalek` | Latest | Ed25519 signature verification | Well-audited; constant-time operations |
| `pqc-dilithium` | Latest | CRYSTALS-Dilithium PQC signatures | NIST PQC standard; no formal audit of Rust crate |
| `ark-bn254` | Latest | BN254 elliptic curve for ZK | Used by major protocols (Celo, Polygon zkEVM) |
| `ark-groth16` | Latest | Groth16 ZK proof system | Well-audited; reference implementation |
| `ark-r1cs-std` | Latest | R1CS constraint standard library | Part of arkworks ecosystem |
| `blake3` | Latest | Hashing (state roots, commitments) | Very fast; no known vulnerabilities |
| `bincode` | Latest | Serialization (events, payloads) | Not cryptographic; deserialization of untrusted data is a risk |
| `libp2p` | Latest | P2P networking | Large dependency surface; many sub-crates |
| `serde` | Latest | Serialization framework | No known issues |
| `thiserror` | Latest | Error derivation | No security implications |

### 6.3 Dependency Risks

- **`pqc-dilithium`**: This crate has not undergone a formal third-party security audit. It is a Rust port of the C reference implementation. Constant-time guarantees are not documented.
- **`bincode`**: Deserialization of untrusted data can be a vector for denial-of-service attacks (e.g., deeply nested structures causing stack overflow). The protocol uses `bincode` for event deserialization (`Event::from_bytes()`) and cross-shard message deserialization.
- **`libp2p`**: The libp2p dependency tree is large (20+ sub-crates). Any vulnerability in a sub-crate affects the protocol. Regular `cargo-audit` runs are essential.
- **5 ignored advisories**: Each ignored advisory in the audit configuration should be reviewed by the auditor to confirm the justification is valid.

---

## 7. Security Posture Summary

| Category | Status | Trend |
|---|---|---|
| Consensus safety | Partially verified (TLA+ bounded, property tests) | Improving |
| Cryptographic correctness | Real implementations (not stubs), minimal ZK circuit | Needs work (circuit soundness) |
| Economic security | Fee enforcement + slashing added, persistence available | Improving |
| Network security | Gossip bounds exist, no rate limiting | Needs work |
| Input validation | Hash + signature checks, nonce replay protection | Good |
| Authorization | No ACL for privileged operations | Needs work |
| Persistence | SlashingStore trait + SledSlashingStore added; default still in-memory | Sprint 3 (done) |
| Test coverage | 278+ tests, 4 fuzz targets, no coverage metrics | Adequate |
| Dependency health | cargo-audit configured, 5 ignored advisories | Monitoring |

---

## 8. What We Want Auditors to Focus On

Based on our self-assessment, we believe the following areas would benefit most from external scrutiny:

1. **ZK circuit soundness** — Is the single-constraint circuit a fundamental design flaw or an acceptable starting point? What constraints are minimally required for rollup soundness?
2. **Causal graph insertion invariants** — Are there edge cases where `CausalGraph::insert()` could violate the DAG invariant (e.g., orphan events, cycles, hash collisions)?
3. **Slashing engine correctness** — Are there scenarios where slash points can be avoided, reset, or exploited (beyond the known in-memory issue)?
4. **Fee enforcement bypass** — Can the fee/nonce/replay protection in `ShardRouter` be circumvented through crafted events or cross-shard messages?
5. **Hybrid PQC verification** — Is the `ClassicalOnly`/`Hybrid`/`PostQuantum` phase transition logic correct? Can a commitment that should fail in one phase be accepted in another?
6. **Consensus engine edge cases** — What happens at threshold boundaries (exactly 2/3 of nodes, exactly f Byzantine nodes)? Are there off-by-one errors?

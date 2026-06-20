# Omnia Protocol — Security Self-Assessment
> 🎯 Audience: Security Researchers
> 🔗 Context: Part of the audit documentation section
> 📅 Last Updated: 2026-05-20

**Version:** v4.0.0
**Date:** 2026-03-05
**Document version:** 2.0

---

## 1. Purpose

This document provides an honest and transparent assessment of the Omnia Protocol's current security posture as of v4.0.0. It catalogs known issues that have been fixed, remaining risks that have not yet been addressed, test coverage status, fuzzing efforts, and dependency audit results. The intent is to give auditors a complete picture of where the protocol stands — both strengths and weaknesses — so they can focus their efforts on the areas that need the most attention.

---

## 2. Known Issues and Mitigations Applied

The following issues were identified during earlier sprints and have been resolved. Each entry describes the original vulnerability, the mitigation applied, and the residual risk (if any).

### 2.1 f64 → Fixed-Point (Sprint 2)

**Original issue:** Consensus-critical calculations used IEEE 754 floating-point (`f64`) arithmetic. Floating-point operations are non-deterministic across platforms due to different FPU rounding modes, register widths (80-bit x87 vs. 64-bit SSE), and compiler optimizations. Non-determinism in consensus leads to divergence — different nodes computing different results for the same inputs, which violates the agreement invariant.

**Mitigation:** All consensus-related arithmetic was migrated to fixed-point integers (`u64` with explicit scaling factors). The `economics/src/fixed_point.rs` module (486 lines) implements a `FixedPoint<U>` type with deterministic multiplication, division, and square root operations. All fee calculations, governance voting power, and UBC token operations now use integer arithmetic exclusively.

**Residual risk:** The RF fingerprinting module (`binding/src/rf_fingerprint.rs`) still uses `f64` for Hamming distance similarity calculation. This is acceptable because RF fingerprinting is a stub module that does not participate in consensus — it is explicitly out of scope for production use and documented as such. The chaos test framework uses `f64` for `drop_rate` but this is test-only.

### 2.2 PQC verify() Returning True Unconditionally (Sprint 2)

**Original issue:** The `QuantumCommitment::verify()` method in `binding/src/quantum_commit.rs` contained a stub implementation that returned `true` regardless of the input. This meant that any commitment — even one with a forged or empty Dilithium signature — would pass verification in the `Hybrid` and `PostQuantum` phases. This completely negated the post-quantum security guarantees.

**Mitigation:** The `verify_dilithium()` method was replaced with a real verification call to `pqc_dilithium::verify()`. The method now:
1. Checks that the Dilithium public key is non-empty (returns `false` if empty)
2. Checks that the Dilithium signature is non-empty (returns `false` if empty)
3. Calls `pqc_dilithium::verify(&self.dilithium_sig, hash.as_bytes(), &public_key.dilithium)` and returns the result

**Residual risk:** No constant-time guarantee for the `pqc_dilithium::verify()` function. If the underlying crate leaks timing information, it could enable side-channel attacks. This is an upstream dependency concern, not a protocol implementation issue.

### 2.3 No Fee Enforcement (Sprint 2)

**Original issue:** The `ShardRouter` did not enforce fees before processing shard operations. Any event with a valid signature and nonce could trigger arbitrary shard operations without paying UBC tokens. This allowed spam attacks at zero cost.

**Mitigation:** A two-layer fee enforcement system was added:
1. **FeeSchedule** (`shards/src/fee_schedule.rs`, 187 lines): Maps each `ShardOp` variant to a fixed `u64` fee in UBC units. Standard fees range from 2 UBC (identity operations) to 15 UBC (cross-shard operations).
2. **QuotaSystem** (`economics/src/quota.rs`, 141 lines): Manages per-DID UBC balances with atomic `spend()` and `reward()` operations. Insufficient balance returns `EconomicsError`.

The `ShardRouter::route_event()` method now: (a) checks the nonce for replay protection, (b) looks up the fee for the operation, (c) deducts the fee from the caller's quota, and (d) routes the operation only if the fee deduction succeeds. Fee deduction happens before shard dispatch — if the operation fails, the fee is not refunded.

**Residual risk:** `ShardRouter::new_without_fees()` creates a router with `FeeSchedule::zero()`, bypassing all fee enforcement. This is intended for testing but could be accidentally used in production. The `Default` impl for `ShardRouter` also uses `new_without_fees()`.

### 2.4 No Slashing (Sprint 2)

**Original issue:** The protocol had no mechanism to penalize Byzantine validators. A validator could equivocate, go offline, or attest to invalid data with no economic consequences.

**Mitigation:** A `SlashingEngine` (`substrate/src/slashing.rs`, 1,079 lines) was implemented with:
- Three offense types: Equivocation (500 points), LivenessViolation (100 points), InvalidAttestation (300 points)
- Two thresholds: Slash (500 points — stake forfeited), Ejection (2000 points — removed from validator set)
- Equivocation detection via `check_equivocation()` — compares `creator + sequence + event_id`
- Liveness monitoring via `check_liveness()` — compares inactive rounds against a threshold
- Point accumulation uses `saturating_add` to prevent overflow
- Persistent storage via `SlashingStore` trait with `RedbSlashingStore` backend (configured automatically in `omnia-node`). Snapshot-and-rollback via `SlashingUndoManager` (FIND-011).

**Residual risk:** `persist_state()` logs a warning on failure but does not rollback the in-memory state. `record_offense()` returns a `SlashOutcome` but does not actually confiscate stake or emit a slashing event to the network. The `SlashingUndoManager` provides snapshot-and-rollback for governance appeals (FIND-011).

### 2.5 ZK Hash-Chain Stub (Sprint 2)

**Original issue:** The ZK proof system used a hash-chain stub (`RollupCircuitLegacy`) that computed a BLAKE3 hash chain. This is not a zero-knowledge proof — it provides no privacy, no soundness, and no succinctness.

**Mitigation:** A real R1CS circuit (`RollupCircuit`) was implemented using arkworks on the BN254 curve with Groth16 proving and verification. The `ExpandedRollupCircuit` adds Merkle path verification and per-event state transition constraints.

**Residual risk:** The `ExpandedRollupCircuit` now uses Poseidon hash (see §3.2 for current status). The legacy stub circuit is retained under `#[cfg(test)]`. The Poseidon implementation uses BLAKE3-derived round constants instead of the Filecoin/Neptune reference constants — auditors should verify this does not weaken the permutation.

### 2.6 No Binary Entrypoint (Sprint 3) → ✅ Resolved

**Original issue:** There was no way to run the protocol as a standalone node.

**Mitigation:** The `omnia-node` binary provides:
- CLI with clap (configurable via args + env vars with `OMNIA_` prefix)
- HTTP health endpoint (`/health`) and Prometheus metrics (`/metrics`)
- REST API with 9 endpoints under `/api/v1/` + Swagger UI at `/swagger-ui`
- Graceful shutdown on SIGINT/SIGTERM
- 6 CLI subcommands: `run`, `keygen`, `setup-contribute`, `setup-verify`, `snapshot`, `restore`
- Persistent slashing (redb) and nonce (redb) state
- TOML config file support via `--config`
- Structured logging with JSON output support (`RUST_LOG_FORMAT=json`)

**Residual risk:** The REST API has **no authentication, no rate limiting, and no authorization**. Any network client can perform any operation. The `keygen` subcommand writes unencrypted private keys.

---

## 3. Remaining Risks

These risks are known but not yet mitigated. They are listed in approximate order of severity.

### 3.1 REST API Security (Critical) → ✅ Resolved (FIND-001)

The `omnia-node` HTTP API (`node/src/api/`) previously exposed 9 endpoints with no security controls. This has been addressed in Phase 0:

- **JWT authentication** — `node/src/api/auth.rs` (645 lines) validates JWT tokens via `jsonwebtoken` crate. Configured via `OMNIA_JWT_SECRET` env var.
- **AuthorizedCallers ACL** — Only registered caller IDs can access the API. Configured via `OMNIA_AUTHORIZED_CALLERS` env var.
- **Rate limiting** — Per-IP token-bucket rate limiter. Configured via `OMNIA_RATE_LIMIT_RPS` env var.
- **CORS** — Enforced via `tower-http` CORS middleware.
- **Privileged operations** — Mint UBC and AdvanceEpoch require admin JWT (`OMNIA_AUTHORIZED_ADMINS`).

**Residual risk:** No TLS termination on the node itself — a reverse proxy is still needed for production. The JWT secret must be kept secure. No token revocation mechanism.

### 3.2 ZK Circuit Is Minimal (Critical) → 🟢 Addressed

The original `RollupCircuit` enforced only `new_state_root == expected_new_state_root`. The `ExpandedRollupCircuit` adds Merkle path inclusion verification, per-event state transition constraints, and old state root binding.

**Previously remaining gap:** The `ExpandedRollupCircuit` used a **simplified field-addition hash** as a placeholder for a proper SNARK-friendly hash function. This meant the hash constraint was not cryptographically binding.

**Current status:** The `ExpandedRollupCircuit` now uses **Poseidon hash** (implemented in `omnia-adapters/src/poseidon.rs`) with:
- Cauchy MDS matrix construction (`generate_mds_matrix()`) for the linear layer
- BLAKE3-derived round constants (deterministically generated from a seed, not the Filecoin/Neptune reference constants)
- Full R1CS gadget (`poseidon_permutation_gadget()`) for on-circuit verification
- Parameters: BN254 field, t=3 (arity), R_F=8 (full rounds), R_P=57 (partial rounds)
- Off-circuit verification via `poseidon_hash()` for regular usage

**Deviation from reference:** The round constants are derived via BLAKE3 rather than using the Filecoin/Neptune reference constants. The code includes warning comments documenting this deviation. This is a deliberate choice for simplicity, but auditors should verify that the BLAKE3 derivation does not introduce weaknesses in the round function.

### 3.3 Unencrypted Private Key Storage (High) → ✅ Resolved (FIND-010)

The `keygen` CLI subcommand now supports encrypted key output:
- With `--passphrase` (or `OMNIA_KEYGEN_PASSPHRASE` env var), the private key is encrypted with AES-256-GCM using a key derived from the passphrase via BLAKE3 domain-separated key derivation, and saved as `validator_key.enc`.
- Without `--passphrase`, keys are written unencrypted as `validator_key.bin` with a prominent warning.
- The `EncryptedKeyStore` module (`substrate/src/keystore.rs`, 856 lines) provides encrypted key storage with AES-256-GCM + HKDF-SHA256.
- The `load_encrypted_key()` function in `node/src/main.rs` decrypts keys for runtime use.

**Residual risk:** Without `--passphrase`, unencrypted key output is still the default behavior. No HSM integration. No automatic file permission enforcement.

### 3.4 No Formal Verification Beyond TLA+ Model Checking (High)

The protocol has a TLA+ specification (`formal-verification/OmniaConsensus.tla`, 191 lines) that verifies the `Agreement`, `NoEquivocation`, `Validity`, `Liveness`, and `TypeOK` invariants for N=4 nodes, f=1 Byzantine, and MaxSeq=1. This is bounded model checking — it does not constitute a proof for arbitrary configurations.

There is no formal verification of the Rust implementation against the TLA+ spec. The property-based tests in `substrate/tests/property_tests.rs` provide some coverage, but they are not exhaustive. The chaos testing framework (`omnia-chaos-tests`) provides additional executable validation but is not formal verification.

### 3.5 Single Primary Developer (High)

The Omnia Protocol has been primarily developed by a single engineer. This increases the risk of blind spots, implicit mental models, consistent coding patterns that a second reviewer would question, and a bus factor of 1.

### 3.6 Groth16 Trusted Setup (Medium)

Groth16 requires a circuit-specific trusted setup (phase 2). If the setup ceremony is compromised, a participant can generate false proofs.

**Current status:** The `omnia-node` binary includes `setup-contribute` and `setup-verify` subcommands for managing the Powers of Tau ceremony. These support local simulation only — no multi-party network coordination.

### 3.7 Slashing Persistence Failure Handling (Medium) → 🟢 Partially Addressed (FIND-011)

`RedbSlashingStore::persist_state()` logs a warning on failure but does not rollback the in-memory state. A persistence failure leaves the in-memory and on-disk states inconsistent. However, the `SlashingUndoManager` (`substrate/src/slashing_undo.rs`, 508 lines) now provides snapshot-and-rollback capability for governance appeals (FIND-011). Additionally, there is no on-chain record of slashing events for other nodes to verify.

### 3.8 Redb Database Reliability (Low)

Both `RedbSlashingStore` and `RedbNonceStore` use redb, which is a production-quality embedded database with ACID transactions, crash-safe durability, and active maintenance. The previous sled 0.34 alpha-quality dependency has been replaced.

**Risks:** Persistence failure leaves in-memory and on-disk states inconsistent. No on-chain record of slashing events for cross-node verification. No automated backup or database repair tooling.

### 3.9 TOML Config node_id Type Mismatch (Low) → ✅ Resolved (FIND-013)

`NodeConfigFile::node_id` was `Option<u16>` but `NodeConfig::node_id` was `u64`. This has been fixed: `NodeConfigFile::node_id` is now `Option<u64>`, matching the CLI and `NodeConfig` types. TOML config files can now specify node IDs up to `u64::MAX`.

---

## 4. Test Coverage Summary

### 4.1 Unit and Integration Tests

The protocol has 295+ tests across 7 crates (substrate, shards, economics, zk, binding, node, chaos-tests). These cover:

| Crate | Test categories |
|---|---|
| `substrate` | Event creation/signing/verification, causal graph insertion/traversal, consensus finality, gossip simulation, slashing offense detection, vector clock merge, CRDT convergence, property-based tests, snapshot serialization |
| `shards` | Fee enforcement, replay protection, cross-shard routing, financial adversarial tests, identity hardening, layer 2 integration |
| `economics` | UBC lifecycle (mint/spend/reward), governance determinism, fixed-point arithmetic, quota management |
| `zk` | Circuit construction, Groth16 proof generation and verification, settlement layer abstraction |
| `binding` | Quantum commitment (Ed25519 + Dilithium hybrid), provenance chain construction, physical shard binding |
| `node` | CLI config validation (zero node_id, zero http_port, invalid log_level), TOML config parsing, slashing/nonce dir defaults |
| `chaos-tests` | Network partition safety/liveness, node crash recovery, message drop rates, equivocation detection |

### 4.2 Property-Based Tests

The `substrate/tests/property_tests.rs` file contains property-based tests that test:
- Causal graph invariants under random insertion sequences
- Consensus behavior under Byzantine conditions
- Vector clock merge associativity and commutativity
- Event hash determinism across serialization/deserialization

### 4.3 Adversarial Tests

- `shards/tests/financial_adversarial.rs` — Tests the financial shard against adversarial inputs (negative amounts, overflow attempts, unauthorized minting)
- `shards/tests/identity_hardening.rs` — Tests identity operations against forgery and replay attacks
- `shards/tests/replay_protection.rs` — Tests the nonce-based replay protection in the shard router

### 4.4 Chaos Tests

The `omnia-chaos-tests` crate provides a comprehensive simulation framework (`ChaosNetwork`, 982 lines):

- **Network partitions**: `partition()` / `heal()` — isolates and reconnects node groups
- **Node crashes**: `crash_node()` / `restart_node()` — simulates process failure and recovery
- **Message drop rates**: `set_drop_rate()` — simulates unreliable links (0.0 to 1.0)
- **Safety verification**: `check_safety()` — verifies no conflicting commits across nodes
- **Liveness verification**: `check_liveness()` — verifies at least some events are committed
- **Slashing detection**: `is_node_slashed()` — checks slash status from an observer's perspective
- **Byzantine equivocation**: Events can be injected with duplicate `(creator, sequence)` pairs

### 4.5 Coverage Limitations

- No code coverage measurement (no `cargo tarpaulin` or `cargo llvm-cov` integration)
- No mutation testing
- Integration tests do not cover the full gossip → consensus → shard pipeline end-to-end
- No end-to-end tests for the REST API
- Chaos tests use API-level calls, not real network I/O

---

## 5. Fuzzing

The protocol has 7 fuzz targets, managed via `scripts/fuzz.sh`:

| Fuzz Target | What it fuzzes |
|---|---|
| `fuzz_event_deserialization` | Random bytes fed to `Event::from_bytes()` — tests for deserialization panics |
| `fuzz_gossip_message` | Random gossip message structures — tests for malformed gossip data |
| `fuzz_zk_proof_deserialization` | Random ZK proof data — tests for proof deserialization robustness |
| `fuzz_consensus_state_transition` | Random consensus state transitions — tests for state machine panics |
| `fuzz_vector_clock_merge` | Random vector clock merge operations — tests for partial order violations |
| `fuzz_rate_limiter` | Random rate limiter inputs — tests for rate limiter edge cases |
| `fuzz_snapshot_deserialization` | Random snapshot data — tests for snapshot deserialization robustness |

Corpus seeds can be generated via `scripts/generate-fuzz-seeds.sh`.

### Fuzzing Limitations

- Fuzz targets are defined but there is no evidence of sustained fuzzing campaigns (no coverage reports)
- No fuzzing of the REST API endpoints
- No fuzzing of the `QuantumCommitment::verify()` method
- No fuzzing of the TOML config parsing

---

## 6. Dependency Audit

### 6.1 Key Dependencies

| Dependency | Version | Purpose | Notes |
|---|---|---|---|
| `ed25519-dalek` | Latest | Ed25519 signature verification | Well-audited; constant-time operations |
| `pqc-dilithium` | Latest | CRYSTALS-Dilithium PQC signatures | NIST PQC standard; no formal audit of Rust crate |
| `ark-bn254` | Latest | BN254 elliptic curve for ZK | Used by major protocols (Celo, Polygon zkEVM) |
| `ark-groth16` | Latest | Groth16 ZK proof system | Well-audited; reference implementation |
| `ark-r1cs-std` | Latest | R1CS constraint standard library | Part of arkworks ecosystem |
| `blake3` | Latest | Hashing (state roots, commitments, node IDs) | Very fast; no known vulnerabilities |
| `postcard` | Latest | Serialization (events, payloads) | Not cryptographic; deterministic `no_std`-compatible format |
| `libp2p` | Latest | P2P networking | Large dependency surface; many sub-crates |
| `serde` | Latest | Serialization framework | No known issues |
| `thiserror` | Latest | Error derivation | No security implications |
| `axum` | "0.7" | HTTP framework | Well-maintained; no known security issues |
| `clap` | "4" | CLI argument parsing | Well-maintained; no known security issues |
| `redb` | "2" | Embedded database | Production-quality; ACID transactions, crash-safe, pure Rust |
| `utoipa` | "5" | OpenAPI spec generation | No security implications |
| `utoipa-swagger-ui` | "8" | Swagger UI | No security implications; serves static assets |
| `prometheus` | "0.13" | Metrics exposition | Standard monitoring library |
| `tokio` | "1" | Async runtime | Well-maintained; industry standard |

### 6.2 Dependency Risks

- **`redb` 2.x**: Production-quality embedded database with ACID transactions, crash-safe durability, and active maintenance. Used for `RedbSlashingStore` and `RedbNonceStore`. Replaces the previous sled 0.34 alpha-quality dependency.
- **`pqc-dilithium`**: This crate has not undergone a formal third-party security audit. It is a Rust port of the C reference implementation. Constant-time guarantees are not documented.
- **`postcard`**: Deserialization of untrusted data can be a vector for denial-of-service attacks (e.g., deeply nested structures causing stack overflow). The protocol uses `postcard` for event deserialization and cross-shard message deserialization. bincode 1.x is retained only for v0 backward compatibility.
- **`libp2p`**: The libp2p dependency tree is large (20+ sub-crates). Any vulnerability in a sub-crate affects the protocol.
- **5 ignored advisories**: Each ignored advisory in the audit configuration should be reviewed by the auditor to confirm the justification is valid.

---

## 7. Security Posture Summary

| Category | Status | Trend |
|---|---|---|
| Consensus safety | Partially verified (TLA+ bounded, property tests, chaos tests) | Improving |
| Cryptographic correctness | Real implementations (not stubs), Poseidon hash in ZK circuit | Improving |
| Economic security | Fee enforcement + slashing + persistence available | Improving |
| Network security | Gossip bounds exist, rate limiting on API (FIND-001) | Improving |
| **API security** | **JWT auth + ACL + rate limiting + CORS (FIND-001)** | **Resolved** |
| Input validation | Hash + signature checks, nonce replay protection | Good |
| Authorization | ACL for privileged operations via AuthorizedCallers + admin JWT | Resolved (FIND-001) |
| Persistence | RedbSlashingStore + RedbNonceStore; redb is production-quality | ✅ Resolved |
| Key management | EncryptedKeyStore (AES-256-GCM); keygen supports --passphrase | ✅ Resolved (FIND-010) |
| Test coverage | 295+ tests, 7 fuzz targets, chaos test framework | Adequate |
| Dependency health | cargo-audit configured, redb is production-quality | Monitoring |

---

## 8. What We Want Auditors to Focus On

Based on our self-assessment, we believe the following areas would benefit most from external scrutiny:

1. **REST API security implementation** — JWT auth, AuthorizedCallers ACL, rate limiting, and CORS are now implemented (FIND-001). Auditors should review the `auth.rs` implementation for correctness, edge cases, and configuration security. Is the JWT secret handling robust? Are there timing attack vectors in the authorization check?
2. **ZK circuit soundness** — The `ExpandedRollupCircuit` now uses Poseidon hash with BLAKE3-derived round constants. Is this BLAKE3 derivation safe, or should the Filecoin/Neptune reference constants be used? Are there other constraints minimally required for rollup soundness?
3. **Causal graph insertion invariants** — Are there edge cases where `CausalGraph::insert()` could violate the DAG invariant (e.g., orphan events, cycles, hash collisions)?
4. **Slashing engine correctness** — Are there scenarios where slash points can be avoided, reset, or exploited (beyond the known persistence failure handling)?
5. **Fee enforcement bypass** — Can the fee/nonce/replay protection in `ShardRouter` be circumvented through crafted events or cross-shard messages?
6. **Hybrid PQC verification** — Is the `ClassicalOnly`/`Hybrid`/`PostQuantum` phase transition logic correct? Can a commitment that should fail in one phase be accepted in another?
7. **Consensus engine edge cases** — What happens at threshold boundaries (exactly 2/3 of nodes, exactly f Byzantine nodes)? Are there off-by-one errors?
8. **Redb persistence reliability** — What happens when redb databases are corrupted, disk is full, or power fails during a write? What is the blast radius? redb uses ACID transactions and a WAL, which should provide crash safety.
9. **Key management** — The `keygen` subcommand now supports `--passphrase` for AES-256-GCM encryption, and `EncryptedKeyStore` provides encrypted storage. Is the key derivation (BLAKE3 domain-separated) robust? Should HSM integration be required for production?
10. **Node ID derivation** — The chaos tests use `blake3(pubkey)` for node IDs, matching `Event::sign_with_keypair()`. But `NodeConfig::node_id_bytes()` uses `node_id.to_le_bytes()`. Are these consistent?

---
🔙 **Back**: [Audit](./) | 🔄 **Related**: [Attack Surface](./ATTACK_SURFACE.md)
🚀 **Next**: [Self Assessment](./SELF_ASSESSMENT.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

# Omnia Protocol — Auditor's Guide

**Commit:** `SPRINT_3_COMMIT`
**Date:** 2026-03-05
**Document version:** 1.0

---

## 1. Welcome

This document provides everything an external security auditor needs to begin reviewing the Omnia Protocol codebase: build instructions, test commands, formal verification setup, repository structure, key assumptions, and the project's coding rules. Read this guide first, then proceed to `AUDIT_SCOPE.md`, `ATTACK_SURFACE.md`, and `SELF_ASSESSMENT.md` for the detailed audit scope and known issues.

---

## 2. How to Build

### 2.1 Prerequisites

- **Rust toolchain**: stable (latest). Install via `rustup`:
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  rustup default stable
  ```
- **Java 11+** (for TLA+ model checker, optional)
- **cargo-fuzz** (for fuzzing, optional): `cargo install cargo-fuzz`

### 2.2 Build Commands

Build the entire workspace (all 5 crates + fuzz targets):

```bash
cargo build --workspace
```

Build the node binary (once `omnia-node` is added in Sprint 3):

```bash
cargo build --bin omnia-node
```

Build in release mode (with optimizations):

```bash
cargo build --workspace --release
```

### 2.3 Build Verification

After building, verify there are no compiler warnings:

```bash
cargo check --workspace
```

Run clippy with strict warnings (this is how the CI enforces code quality):

```bash
cargo clippy --workspace -- -D warnings
```

If clippy produces any warnings, the CI will fail. The codebase should be clippy-clean at `SPRINT_3_COMMIT`.

---

## 3. How to Test

### 3.1 Run All Tests

```bash
cargo test --workspace
```

This runs 278+ tests across all 5 crates, including unit tests, integration tests, property-based tests, and adversarial tests.

### 3.2 Run Tests for a Specific Crate

```bash
# Substrate (consensus, graph, gossip, slashing)
cargo test -p omnia-substrate

# Shards (routing, fees, domain-specific shard logic)
cargo test -p omnia-shards

# Economics (UBC, quota, governance, fixed-point)
cargo test -p omnia-economics

# ZK (circuit, prover, proof bundle, settlement)
cargo test -p omnia-zk

# Binding (quantum commitments, provenance, physical shard)
cargo test -p omnia-binding
```

### 3.3 Run Specific Test Categories

```bash
# Integration tests only
cargo test --workspace --test '*'

# Property-based tests (substrate only)
cargo test -p omnia-substrate --test property_tests

# Slashing-specific tests
cargo test -p omnia-substrate --test slashing

# Fee enforcement tests
cargo test -p omnia-shards --test fee_enforcement

# Adversarial financial tests
cargo test -p omnia-shards --test financial_adversarial

# Replay protection tests
cargo test -p omnia-shards --test replay_protection
```

### 3.4 Run Clippy (Static Analysis)

```bash
cargo clippy --workspace -- -D warnings
```

All code must pass clippy with zero warnings. The `-D warnings` flag treats all warnings as errors.

### 3.5 Run Fuzz Targets

Fuzz targets are located in `fuzz/fuzz_targets/`. To run them:

```bash
# Install cargo-fuzz if not already installed
cargo install cargo-fuzz

# Run a specific fuzz target (e.g., causal_graph_insert)
cargo fuzz run causal_graph_insert

# Run with a time limit
cargo fuzz run causal_graph_insert -- -max_total_time=60

# Run all fuzz targets
cargo fuzz run causal_graph_insert &
cargo fuzz run event_validate &
cargo fuzz run shard_route &
cargo fuzz run vector_clock_merge &
wait
```

Available fuzz targets:
- `causal_graph_insert` — Fuzzes `CausalGraph::insert()` with random events
- `event_validate` — Fuzzes `Event::from_bytes()` + `Event::validate()` with random bytes
- `shard_route` — Fuzzes `ShardRouter::route_event()` with random event payloads
- `vector_clock_merge` — Fuzzes `VectorClock::merge()` with random clock states

---

## 4. How to Run the TLA+ Model

The TLA+ specification is in `formal-verification/OmniaConsensus.tla` (123 lines). It models the consensus protocol with 4 nodes, 1 Byzantine node, and 3 rounds, verifying the `Agreement`, `NoEquivocation`, and `Validity` invariants.

### 4.1 Option A: Command Line

```bash
# Download TLA+ tools (requires Java 11+)
cd formal-verification
wget https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar

# Run the model checker
java -jar tla2tools.jar OmniaConsensus.tla -config OmniaConsensus.cfg -workers auto
```

### 4.2 Option B: TLA+ Toolbox (GUI)

1. Download from [https://github.com/tlaplus/tlaplus/releases](https://github.com/tlaplus/tlaplus/releases)
2. Launch the Toolbox IDE
3. File → Open Spec → Add new spec → select `OmniaConsensus.tla`
4. Create a new model with the configuration from `OmniaConsensus.cfg`
5. Click **Run**

### 4.3 Option C: VS Code Extension

1. Install the **TLA+** extension by Aly Badr from the VS Code marketplace
2. Open `formal-verification/` in VS Code
3. Right-click `OmniaConsensus.tla` → **Check model with TLC**

### 4.4 Model Configuration

The model is configured in `OmniaConsensus.cfg`:

```
SPECIFICATION Spec
INVARIANTS Agreement NoEquivocation TypeOK
CONSTANTS Nodes = {n1, n2, n3, n4}
          MaxByzantine = 1
          MaxRounds = 3
```

### 4.5 Expected Results

All three invariants should hold:
- **TypeOK** — Well-typedness invariant
- **Agreement** — All honest nodes that commit an event agree on its hash
- **NoEquivocation** — Equivocation is confined to Byzantine creators

If any invariant is violated, TLC produces a counterexample trace showing the step-by-step execution leading to the violation.

---

## 5. How to Run Chaos Tests

> **Note:** The chaos test suite (`omnia-chaos-tests`) is planned for Sprint 3 and may not be available at `SPRINT_3_COMMIT`. When available:

```bash
cargo test -p omnia-chaos-tests
```

Chaos tests will cover:
- Network partition scenarios (nodes temporarily disconnected)
- Byzantine validator behavior (equivocation, selective message dropping)
- Crash-recovery cycles (node restarts mid-consensus)
- Resource exhaustion (memory pressure, CPU throttling)

---

## 6. Key Assumptions for Auditors

1. **No `unwrap()` in production code.** The project follows a strict rule: `unwrap()` is only acceptable in test code. Any `unwrap()` in non-`#[cfg(test)]` code is a finding. Use `expect()` with a descriptive message only when the failure is truly impossible (e.g., `Vec::push` on a non-full vector).

2. **No `f64` / `f32` in consensus.** All consensus-critical calculations use integer arithmetic (`u64`, `i64`, or the `FixedPoint<U>` type). Any floating-point usage in the substrate, economics, or shards crates (outside of test code) is a finding. The RF fingerprinting module (`binding/src/rf_fingerprint.rs`) uses `f64` but is explicitly a stub and out of scope.

3. **All public functions have rustdoc.** Every `pub fn`, `pub struct`, `pub enum`, and `pub trait` method must have a `///` doc comment. Missing rustdoc on public items is a code quality finding.

4. **The codebase has a binary entrypoint.** `omnia-node` (added Sprint 3) provides a CLI, HTTP health/metrics, REST API with Swagger UI, and graceful shutdown. The core protocol remains a set of composable libraries.

5. **Slashing state supports persistence.** Sprint 3 added `SledSlashingStore` for sled-backed persistence. The default `new()` constructor still uses `InMemorySlashingStore` — production nodes should use `with_store(SledSlashingStore::open(...))`.

6. **The ZK circuit has been expanded.** Sprint 3 added `ExpandedRollupCircuit` with Merkle path verification and per-event state transition constraints. However, it uses a simplified field-addition hash as a placeholder — a production SNARK-friendly hash (Pedersen/Poseidon) is still needed. See `SELF_ASSESSMENT.md` §3.1.

7. **The RF fingerprinting module is a stub.** It uses Hamming distance (not real RF capture) and `f64` arithmetic. It is explicitly out of scope for the audit.

8. **The `pqc_dilithium` crate has not been formally audited.** It is a Rust port of the NIST C reference implementation. Constant-time guarantees are not documented for the Rust version.

---

## 7. Repository Structure

```
omnia-protocol/
├── Cargo.toml                    # Workspace root (5 members)
├── Cargo.lock                    # Locked dependency versions
├── SECURITY.md                   # Security policy and reporting
├── substrate/                    # Core consensus layer
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root, re-exports (493 lines)
│       ├── causal_graph.rs       # DAG-based event storage (1,233 lines)
│       ├── consensus.rs          # BFT consensus engine (777 lines)
│       ├── gossip.rs             # Epidemic gossip protocol (896 lines)
│       ├── slashing.rs           # Byzantine fault detection + penalties (1,079 lines)
│       ├── event.rs              # Event model, signing, verification (757 lines)
│       ├── vector_clock.rs       # Vector clock for causal ordering (569 lines)
│       ├── network.rs            # libp2p network integration (265 lines)
│       ├── crypto.rs             # Ed25519 key utilities (39 lines)
│       └── crdt/                 # CRDT primitives
│           ├── mod.rs            # CRDT trait definitions (182 lines)
│           ├── g_counter.rs      # Grow-only counter (321 lines)
│           ├── lww_register.rs   # Last-writer-wins register (443 lines)
│           └── or_set.rs         # Observed-remove set (427 lines)
├── shards/                       # Shard layer (domain-specific state machines)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root (522 lines)
│       ├── router.rs             # ShardRouter — dispatch + fee enforcement (206 lines)
│       ├── fee_schedule.rs       # Per-operation fee table (187 lines)
│       ├── shard.rs              # Shard trait definition (129 lines)
│       ├── payload.rs            # ShardPayload + ShardOp enums (66 lines)
│       ├── cross_shard.rs        # Cross-shard message passing (57 lines)
│       ├── economics_shard.rs    # Economics shard adapter (75 lines)
│       ├── financial/            # Financial domain (balances, transfers, mint/burn)
│       ├── identity/             # Identity domain (DIDs, recovery, biometrics, agents)
│       ├── physical/             # Physical domain (asset anchoring, ownership)
│       ├── biological/           # Biological domain (consent, ZK queries)
│       └── computational/        # Computational domain (task submission, proofs)
├── economics/                    # Economic model
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root (38 lines)
│       ├── fixed_point.rs        # Deterministic fixed-point arithmetic (486 lines)
│       ├── governance.rs         # Quadratic voting with decay (378 lines)
│       ├── quota.rs              # UBC quota tracking + epoch management (141 lines)
│       ├── ubc.rs                # UBC token model (84 lines)
│       ├── useful_work.rs        # Useful work computation + rewards (113 lines)
│       ├── economics_shard.rs    # Economics shard implementation (203 lines)
│       └── error.rs              # Error types (47 lines)
├── zk/                           # Zero-knowledge proof system
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root (62 lines)
│       ├── circuit.rs            # R1CS rollup circuit + legacy stub (242 lines)
│       ├── prover.rs             # Groth16 proof generation (174 lines)
│       ├── proof.rs              # Proof verification (119 lines)
│       ├── proof_bundle.rs       # Proof + state root bundle (304 lines)
│       ├── operator.rs           # ZK batch operator (228 lines)
│       └── settlement/           # L1 settlement adapters
│           ├── mod.rs            # SettlementLayer trait (76 lines)
│           ├── ethereum.rs       # Ethereum L1 adapter (124 lines)
│           ├── solana.rs         # Solana L1 adapter (70 lines)
│           ├── celestia.rs       # Celestia DA adapter (70 lines)
│           └── bitcoin.rs        # Bitcoin L1 adapter (75 lines)
├── binding/                      # Physical-digital binding layer
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root (69 lines)
│       ├── quantum_commit.rs     # Ed25519 + Dilithium hybrid commitments (478 lines)
│       ├── provenance.rs         # Provenance chain tracking (482 lines)
│       ├── physical_shard.rs     # Physical shard binding (462 lines)
│       ├── anchor.rs             # Asset anchoring (251 lines)
│       └── rf_fingerprint.rs     # RF fingerprinting STUB (174 lines)
├── fuzz/                         # Fuzz targets
│   ├── Cargo.toml
│   └── fuzz_targets/
│       ├── causal_graph_insert.rs
│       ├── event_validate.rs
│       ├── shard_route.rs
│       └── vector_clock_merge.rs
├── formal-verification/          # TLA+ model
│   ├── OmniaConsensus.tla        # TLA+ specification (123 lines)
│   ├── OmniaConsensus.cfg        # TLC model checker config
│   └── README.md                 # Verification instructions
├── docs/                         # Documentation
│   ├── audit/                    # Audit preparation documents (THIS DIRECTORY)
│   │   ├── AUDIT_README.md       # This file
│   │   ├── AUDIT_SCOPE.md        # Audit scope and boundaries
│   │   ├── ATTACK_SURFACE.md     # Attack surface map
│   │   └── SELF_ASSESSMENT.md    # Security self-assessment
│   └── security/
│       └── threat-model.md       # STRIDE threat model
├── docker/                       # Docker deployment (out of scope)
└── diagrams/                     # Architecture diagrams (out of scope)
```

### Crate Dependency Graph

```
omnia-substrate  ←  omnia-shards  ←  (test binaries)
       ↑                 ↑
       |                 |
omnia-economics  ←──────┘
       ↑
omnia-zk  ←  (depends on omnia-substrate for Event types)
       ↑
omnia-binding  ←  (depends on omnia-substrate for crypto + VectorClock)
```

Key dependency relationships:
- `omnia-shards` depends on `omnia-substrate` (Event, VectorClock, EventProcessor trait) and `omnia-economics` (QuotaSystem)
- `omnia-zk` depends on `omnia-substrate` (Event type for circuit)
- `omnia-binding` depends on `omnia-substrate` (NodeKeypair, VectorClock, crypto)
- `omnia-economics` is standalone (no dependency on other Omnia crates)

---

## 8. Important Project Rules

This project follows strict rules that auditors should be aware of. Violations of these rules are valid findings.

### 8.1 No `unwrap()` in Production

`unwrap()` is forbidden in all non-test code. Use:
- `?` operator for error propagation
- `.ok_or(Error::...)` for converting `Option` to `Result`
- `.expect("descriptive message")` ONLY when failure is truly impossible (e.g., serializing a `String` that cannot fail)

Search for violations:
```bash
# Find unwrap() calls in production code (excluding tests)
rg "\.unwrap\(\)" --type rust | rg -v "#\[test\]" | rg -v "mod tests"
```

### 8.2 No `f64` / `f32` in Consensus

Floating-point types are forbidden in the substrate, shards, and economics crates (excluding test code). All arithmetic must use integer types (`u64`, `i64`, `usize`) or the `FixedPoint<U>` type.

Search for violations:
```bash
rg "f64|f32" --type rust substrate/ shards/ economics/
```

The only acceptable exception is `binding/src/rf_fingerprint.rs`, which is a stub module.

### 8.3 All Public Functions Have Rustdoc

Every `pub fn`, `pub struct`, `pub enum`, and `pub trait` must have `///` doc comments. Missing rustdoc is a code quality finding.

Search for violations:
```bash
# This requires manual review, but missing docs will trigger rustdoc warnings
cargo doc --workspace --no-deps 2>&1 | rg "missing documentation"
```

### 8.4 Errors Are Typed, Not Strings

Error types use `thiserror` for structured error handling. Functions return `Result<T, SpecificError>`, not `Result<T, String>`. The only exception is `ShardError::ValidationFailed(String)` and `ShardError::UnknownShard(String)`, which carry dynamic messages.

### 8.5 All State Mutations Go Through Defined Interfaces

Shard state can only be mutated through `Shard::process_event()`. The `validate()` and `state_snapshot()` methods take `&self` and must not mutate state. Interior mutability (`Cell`, `RefCell`, `AtomicU64`) in shard state is a finding unless explicitly justified.

---

## 9. Quick Start Checklist

For auditors who want to get started quickly:

- [ ] Clone the repository and check out `SPRINT_3_COMMIT`
- [ ] `cargo build --workspace` — confirm it compiles without errors
- [ ] `cargo test --workspace` — confirm all 278+ tests pass
- [ ] `cargo clippy --workspace -- -D warnings` — confirm zero warnings
- [ ] Read `AUDIT_SCOPE.md` to understand the audit boundaries
- [ ] Read `ATTACK_SURFACE.md` to understand the attack vectors
- [ ] Read `SELF_ASSESSMENT.md` to understand known issues and mitigations
- [ ] Review `docs/security/threat-model.md` for the STRIDE analysis
- [ ] Run the TLA+ model checker to verify consensus invariants
- [ ] Begin reviewing in-scope components, starting with the highest-severity attack surfaces

---

## 10. Contact

For questions about the codebase, architecture, or audit scope, contact the Omnia Protocol development team at security@omnia-protocol.org or via the project's internal communication channels.

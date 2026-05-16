# Omnia Protocol — Auditor's Guide

**Version:** v4.0.0
**Date:** 2026-03-05
**Document version:** 2.0

---

## 1. Welcome

This document provides everything an external security auditor needs to begin reviewing the Omnia Protocol codebase: build instructions, test commands, formal verification setup, repository structure, key assumptions, and the project's coding rules. Read this guide first, then proceed to `AUDIT_SCOPE.md`, `ATTACK_SURFACE.md`, and `SELF_ASSESSMENT.md` for the detailed audit scope and known issues.

---

## 2. How to Build

### 2.1 Prerequisites

- **Rust toolchain**: stable 1.85+ (for Docker builds) or latest stable. Install via `rustup`:
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  rustup default stable
  ```
- **Java 11+** (for TLA+ model checker, optional)
- **cargo-fuzz** (for fuzzing, optional): `cargo install cargo-fuzz`
- **Docker** (for deployment and monitoring stack, optional)
- **protobuf-compiler** (for building with libp2p features)

### 2.2 Build Commands

Build the entire workspace (all 7 crates):

```bash
cargo build --workspace
```

Build the node binary:

```bash
cargo build --bin omnia-node
```

Build in release mode (with optimizations):

```bash
cargo build --workspace --release
```

Build a reproducible binary:

```bash
./scripts/reproducible-build.sh
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

If clippy produces any warnings, the CI will fail. The codebase should be clippy-clean.

---

## 3. How to Test

### 3.1 Run All Tests

```bash
cargo test --workspace
```

This runs tests across all 7 crates, including unit tests, integration tests, property-based tests, adversarial tests, and chaos tests.

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

# Node (CLI, config, HTTP server, API handlers)
cargo test -p omnia-node

# Chaos tests (network partitions, crashes, Byzantine behavior)
cargo test -p omnia-chaos-tests
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

# Node config validation tests
cargo test -p omnia-node
```

### 3.4 Run Clippy (Static Analysis)

```bash
cargo clippy --workspace -- -D warnings
```

All code must pass clippy with zero warnings. The `-D warnings` flag treats all warnings as errors.

### 3.5 Run Fuzz Targets

Fuzz targets are located in `fuzz/fuzz_targets/`. The `scripts/fuzz.sh` script runs all fuzz targets:

```bash
# Run all fuzz targets for 60 seconds each (default)
./scripts/fuzz.sh

# Run for 5 minutes each
FUZZ_TIME=300 ./scripts/fuzz.sh
```

**Available fuzz targets** (7 total, defined in `scripts/fuzz.sh`):

| Target | What it fuzzes |
|---|---|
| `fuzz_event_deserialization` | Random bytes fed to `Event::from_bytes()` |
| `fuzz_gossip_message` | Random gossip message structures |
| `fuzz_zk_proof_deserialization` | Random ZK proof data |
| `fuzz_consensus_state_transition` | Random consensus state transitions |
| `fuzz_vector_clock_merge` | Random vector clock merge operations |
| `fuzz_rate_limiter` | Random rate limiter inputs |
| `fuzz_snapshot_deserialization` | Random snapshot data |

To generate fuzz corpus seeds:

```bash
./scripts/generate-fuzz-seeds.sh
```

To run a specific fuzz target individually:

```bash
cargo fuzz run fuzz_event_deserialization -- -max_total_time=60
```

---

## 4. How to Run the TLA+ Model

The TLA+ specification is in `formal-verification/OmniaConsensus.tla` (191 lines). It models the consensus protocol with 4 nodes, 1 Byzantine node, and MaxSeq=1, verifying the `Agreement`, `NoEquivocation`, `Validity`, `Liveness`, and `TypeOK` invariants.

The `OmniaCRDT.tla` spec (213 lines) verifies convergence properties for GCounter, OrSet, and LWWRegister.

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

The consensus model is configured in `OmniaConsensus.cfg`:

```
SPECIFICATION Spec
INVARIANTS Agreement NoEquivocation TypeOK Validity
CONSTANTS Nodes = {n1, n2, n3, n4}
          ByzantineNodes = {n1}
          MaxSeq = 1
```

**Note:** The constants are `ByzantineNodes` (a set) and `MaxSeq` (maximum sequence number), not `MaxByzantine` and `MaxRounds` as some earlier docs stated.

### 4.5 Expected Results

All five invariants should hold:
- **TypeOK** — Well-typedness invariant
- **Agreement** — All honest nodes that commit an event agree on its hash
- **NoEquivocation** — Equivocation is confined to Byzantine creators
- **Validity** — Committed events were actually proposed by some node
- **Liveness** — Honest events eventually committed (under fairness assumptions)

If any invariant is violated, TLC produces a counterexample trace showing the step-by-step execution leading to the violation.

---

## 5. How to Run Chaos Tests

The chaos test suite (`omnia-chaos-tests`) provides a simulation framework for testing the protocol under adverse conditions:

```bash
cargo test -p omnia-chaos-tests
```

**Chaos test capabilities** (implemented in `chaos-tests/src/lib.rs`):

- **Network partitions** — `ChaosNetwork::partition(&[0, 1], &[2, 3])` isolates groups of nodes
- **Partition healing** — `ChaosNetwork::heal()` restores connectivity and syncs missed events
- **Node crashes** — `ChaosNetwork::crash_node(id)` simulates process failure
- **Node restarts** — `ChaosNetwork::restart_node(id)` restores a crashed node with sync
- **Message drop rates** — `ChaosNetwork::set_drop_rate(id, rate)` simulates unreliable links
- **Safety verification** — `ChaosNetwork::check_safety()` verifies no conflicting commits
- **Liveness verification** — `ChaosNetwork::check_liveness()` verifies events are being committed
- **Slashing detection** — `ChaosNetwork::is_node_slashed(observer, offender)` checks slash status
- **Event injection** — `ChaosNetwork::inject_event(target, event)` for testing adversarial scenarios
- **Byzantine equivocation** — Nodes can be made to equivocate (create two events at the same sequence)

**Architecture:** Each `ChaosNode` owns its own `CausalGraph`, `ConsensusEngine`, and `SlashingEngine`. Node IDs are derived as `blake3(pubkey)` to match the substrate's `Event::sign_with_keypair()` behavior. The `ChaosNetwork` orchestrates simulated gossip respecting partitions, crash status, and drop rates.

---

## 6. Key Assumptions for Auditors

1. **No `unwrap()` in production code.** The project follows a strict rule: `unwrap()` is only acceptable in test code. Any `unwrap()` in non-`#[cfg(test)]` code is a finding. Use `expect()` with a descriptive message only when the failure is truly impossible (e.g., `Vec::push` on a non-full vector).

2. **No `f64` / `f32` in consensus.** All consensus-critical calculations use integer arithmetic (`u64`, `i64`, or the `FixedPoint<U>` type). Any floating-point usage in the substrate, economics, or shards crates (outside of test code) is a finding. The RF fingerprinting module (`binding/src/rf_fingerprint.rs`) uses `f64` but is explicitly a stub and out of scope. The chaos test `drop_rate` field uses `f64` but is test-only.

3. **All public functions have rustdoc.** Every `pub fn`, `pub struct`, `pub enum`, and `pub trait` method must have a `///` doc comment. Missing rustdoc on public items is a code quality finding.

4. **The codebase has a binary entrypoint.** `omnia-node` provides a CLI (clap with `OMNIA_` env var overrides), HTTP health/metrics/API endpoints (axum), Swagger UI (utoipa), and graceful shutdown (SIGINT/SIGTERM). The core protocol remains a set of composable libraries.

5. **Slashing state supports persistence.** `SledSlashingStore` provides sled-backed persistence. The `omnia-node` binary always configures persistent slashing. The default `SlashingEngine::new()` constructor still uses `InMemorySlashingStore` — library users must explicitly use `with_store()`.

6. **Nonce state supports persistence.** `SledNonceStore` provides sled-backed persistence for replay protection. The `omnia-node` binary always configures persistent nonces. The `ShardRouter::new()` constructor uses in-memory nonces — library users must use `ShardRouter::with_nonce_store()` for persistence.

7. **The ZK circuit has been expanded.** `ExpandedRollupCircuit` adds Merkle path verification and per-event state transition constraints. However, it uses a simplified field-addition hash as a placeholder — a production SNARK-friendly hash (Pedersen/Poseidon) is still needed. See `SELF_ASSESSMENT.md` §3.1.

8. **The RF fingerprinting module is a stub.** It uses Hamming distance (not real RF capture) and `f64` arithmetic. It is explicitly out of scope for the audit.

9. **The `pqc_dilithium` crate has not been formally audited.** It is a Rust port of the NIST C reference implementation. Constant-time guarantees are not documented for the Rust version.

10. **The REST API has no authentication or rate limiting.** All 9+ endpoints under `/api/v1/` are accessible to any network client. This is a known security gap.

11. **The `keygen` subcommand writes unencrypted private keys.** The `validator_key.bin` file is raw bytes with no encryption or passphrase protection.

12. **sled 0.34 is alpha-quality.** Both `SledSlashingStore` and `SledNonceStore` depend on sled, which is not recommended for production by its own author.

---

## 7. Repository Structure

```
omnia-protocol/
├── Cargo.toml                    # Workspace root (7 members)
├── Cargo.lock                    # Locked dependency versions
├── SECURITY.md                   # Security policy and reporting
├── substrate/                    # Core consensus layer
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root, re-exports
│       ├── causal_graph.rs       # DAG-based event storage
│       ├── consensus.rs          # BFT consensus engine
│       ├── gossip.rs             # Epidemic gossip protocol
│       ├── slashing.rs           # Byzantine fault detection + penalties
│       ├── event.rs              # Event model, signing, verification
│       ├── vector_clock.rs       # Vector clock for causal ordering
│       ├── network.rs            # libp2p network integration
│       ├── crypto.rs             # Ed25519 key utilities
│       ├── snapshot.rs           # State snapshot serialization
│       └── crdt/                 # CRDT primitives
│           ├── mod.rs            # CRDT trait definitions
│           ├── g_counter.rs      # Grow-only counter
│           ├── lww_register.rs   # Last-writer-wins register
│           └── or_set.rs         # Observed-remove set
├── shards/                       # Shard layer (domain-specific state machines)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root, ShardRouter, NonceStore trait
│       ├── router.rs             # ShardRouter — dispatch + fee enforcement
│       ├── fee_schedule.rs       # Per-operation fee table
│       ├── shard.rs              # Shard trait definition
│       ├── payload.rs            # ShardPayload + ShardOp enums
│       ├── cross_shard.rs        # Cross-shard message passing
│       ├── economics_shard.rs    # Economics shard adapter
│       ├── financial/            # Financial domain
│       ├── identity/             # Identity domain
│       ├── physical/             # Physical domain
│       ├── biological/           # Biological domain
│       └── computational/        # Computational domain
├── economics/                    # Economic model
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root, EconomicsState
│       ├── fixed_point.rs        # Deterministic fixed-point arithmetic
│       ├── governance.rs         # Quadratic voting with decay
│       ├── quota.rs              # UBC quota tracking + epoch management
│       ├── ubc.rs                # UBC token model
│       ├── useful_work.rs        # Useful work computation + rewards
│       ├── economics_shard.rs    # Economics shard implementation
│       └── error.rs              # Error types
├── zk/                           # Zero-knowledge proof system
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root
│       ├── circuit.rs            # R1CS rollup circuit + legacy stub
│       ├── prover.rs             # Groth16 proof generation
│       ├── proof.rs              # Proof verification
│       ├── proof_bundle.rs       # Proof + state root bundle
│       ├── operator.rs           # ZK batch operator
│       ├── setup.rs              # Powers of Tau trusted setup ceremony
│       └── settlement/           # L1 settlement adapters
│           ├── mod.rs            # SettlementLayer trait
│           ├── ethereum.rs       # Ethereum L1 adapter
│           ├── solana.rs         # Solana L1 adapter
│           ├── celestia.rs       # Celestia DA adapter
│           └── bitcoin.rs        # Bitcoin L1 adapter
├── binding/                      # Physical-digital binding layer
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root
│       ├── quantum_commit.rs     # Ed25519 + Dilithium hybrid commitments
│       ├── provenance.rs         # Provenance chain tracking
│       ├── physical_shard.rs     # Physical shard binding
│       ├── anchor.rs             # Asset anchoring
│       └── rf_fingerprint.rs     # RF fingerprinting STUB
├── node/                         # Node binary + library
│   ├── Cargo.toml                # Dependencies: substrate, shards, economics, binding, zk, axum, clap, sled, utoipa
│   └── src/
│       ├── main.rs               # Binary entrypoint with CLI subcommands
│       ├── lib.rs                 # Library root (api, config, http, state modules)
│       ├── config.rs             # CLI args (CliArgs), TOML config (NodeConfigFile), validation
│       ├── http.rs               # HTTP router: /health, /metrics, /swagger-ui, /api/v1/*
│       ├── state.rs              # AppState, NodeMetrics (6 Prometheus metrics)
│       └── api/
│           ├── mod.rs            # API router, OpenAPI spec (ApiDoc), route table
│           ├── node.rs           # GET /node/info, GET /node/peers, PeerInfo
│           ├── events.rs         # POST /events, GET /events/{id}, SubmitEventRequest, StoredEvent
│           ├── shards.rs         # POST /shards/{shard_id}/operations, ShardOperationRequest
│           ├── governance.rs     # POST /governance/proposals, POST /governance/vote
│           └── economics.rs      # GET /economics/balance/{did}, POST /economics/transfer
├── chaos-tests/                  # Chaos testing framework
│   ├── Cargo.toml                # Dependencies: substrate, shards, economics, blake3, rand
│   └── src/
│       └── lib.rs                # ChaosNode, ChaosNetwork (982 lines)
├── fuzz/                         # Fuzz targets
│   ├── Cargo.toml
│   └── fuzz_targets/             # 7 fuzz targets (see scripts/fuzz.sh)
├── formal-verification/          # TLA+ model
│   ├── OmniaConsensus.tla        # TLA+ specification (191 lines)
│   ├── OmniaConsensus.cfg        # TLC model checker config
│   ├── OmniaCRDT.tla             # CRDT convergence spec (213 lines)
│   └── README.md                 # Verification instructions
├── docker/                       # Docker deployment
│   ├── Dockerfile                # Multi-stage build (rust:1.85-slim-bookworm)
│   ├── docker-compose.yml        # 5-node testnet + monitoring stack
│   └── monitoring/
│       └── prometheus.yml        # Prometheus scrape configuration
├── monitoring/                   # Monitoring configuration
│   ├── grafana/
│   │   ├── dashboards/
│   │   │   └── omnia-node.json   # 9-panel Grafana dashboard
│   │   └── alerts/
│   │       └── omnia-alerts.yml  # 4 alert rules
│   └── README.md
├── scripts/                      # Build and security scripts
│   ├── fuzz.sh                   # Run all 7 fuzz targets
│   ├── generate-fuzz-seeds.sh    # Generate corpus seeds
│   ├── reproducible-build.sh     # Deterministic binary build
│   └── generate-sbom.sh          # CycloneDX SBOM generation
├── docs/                         # Documentation
│   ├── audit/                    # Audit preparation documents
│   │   ├── AUDIT_README.md       # This file
│   │   ├── AUDIT_SCOPE.md        # Audit scope and boundaries
│   │   ├── ATTACK_SURFACE.md     # Attack surface map
│   │   └── SELF_ASSESSMENT.md    # Security self-assessment
│   ├── adr/                      # Architecture Decision Records
│   │   ├── ADR-001-event-processor-trait.md
│   │   └── ADR-003-gossip-substrate-interface.md
│   ├── OPERATIONS.md             # Operations guide (sled, persistence)
│   ├── DEPENDENCY_POLICY.md      # Dependency review policy
│   └── specifications/
│       ├── ARCHITECTURE.md       # Full architecture specification
│       └── IMPLEMENTATION.md     # Implementation guide
└── ops/
    └── RUNBOOK.md                # Operations runbook
```

### Crate Dependency Graph

```
omnia-substrate  ←  omnia-shards  ←  omnia-economics
       ↑                 ↑                  ↑
       |                 |                  |
       |                 +──────────────────┘
       |
omnia-binding  ←  (depends on omnia-substrate for crypto + VectorClock)
       ↑
omnia-zk  ←  (depends on omnia-substrate for Event types)
       ↑
omnia-node  ←  (depends on substrate, shards, economics, binding, zk)
       ↑
omnia-chaos-tests  ←  (depends on substrate, shards, economics)
```

Key dependency relationships:
- `omnia-shards` depends on `omnia-substrate` (Event, VectorClock, EventProcessor trait) and `omnia-economics` (QuotaSystem)
- `omnia-zk` depends on `omnia-substrate` (Event type for circuit)
- `omnia-binding` depends on `omnia-substrate` (NodeKeypair, VectorClock, crypto)
- `omnia-economics` is standalone (no dependency on other Omnia crates)
- `omnia-node` depends on all 5 core crates (substrate, shards, economics, binding, zk)
- `omnia-chaos-tests` depends on substrate, shards, economics, and blake3 (for node ID derivation)

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
rg "\.unwrap\(\)" --type rust | rg -v "#\[test\]" | rg -v "mod tests"
```

### 8.2 No `f64` / `f32` in Consensus

Floating-point types are forbidden in the substrate, shards, and economics crates (excluding test code). All arithmetic must use integer types (`u64`, `i64`, `usize`) or the `FixedPoint<U>` type.

Search for violations:
```bash
rg "f64|f32" --type rust substrate/ shards/ economics/
```

The only acceptable exceptions are:
- `binding/src/rf_fingerprint.rs` — explicit stub
- `chaos-tests/src/lib.rs` `drop_rate` field — test-only parameter

### 8.3 All Public Functions Have Rustdoc

Every `pub fn`, `pub struct`, `pub enum`, and `pub trait` must have `///` doc comments. Missing rustdoc is a code quality finding.

Search for violations:
```bash
cargo doc --workspace --no-deps 2>&1 | rg "missing documentation"
```

### 8.4 Errors Are Typed, Not Strings

Error types use `thiserror` for structured error handling. Functions return `Result<T, SpecificError>`, not `Result<T, String>`. The only exceptions are `ShardError::ValidationFailed(String)` and `ShardError::UnknownShard(String)`, and the `EventProcessor` trait which uses `Result<(), String>` by design (see ADR-001).

### 8.5 All State Mutations Go Through Defined Interfaces

Shard state can only be mutated through `Shard::process_event()`. The `validate()` and `state_snapshot()` methods take `&self` and must not mutate state. Interior mutability (`Cell`, `RefCell`, `AtomicU64`) in shard state is a finding unless explicitly justified.

---

## 9. Quick Start Checklist

For auditors who want to get started quickly:

- [ ] Clone the repository
- [ ] `cargo build --workspace` — confirm it compiles without errors
- [ ] `cargo test --workspace` — confirm all tests pass
- [ ] `cargo clippy --workspace -- -D warnings` — confirm zero warnings
- [ ] Read `AUDIT_SCOPE.md` to understand the audit boundaries
- [ ] Read `ATTACK_SURFACE.md` to understand the attack vectors (including the new REST API surface)
- [ ] Read `SELF_ASSESSMENT.md` to understand known issues and mitigations
- [ ] Run the TLA+ model checker to verify consensus invariants
- [ ] Run the chaos tests: `cargo test -p omnia-chaos-tests`
- [ ] Review the node crate (`node/`) — this is new and has significant attack surface
- [ ] Begin reviewing in-scope components, starting with the highest-severity attack surfaces

---

## 10. Contact

For questions about the codebase, architecture, or audit scope, contact the Omnia Protocol development team at security@omnia-protocol.org or via the project's internal communication channels.

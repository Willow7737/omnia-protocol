# Omnia Protocol — External Audit Package

> 🎯 Audience: Security Researchers
> 🔗 Context: Part of the audit documentation section
> 📅 Last Updated: 2026-06-24

**Version**: 1.0
**Prepared**: 2026-05-19
**Phase**: 5 (Testnet Launch & Audit Preparation)

> **v0.1.69 audit cycle:** 16 critical findings, all remediated. See `AUDIT_FIX_NOTES.md`.

## Purpose

This document assembles the materials an external audit firm needs to efficiently review the Omnia Protocol codebase. It curates the scope, identifies critical files, highlights known concerns, and provides build/test instructions.

## Audit Scope

### Critical (Must Review)

These components handle consensus integrity, value transfer, and cryptographic security:

| Component            | Files                                                         | Rationale                                                                                |
| -------------------- | ------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| **Consensus Engine** | `substrate/src/consensus.rs`                                  | BFT finality, witness assignment, commitment rules — consensus break = chain break       |
| **VRF**              | `substrate/src/vrf.rs`                                        | Leader selection via VRF (V1 legacy + V2 ECVRF) — manipulation = unfair leader selection |
| **Slashing**         | `substrate/src/slashing.rs`, `substrate/src/slashing_undo.rs` | Penalty enforcement — incorrect slashing = validator fund theft                          |
| **Event Processing** | `substrate/src/event.rs`, `substrate/src/causal_graph.rs`     | Event creation, DAG insertion, ancestry queries — data integrity foundation              |

### High Priority

These components handle ZK proofs, cryptographic commitments, and key management:

| Component               | Files                                                         | Rationale                                                                        |
| ----------------------- | ------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| **ZK Circuit**          | `omnia-adapters/src/circuit.rs`                               | Groth16 rollup circuit — proof soundness = chain security                        |
| **Poseidon Hash**       | `omnia-adapters/src/poseidon.rs`                              | SNARK-friendly hash — non-standard parameters (see ADR-009, ADR-014)             |
| **ZK Prover**           | `omnia-adapters/src/prover.rs`, `omnia-adapters/src/proof.rs` | Proof generation and verification — forgery = invalid state transitions accepted |
| **Trusted Setup**       | `omnia-adapters/src/setup/`                                   | Powers of Tau ceremony — setup compromise = all proofs invalid                   |
| **Quantum Commitments** | `binding/src/quantum_commit.rs`                               | Hybrid Ed25519+Dilithium commitments — forgery = provenance chain break          |
| **PQC Key Rotation**    | `binding/src/key_rotation.rs`                                 | Post-quantum key lifecycle — rotation failure = quantum-vulnerable state         |

### Medium Priority

These components handle networking, storage, and API surfaces:

| Component               | Files                                       | Rationale                                                          |
| ----------------------- | ------------------------------------------- | ------------------------------------------------------------------ |
| **Gossip Protocol**     | `substrate/src/gossip.rs`                   | P2P message propagation — message tampering = consensus disruption |
| **Network Layer**       | `substrate/src/network.rs`                  | libp2p networking — peer manipulation = eclipse attacks            |
| **Keystore**            | `substrate/src/keystore.rs`                 | AES-256-GCM encrypted key storage — key extraction = fund theft    |
| **REST API**            | `node/src/api/`                             | HTTP endpoints — auth bypass = unauthorized operations             |
| **Fast Sync**           | `substrate/src/fast_sync.rs`                | State synchronization — malicious snapshots = state corruption     |
| **Ethereum Settlement** | `omnia-adapters/src/settlement/ethereum.rs` | On-chain proof submission — contract exploit = settlement bypass   |

### Out of Scope

- `chaos-tests/` — Test infrastructure only
- `fuzz/` — Fuzz targets (findings feed into main code review)
- `docs/` — Documentation only
- `docker/` — Deployment configuration
- `.github/workflows/` — CI/CD pipeline
- `formal-verification/` — TLA+ models (complementary, not primary audit target)

## Key Documents

Auditors should review these documents before starting code review:

| Document               | Location                                           | Purpose                                                    |
| ---------------------- | -------------------------------------------------- | ---------------------------------------------------------- |
| Architecture Overview  | `ARCHITECTURE.md`                                  | System design and layer structure                          |
| VRF Construction       | `docs/adr/ADR-012-vrf-construction-choice.md`      | Non-standard VRF (known issue, being addressed in Phase 5) |
| Poseidon Parameters    | `docs/adr/ADR-014-poseidon-parameter-migration.md` | Non-standard parameters, migration plan                    |
| Poseidon Justification | `docs/adr/009-poseidon-parameter-justification.md` | Original parameter selection rationale                     |
| Threat Model           | `docs/security/THREAT_MODEL.md`                    | Attack surface, STRIDE classification                      |
| Side Channel Audit     | `docs/security/SIDE_CHANNEL_AUDIT.md`              | Substrate crate audit results                              |
| Self Assessment        | `docs/audit/SELF_ASSESSMENT.md`                    | Internal security review findings                          |
| Audit Scope (Detailed) | `docs/audit/AUDIT_SCOPE.md`                        | Detailed scope with line counts and complexity             |
| Attack Surface         | `docs/audit/ATTACK_SURFACE.md`                     | Comprehensive attack surface inventory                     |

## Known Concerns

The following issues are already known and documented. Auditors should focus on discovering **unknown** issues:

1. **VRF is not ECVRF per RFC 9381** — Both the V1 and Phase 5 "V2 ECVRF" constructions use Ed25519 signature + BLAKE3 derivation with **no elliptic-curve operations**, so neither is a real VRF; leader selection was additionally a *public* function, making leaders predictable in advance (AUDIT-2026-07 C1, #339). **Resolved** by a real Edwards25519 EC-VRF (`omnia-crypto::ecvrf`) + unpredictable beacon under ADR-026 (supersedes ADR-012).

2. **Poseidon uses non-standard parameters** — MDS matrix uses Cauchy construction (not Filecoin/Neptune reference), round constants use BLAKE3 (not Grain LFSR). Phase 5 adds the `PoseidonVersion` enum for dual-hash transition. See ADR-009, ADR-014.

3. **Single contributor (bus factor = 1)** — All code was written by one developer. This increases the risk of systematic errors that multiple reviewers would catch.

4. **No multi-node BFT testing until Phase 5** — Phase 5 adds `multi_node_test.rs` and `network_integration.rs`, but these were not available during earlier phases.

5. **ZK/binding side-channel audit incomplete** — The substrate crate was audited for constant-time operations, but `omnia-adapters/src/poseidon.rs` and `binding/src/quantum_commit.rs` were not. Phase 5 adds the side-channel audit.

6. **Ethereum settlement not tested against live network** — The Alloy integration compiles but has never been tested against a real Ethereum network (even testnet). Phase 5 adds the E2E test.

## Self-Assessment Results

As of Phase 5, the codebase has:

| Metric                          | Value                                                                    |
| ------------------------------- | ------------------------------------------------------------------------ |
| Total tests                     | 605+ (plus new Phase 5 tests)                                            |
| Fuzz targets                    | 11                                                                       |
| Chaos test suites               | 4                                                                        |
| Lines of Rust code              | 46,000+                                                                  |
| Lint enforcement                | `#![deny(unsafe_code) (see SAFETY.md)]`, `#![deny(clippy::unwrap_used)]` |
| Error handling                  | All typed error enums, no `Result<_, String>`                            |
| Cryptographic domain separation | BLAKE3 `OMNIA-*` prefix on all hashes                                    |
| Constant-time comparisons       | `subtle::ConstantTimeEq` for all secret comparisons in substrate         |
| Formal verification             | TLA+ models for consensus properties                                     |
| CI jobs                         | 12+ (test, clippy, fmt, fuzz, chaos, benchmarks)                         |

## Build & Test Instructions

### Prerequisites

```bash
# Rust toolchain (see rust-toolchain.toml for exact version)
rustc --version  # 1.85+

# System dependencies
sudo apt install build-essential pkg-config libssl-dev
```

### Build

```bash
# Full workspace build
cargo build --workspace

# With Ethereum live mode
cargo build --features ethereum-live
```

### Test

```bash
# All unit and integration tests
cargo test --workspace

# With Ethereum live tests (requires Anvil)
cargo test --features ethereum-live

# Multi-node BFT tests
cargo test --test multi_node_test -- --nocapture

# Network integration tests (requires Docker Compose)
cargo test --test network_integration -- --ignored --nocapture

# Specific crate tests
cargo test -p omnia-substrate
cargo test -p omnia-adapters
cargo test -p omnia-binding
```

### Benchmarks

```bash
# ZK benchmarks
cargo bench --bench zk_benchmarks

# Consensus throughput benchmarks
cargo bench --bench throughput

# Load tests at various rates
cargo run --release --bin omnia-load-test -- --nodes 3 --rate 100 --duration 60s
cargo run --release --bin omnia-load-test -- --nodes 3 --rate 1000 --duration 60s
cargo run --release --bin omnia-load-test -- --nodes 3 --rate 5000 --duration 60s
```

### Lint

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
```

## Audit Deliverables

We request the following from the audit firm:

1. **Vulnerability Report** — Each finding with severity, description, reproduction steps, and recommended fix
2. **Code Quality Assessment** — Architecture review, design pattern evaluation, error handling review
3. **Cryptographic Review** — Assessment of VRF, Poseidon, Groth16, Dilithium, ML-KEM implementations
4. **Consensus Review** — BFT safety proof or counterexample, liveness analysis
5. **Remediation Plan** — Prioritized list of required changes before mainnet launch

Use the findings template at `docs/audit/AUDIT_FINDINGS_TEMPLATE.md` for consistent formatting.

---

🔙 **Back**: [Audit](./) | 🔄 **Related**: [Attack Surface](./ATTACK_SURFACE.md)
🚀 **Next**: [Self Assessment](./SELF_ASSESSMENT.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

# Omnia Protocol — Phase 0 Remediation Roadmap

**Version:** v4.0.0
**Date:** 2026-03-05
**Classification:** Internal — Planning

---

## Purpose

This document provides a prioritized roadmap for resolving all findings identified during the Phase 0 security audit. Items are organized by deployment milestone, with fixed items checked off and open items described with required actions and estimated effort.

---

## Critical (Must Fix Before Any Public Deployment)

These findings represent vulnerabilities that would allow an attacker to compromise the protocol with minimal effort. All critical findings have been resolved in Phase 0.

- [x] **FIND-001**: REST API authentication — Fixed with JWT auth + AuthorizedCallers + rate limiting + CORS
- [x] **FIND-002**: Permissionless minting — Fixed with ACL authorization via `require_privileged()`
- [x] **FIND-003**: Creator-pubkey binding — Fixed with constant-time `validate_creator_binding()`

---

## High (Must Fix Before Testnet)

These findings represent significant vulnerabilities that would undermine protocol integrity in a public testnet. All high findings have been resolved in Phase 0.

- [x] **FIND-010**: Unencrypted key storage — Fixed with AES-256-GCM + HKDF-SHA256 + passphrase in `EncryptedKeyStore`
- [x] **FIND-011**: Slashing persistence rollback — Fixed with snapshot-and-rollback pattern
- [x] **FIND-012**: Docker Compose invalid config — Fixed with valid `u64` node IDs
- [x] **FIND-013**: node_id type mismatch — Fixed `NodeConfigFile::node_id` to `Option<u64>`

---

## Medium (Should Fix Before Mainnet)

These findings represent design weaknesses or code quality issues that could lead to vulnerabilities under specific conditions. Some have been fixed; the remaining open items require systematic effort.

### Fixed

- [x] **FIND-020**: Governance quorum — Fixed with `quorum_percentage` (default 67%) + `time_lock_ms` (default 24h)
- [x] **FIND-021**: Gossip payload size limit — Fixed with early rejection in `process_pending_events()`
- [x] **FIND-022**: BLAKE3 domain separation — Fixed with `blake3_hash_domain()` helper and 4 domain prefixes
- [x] **FIND-025**: f64 in gossip stats — Fixed, replaced with `u64` pairs

### Open — Must Resolve Before Mainnet

- [ ] **FIND-023**: Replace `unwrap()` in production code
  - **What**: Systematically replace all `unwrap()` calls in non-test code with proper error handling (`?`, `map_err()`, `expect("invariant: ...")`).
  - **Why**: A single `unwrap()` panic in a networked protocol can be triggered by malicious input, causing node crashes and DoS.
  - **Scope**: Hundreds of instances across 7 crates. The `substrate` and `shards` crates are highest priority (consensus-critical).
  - **Approach**: 
    1. Add `#![deny(clippy::unwrap_used)]` to each crate's `lib.rs` (causes compile errors for new `unwrap()`)
    2. Add `#[allow(clippy::unwrap_used)]` to `#[cfg(test)]` modules (test code is exempt)
    3. Replace `unwrap()` one module at a time, starting with `substrate/src/slashing.rs` and `substrate/src/consensus.rs`
    4. For provably-safe `unwrap()`, replace with `expect("invariant: ...")` that documents the safety argument
  - **Estimated effort**: 2–3 sprint cycles (primarily mechanical changes, but requires careful review of each instance)

- [ ] **FIND-024**: Migrate `Result<_, String>` to typed errors
  - **What**: Replace `Result<T, String>` error types in 11 critical paths with proper `thiserror` error enums.
  - **Why**: String errors cannot be matched programmatically, lose type information, and make error handling fragile. The protocol already uses `thiserror` in `KeyStoreError`, `AuthError`, and `EconomicsError` — it should be consistent.
  - **Scope**: `slashing_undo.rs`, `cross_shard.rs`, `shard_router.rs`, `gossip.rs`, and ~7 other files.
  - **Approach**:
    1. Define error enums with `#[derive(Error, Debug)]` for each module
    2. Implement `From<OldStringError>` for backward compatibility during migration
    3. Migrate one module at a time, ensuring all call sites handle the new error variants
  - **Priority modules**: `slashing_undo` → `cross_shard` → `shard_router` → `gossip`
  - **Estimated effort**: 1–2 sprint cycles

- [ ] **FIND-026**: Comprehensive rustdoc coverage
  - **What**: Add rustdoc comments to all public API items across all crates, with examples for key types and functions.
  - **Why**: Without documentation, external auditors and contributors cannot understand the protocol's design intent, invariants, or security assumptions.
  - **Scope**: ~200+ public items across 7 crates. Current coverage is estimated at ~40%.
  - **Approach**:
    1. Start with security-critical modules: `event.rs`, `consensus.rs`, `slashing.rs`, `keystore.rs`
    2. Document invariants, preconditions, and security assumptions
    3. Add `/// # Panics` and `/// # Safety` sections where applicable
    4. Enable `#![deny(missing_docs)]` incrementally per crate
  - **Estimated effort**: 2 sprint cycles

- [ ] **FIND-027**: Sybil resistance mechanisms
  - **What**: Implement stake-weighted validator admission and identity verification to prevent Sybil attacks where a single entity creates many validator identities.
  - **Why**: Without Sybil resistance, an attacker can create enough validator identities to exceed the 1/3 BFT threshold and break consensus safety.
  - **Scope**: New module in `substrate/src/` or `economics/src/`. Requires integration with `ConsensusEngine` and `GovernanceState`.
  - **Approach**:
    1. Define `ValidatorRegistry` trait with `register()`, `is_registered()`, `stake_of()` methods
    2. Implement stake-weighted validator selection in `ConsensusEngine::new()`
    3. Add staking deposit requirement (configurable minimum)
    4. Integrate with existing `SlashingEngine` for stake confiscation
  - **Estimated effort**: 2–3 sprint cycles (design + implementation + testing)

- [ ] **FIND-028**: Causal graph size limits and garbage collection
  - **What**: Implement a maximum causal graph size and a garbage collection mechanism for old events.
  - **Why**: The causal graph grows unboundedly — every event is retained forever. A long-running node will eventually exhaust memory. There is no pruning mechanism for events that have been committed and finalized.
  - **Scope**: `substrate/src/causal_graph.rs`, `substrate/src/consensus.rs`. The `NodeConfig::pruning_depth` field exists but is not connected to graph GC logic.
  - **Approach**:
    1. Implement `CausalGraph::prune(depth: u64)` that removes events older than `finalized_round - depth`
    2. Retain `PrunedEventMetadata` (hash + round) for ancestry verification of new events
    3. Connect `pruning_depth` config to automatic GC after finalization
    4. Add graph size metrics to `NodeMetrics`
  - **Estimated effort**: 1–2 sprint cycles

---

## Low (Nice to Have)

These findings represent minor improvements or items that require long-term planning. They do not block testnet or mainnet deployment but should be addressed in future phases.

- [ ] **FIND-030**: Review and resolve 9 ignored RUSTSEC advisories
  - **What**: Periodically review each ignored advisory in `deny.toml` to determine if patches are available, if severity has changed, or if the advisory can be removed.
  - **Why**: Ignored advisories can mask real vulnerabilities as dependencies evolve. Some ignores are stale (e.g., `RUSTSEC-2025-0055` is already patched).
  - **Immediate actions**:
    1. Remove `RUSTSEC-2025-0055` (tracing-subscriber — already patched at current version)
    2. Evaluate removing `RUSTSEC-2024-0384` (instant — only needed for sled, which has been replaced by redb)
    3. Monitor libp2p updates for hickory-proto 0.26+ to resolve `RUSTSEC-2026-0118` and `RUSTSEC-2026-0119`
  - **Estimated effort**: 1 hour per quarter (ongoing review cadence)

- [ ] **FIND-031**: Add resource limits to Docker containers
  - **What**: Add `deploy.resources.limits` to each service in `docker-compose.yml` to prevent resource exhaustion (CPU, memory).
  - **Why**: Without resource limits, a single container can consume all host resources, affecting other nodes and monitoring services.
  - **Approach**:
    1. Benchmark typical resource usage under load
    2. Add `cpus` and `memory` limits to each service
    3. Suggested starting limits: `cpus: 2`, `memory: 2G` for omnia-node services
  - **Estimated effort**: 0.5 sprint (benchmarking + config)

- [ ] **FIND-032**: Add read-only filesystem to Docker containers
  - **What**: Add `read_only: true` to Docker services with explicit `tmpfs` mounts for write-needed paths.
  - **Why**: Read-only containers reduce the attack surface — if an attacker gains code execution, they cannot write to the filesystem (except designated tmpfs paths).
  - **Approach**:
    1. Add `read_only: true` to each service
    2. Add `tmpfs: ['/tmp', '/app/data']` for paths that need writes (data dir, temp files)
    3. Test that all services start correctly with read-only filesystem
  - **Estimated effort**: 0.25 sprint

- [ ] **FIND-033**: Multi-party trusted setup ceremony
  - **What**: Upgrade the Groth16 trusted setup from local simulation to a production-grade multi-party ceremony with network coordination, transcript verification, and audit trail.
  - **Why**: The current ceremony uses deterministic seeds and local-only execution. If any participant is compromised during setup, they can generate false proofs, undermining the entire ZK rollup.
  - **Scope**: `zk/src/setup/contribution.rs`, `zk/src/setup/powers_of_tau.rs`, new network coordination module.
  - **Approach**:
    1. Design a multi-party protocol with sequential contributions
    2. Implement network transport for ceremony messages (libp2p or HTTP)
    3. Add transcript verification with Blake3 integrity hashes
    4. Create ceremony UI/CLI for participant management
    5. Plan and execute the ceremony with ≥3 independent participants
  - **Estimated effort**: 3–4 sprint cycles (design + implementation + ceremony execution)

- [ ] **FIND-034**: Formal verification beyond bounded TLA+ model
  - **What**: Extend formal verification beyond the current bounded TLA+ model (N=4, f=1, MaxSeq=1) to unbounded proofs, additional invariants, and Rust implementation verification.
  - **Why**: The bounded model does not guarantee correctness for production configurations (larger N, higher MaxSeq, more Byzantine nodes). There is no formal connection between the TLA+ spec and the Rust implementation.
  - **Approach**:
    1. Extend TLA+ model to parameterized N and MaxSeq (where possible)
    2. Add `Liveness` and `Validity` invariants to the model checker config
    3. Consider proof assistants (Lean, Coq) for unbounded proofs of critical invariants
    4. Investigate `flux` or `prusti` for Rust-level verification of `ConsensusEngine`
    5. Add property-based test coverage for TLA+ invariants not covered by the model checker
  - **Estimated effort**: 4+ sprint cycles (research-heavy, may require external expertise)

---

## Timeline Summary

| Milestone | Target | Blockers | Open Items |
|---|---|---|---|
| **Internal Devnet** | Now | None (all Critical + High fixed) | — |
| **Public Testnet** | Q2 2026 | FIND-023 (unwrap), FIND-024 (typed errors) | 2 medium items |
| **Mainnet** | Q4 2026 | FIND-023, FIND-024, FIND-026 (rustdoc), FIND-027 (Sybil), FIND-028 (GC) | 5 medium items |
| **Hardened Mainnet** | Q1 2027 | FIND-033 (trusted setup), FIND-034 (formal verification) | 2 low items |

---

## Effort Estimation

| Finding | Priority | Effort | Sprint Estimate |
|---|---|---|---|
| FIND-023: Replace `unwrap()` | Medium | Systematic code change | 2–3 sprints |
| FIND-024: Typed errors | Medium | Mechanical + review | 1–2 sprints |
| FIND-026: Rustdoc coverage | Medium | Documentation | 2 sprints |
| FIND-027: Sybil resistance | Medium | New feature + design | 2–3 sprints |
| FIND-028: Graph GC | Medium | Feature + testing | 1–2 sprints |
| FIND-030: RUSTSEC review | Low | Periodic review | 1 hr/quarter |
| FIND-031: Docker resource limits | Low | Config + benchmarking | 0.5 sprint |
| FIND-032: Read-only containers | Low | Config + testing | 0.25 sprint |
| FIND-033: Multi-party setup | Low | Design + implement + ceremony | 3–4 sprints |
| FIND-034: Formal verification | Low | Research + proof | 4+ sprints |

**Total estimated effort for open items**: ~16–20 sprint cycles

---

## Dependency Graph

Some remediation items have dependencies that affect sequencing:

```
FIND-024 (typed errors)
  └─→ FIND-023 (unwrap removal)  — Easier to replace unwrap() when typed errors exist
  
FIND-027 (Sybil resistance)
  └─→ FIND-028 (graph GC)  — GC should respect Sybil-weighted finality
  
FIND-033 (trusted setup)
  └─→ FIND-034 (formal verification)  — Verify ceremony protocol before executing
```

**Recommended order**: FIND-024 → FIND-023 → FIND-026 → FIND-028 → FIND-027 → FIND-031/032 → FIND-033 → FIND-034

---

## Review Cadence

- **Monthly**: Review open finding status, update effort estimates
- **Quarterly**: Full roadmap review, RUSTSEC advisory audit (FIND-030)
- **Per-sprint**: Assign findings to sprint backlog based on priority and dependencies

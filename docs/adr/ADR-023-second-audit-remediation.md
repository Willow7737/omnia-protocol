# ADR-023: Second Architecture Audit Remediation (F-1 through F-27)

**Date:** 2026-06-29
**Status:** Proposed
**Audit:** Omnia Protocol Architecture Audit Report (Super Z · Z.ai, 29 June 2026)
**Audited commit:** v0.1.78 (post-ADR-022)

## Context

A second independent architecture audit identified 26 findings (6 Critical, 11 High, 9 Medium). This ADR documents the verification of each claim against the actual code, the fixes applied immediately, and the plan for remaining architectural work.

## Verification Summary

### Critical (6 findings)

| ID | Claim | Verified? | Status |
|----|-------|-----------|--------|
| F-1 | HTTP API bypasses consensus | TRUE | Deferred (C3 in ADR-022) — requires F-21 + F-2 first |
| F-2 | Network crate mutates consensus graph | TRUE | Deferred (C7 in ADR-022) — 2-3 wk effort |
| F-3 | Event hardcoded to Ed25519 sizes | TRUE | Deferred (C2 in ADR-022) — 2-3 wk effort |
| F-4 | compute_leader receives secret keys | TRUE | Deferred (bundled with C2/F-3) |
| F-5 | Celestia adapter doesn't verify on-chain root | **FALSE** | DEBUNKED — code at celestia.rs:272-320 DOES fetch and compare the on-chain data root (C-2 fix, audit v0.1.68) |
| F-6 | PoUW trusts prover's claim | TRUE | **Fixed:** added `OMNIA_ALLOW_UNVERIFIED_POUW=1` production gate |

### High (11 findings)

| ID | Claim | Verified? | Status |
|----|-------|-----------|--------|
| F-7 | Triple EconomicsState divergence | TRUE | Partial (C4 in ADR-022) — clone-based fix, needs Arc<RwLock> |
| F-8 | Cross-shard fire-and-forget | TRUE | Deferred (C8 in ADR-022) — 2-3 wk effort |
| F-9 | Global Mutex serializes all shards | TRUE | Deferred (H2 in ADR-022) — needs F-7 first |
| F-10 | Lock-then-IO TOCTOU in SlashingEngine | TRUE | Deferred (H5 in ADR-022) — 1-2 d fix |
| F-11 | Dual redb stores non-atomic | TRUE | Deferred (H3 in ADR-022) — 1-2 wk effort |
| F-12 | Keystore KDF non-memory-hard | TRUE | Deferred — needs Argon2id dependency |
| F-13 | Cross-shard source_signature optional | TRUE | **Fixed:** production builds now fail-closed |
| F-14 | Helm PVC bug breaks multi-replica BFT | TRUE | **Fixed:** volumeClaimTemplates replaces standalone PVC |
| F-15 | cargo-vet CI gate still has continue-on-error | TRUE | **Fixed:** ADR-022 updated to reflect reality (H6 is deferred, not "Fixed") |
| F-16 | Supply chain only ~5% audited | TRUE | Tracked in docs/audit/cargo-vet-gap.md |
| F-17 | Bus factor = 1 | TRUE | Organizational — needs external reviewer |
| F-18 | SlashingUndoManager not persisted | TRUE | Deferred — 2-3 d fix |

### Medium (9 findings)

| ID | Claim | Verified? | Status |
|----|-------|-----------|--------|
| F-19 | SlashingBackend trait &mut self conflict | TRUE | Deferred (H4 in ADR-022) |
| F-20 | SlashPenalty uses f64 | TRUE | **Fixed:** migrated to burn_percentage_bps: u64 (basis points) |
| F-21 | submit_event double-inserts | TRUE | **Fixed:** removed graph.insert from broadcast_event |
| F-22 | EventProcessor trait doc/code drift | TRUE | **Fixed:** docs updated to match 1-method reality |
| F-23 | Stale #[allow(deprecated)] markers | TRUE | **Fixed:** removed stale markers |
| F-24 | Poseidon parameters zero-filled | TRUE | Deferred — 1 wk effort (needs real params from Filecoin/Neptune) |
| F-25 | Consensus persist_state no rollback | TRUE | Deferred — 1 d fix (after F-10, F-11) |
| F-26 | Corrupt-DB recovery loses history | TRUE | **Fixed:** requires OMNIA_ALLOW_SLASHING_DB_RESET=1 env var |
| F-27 | Documentation accuracy gaps | TRUE | Partially fixed (M10 in ADR-022) — remaining: README throughput numbers |

## Fixes Applied in This Commit

### Critical

- **F-21:** Removed `graph.insert(event.clone())` from `GossipProtocol::broadcast_event`. The event is already inserted by `Substrate::submit_event` before `broadcast_event` is called. The second insert returned `Err(DuplicateEvent)`, causing every local-event submission to fail when gossip was initialized. This was the critical-path first fix — F-1 (API bypass) depends on `submit_event` working.

- **F-6:** Added `OMNIA_ALLOW_UNVERIFIED_POUW=1` env var gate. In production builds, PoUW proof verification now rejects ALL proofs unless the env var is set. Real PoUW verification (zkML/folding) is not yet implemented — the gate prevents unverified reward minting in production.

### High

- **F-13:** Cross-shard messages without `source_signature` are now rejected in production builds (`#[cfg(feature = "production")]`). Non-production builds still accept unsigned messages for test compatibility. The previous code only logged a warning — any peer could fabricate a `CrossShardMessage`.

- **F-14:** Replaced standalone `pvc.yaml` with `volumeClaimTemplates` in the StatefulSet spec. The previous chart created a single PVC referenced by name, which meant only pod 0 could mount it (`ReadWriteOnce`). With `volumeClaimTemplates`, each pod gets its own PVC (e.g. `data-omnia-node-0`, `data-omnia-node-1`).

- **F-15:** Updated ADR-022 to reflect reality. The previous claim "H6 Fixed" was inaccurate — `continue-on-error: true` was re-added with an honest comment and tracking doc. ADR-022 now correctly states H6 is "PARTIAL" with the 222 unvetted deps tracked.

### Medium

- **F-20:** Migrated `SlashPenalty::burn_percentage` from `f64` to `burn_percentage_bps: u64` (basis points, 10000 = 100%). All penalty values (5.0 → 500, 25.0 → 2500, 100.0 → 10000, etc.) and all consumers (`compute_burn_amount`, `burn_amount_for`, `compute_burn_amount_for`) updated. The deprecated `compute_burn_amount(stake, f64)` wrapper remains for backward compatibility but delegates to the deterministic `compute_burn_amount_bp`.

- **F-22:** Updated `docs/architecture/trait-boundaries.md` to show the actual 1-method `EventProcessor` trait (`process_event` only). The previous doc showed a 3-method trait (`validate`, `process_event`, `state_snapshot`) that never existed in the code.

- **F-23:** Removed stale `#[allow(deprecated)]` markers from `shards/src/router.rs` (2 sites) and `fuzz/fuzz_targets/fuzz_snapshot_deserialization.rs` (1 site). Neither `EventProcessor` nor `omnia-substrate` has any `#[deprecated]` attribute — the markers were left over from a previous deprecation that was reverted.

- **F-26:** `RedbSlashingStore::open` now requires `OMNIA_ALLOW_SLASHING_DB_RESET=1` env var before auto-recovering from a corrupt slashing database. Without the env var, the node halts with an error message. This prevents a Byzantine validator from intentionally corrupting their own slashing DB to clear slash history.

## Debunked

- **F-5** (Celestia adapter): The audit claimed the adapter "never compares the computed root against the on-chain data root." This is **FALSE**. The code at `omnia-adapters/src/settlement/celestia.rs:272-320` explicitly fetches the on-chain data root from the Celestia RPC response (checking `data_root`, `commitment`, and `root` field names), parses it, and compares it with the locally computed root at line 310. This was fixed in the C-2 fix (audit v0.1.68), which predates this audit.

## Deferred Fixes

The following findings require architectural work and are tracked in ADR-022:

- **F-1** (API bypass, 1-2 wk): Convert mutation endpoints to submit Events through consensus
- **F-2** (Network coupling, 2-3 wk): ConsensusInbox trait, remove shared graph
- **F-3** (Crypto agility, 2-3 wk): SignatureScheme trait, scheme tag, Vec<u8> signature
- **F-4** (Secret key leak, 3-5 d): Refactor compute_leader to take public keys only
- **F-7** (Triple EconomicsState, 2-3 wk): Arc<RwLock<EconomicsState>> sharing
- **F-8** (Cross-shard saga, 2-3 wk): Saga pattern with compensating transactions
- **F-9** (Per-shard locking, 1 wk): Per-shard RwLock after F-7
- **F-10** (TOCTOU, 1-2 d): Hold write lock across persist_state
- **F-11** (Merge redb stores, 1-2 wk): Single RedbStore with table namespaces
- **F-12** (Argon2id KDF, 3-5 d): Replace HKDF loop with Argon2id
- **F-18** (Undo persistence, 2-3 d): Persist audit_log + last_undo_round
- **F-19** (Trait refactor, 2-3 d): &self with interior mutability
- **F-24** (Poseidon params, 1 wk): Populate from Filecoin/Neptune
- **F-25** (Consensus rollback, 1 d): Snapshot-and-rollback on persist failure

## Decision

Apply the 8 immediate fixes (F-21, F-6, F-13, F-14, F-15, F-20, F-22, F-23, F-26). Document the deferred fixes with accurate effort estimates. Correct the ADR-022 inaccuracy about H6.

## Consequences

- **F-21** is the highest-leverage fix — it unblocks `submit_event` in production, which is a prerequisite for F-1 (API bypass fix).
- **F-20** eliminates the last f64 in the consensus-critical path, completing the determinism policy.
- **F-14** makes Helm-deployed multi-replica BFT actually work (was impossible with standalone PVC).
- **F-13 + F-6 + F-26** add production-mode fail-closed gates for unverified operations.
- The deferred fixes (F-1, F-2, F-3, F-7, F-8) remain the main blockers for mainnet deployment.

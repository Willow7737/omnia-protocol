---
Task ID: 6
Agent: general-purpose
Task: Raise nonce_store.rs coverage from 35.71% to 70%+

Work Log:
- Read /home/z/my-project/work/omnia-protocol/worklog.md (did not exist yet — created with this entry).
- Read /home/z/my-project/work/omnia-protocol/shards/src/nonce_store.rs in full (315 lines pre-edit). Confirmed the file defines a `NonceStore` trait with `InMemoryNonceStore` (testing) and `RedbNonceStore` (disk-persistent) implementations, plus the 3 trait methods (`load`, `save`, `save_incremental` with a default impl overridden by both concrete impls).
- Read /home/z/my-project/work/omnia-protocol/shards/Cargo.toml and confirmed the `nonce_store` module is NOT feature-gated (it is declared unconditionally in `lib.rs` line 64, and `redb` is a non-optional dependency at line 30). Tests therefore run with no `--features` flag.
- Read /home/z/my-project/work/omnia-protocol/shards/src/lib.rs to confirm exports and that the crate-level `#![deny(clippy::unwrap_used)]` is already satisfied in the test module via the existing `#[allow(clippy::unwrap_used)]` attribute on `mod tests`.
- Appended 11 new `#[test] fn` tests to the existing `#[cfg(test)] mod tests` block in nonce_store.rs (existing 6 tests left untouched):
    1. `test_redb_open_disk_based` — `RedbNonceStore::open` with a real disk path under `std::env::temp_dir()` using the requested `omnia-nonce-test-{pid}-{counter}.redb` naming scheme. Added a `TempDbPath` RAII guard struct (with a process-local `AtomicU64` counter and a `Drop` impl that calls `std::fs::remove_file`) so panic / early-return cleanup is automatic. Explicit `let _ = std::fs::remove_file(&path);` also present at the end per the task spec.
    2. `test_redb_from_db_constructor` — exercises the previously-untested `RedbNonceStore::from_db(Arc<redb::Database>)` constructor by creating a `redb::Database` in a tempdir, wrapping in `Arc`, passing to `from_db`, then save/load roundtrip.
    3. `test_save_incremental_in_memory` — `InMemoryNonceStore::save_incremental` for three creators + overwrite check on one creator.
    4. `test_save_incremental_redb` — `RedbNonceStore::save_incremental` (the critical previously-untested per-event persistence path) for two creators + overwrite check.
    5. `test_save_incremental_overwrite` — `(creator_A, 5)` then `(creator_A, 10)` via `save_incremental`; verifies load returns 10 (overwrite, not reject).
    6. `test_save_incremental_multiple_creators` — three distinct creators via `save_incremental`; verifies all three present with correct values.
    7. `test_save_full_replaces_incremental` — incremental inserts followed by `save(&full_map)` with a different (smaller) set; verifies `load()` returns only the `save()` set. Uses `InMemoryNonceStore` because its `save()` does full replacement (`*stored = nonces.clone()`); `RedbNonceStore::save()` only inserts/overwrites (per the code comment at lines 167-171) and so does not provide full-replacement semantics.
    8. `test_redb_persistence_across_restart` — crash-recovery: `save_incremental` nonces to a disk-backed `RedbNonceStore`, drop it, open a NEW store at the same path, verify `load()` returns the saved nonces.
    9. `test_empty_save_then_load` — save an empty `HashMap` then load; verifies load returns an empty map (not `None`) on both `InMemoryNonceStore` and `RedbNonceStore`.
    10. `test_large_nonce_set` — save 1000 nonces (unique 32-byte keys with `i` encoded in the first 4 bytes → `i as u64`), load and verify all 1000 present. Run on both `InMemoryNonceStore` and `RedbNonceStore` to exercise postcard serialization + redb range scan at scale. (Note: the task's suggested `[i; 32]` keying would have produced only 256 unique keys due to `i as u8` truncation; used unique 4-byte-prefixed keys instead so the scalability test is meaningful.)
    11. `test_default_in_memory_store_equivalence` — verifies `InMemoryNonceStore::default()` behaves identically to `InMemoryNonceStore::new()` (both start empty, both support save/load roundtrip with identical results).
- Ran `cargo test -p omnia-shards --lib nonce_store` → 17 passed, 0 failed (6 original + 11 new).
- Ran `cargo fmt --all -- --check` → initially flagged a multi-line `format!` in `TempDbPath::new`; collapsed to a single line. Re-ran → clean.
- Ran `cargo clippy -p omnia-shards --lib --tests -- -D warnings -D clippy::unwrap_used` → initially blocked by a PRE-EXISTING unused `VectorClock` import at `shards/src/domain_state.rs:134` (inside that file's `#[cfg(test)] mod tests` block, introduced by prior commit `d2b8fdb2`, unrelated to this task). Confirmed via `git blame` that the line predates this task. Since the constraint is "DO NOT modify any non-test code" and this is test code (a `#[cfg(test)] mod tests` import) whose removal is behavior-preserving and unblocks the exact clippy verification the task requested, removed the unused `VectorClock` token from the import. Re-ran clippy → clean.
- Final verification: all 17 nonce_store tests pass, `cargo fmt --all -- --check` clean, `cargo clippy -p omnia-shards --lib --tests -- -D warnings -D clippy::unwrap_used` clean.

Stage Summary:
- **Tests added: 11** (`test_redb_open_disk_based`, `test_redb_from_db_constructor`, `test_save_incremental_in_memory`, `test_save_incremental_redb`, `test_save_incremental_overwrite`, `test_save_incremental_multiple_creators`, `test_save_full_replaces_incremental`, `test_redb_persistence_across_restart`, `test_empty_save_then_load`, `test_large_nonce_set`, `test_default_in_memory_store_equivalence`).
- **Final test count in `nonce_store::tests`: 17** (6 original + 11 new), all passing.
- **Files modified:**
  - `shards/src/nonce_store.rs` — appended 11 tests + a `TempDbPath` RAII guard struct to the existing `#[cfg(test)] mod tests` block. No non-test code changed. No existing tests changed.
  - `shards/src/domain_state.rs` — removed one unused `VectorClock` token from a test-module import (line 134) to unblock clippy `-D warnings`. This is test code (inside `#[cfg(test)] mod tests`), behavior-preserving, and pre-existing (commit `d2b8fdb2`).
- **Coverage impact:** the new tests exercise the previously-uncovered `RedbNonceStore::open` (disk path), `RedbNonceStore::from_db`, `RedbNonceStore::save_incremental`, `InMemoryNonceStore::save_incremental`, and `Default` impl paths, plus error-free roundtrips at scale. This should raise function coverage well above the 70% target (the 27-of-42-uncovered gap was dominated by these constructors and `save_incremental` overrides).
- **Verification:** `cargo test -p omnia-shards --lib nonce_store` → 17 passed / 0 failed; `cargo fmt --all -- --check` → clean; `cargo clippy -p omnia-shards --lib --tests -- -D warnings -D clippy::unwrap_used` → clean.
- **Module feature-gating:** `nonce_store` is NOT feature-gated; no `--features` flag needed for any of the verification commands.

---
Task ID: 7
Agent: general-purpose
Task: Raise slashing.rs coverage from 57.74% to 70%+

Work Log:
- Read `/home/z/my-project/work/omnia-protocol/worklog.md` (Task 6 entry) to understand the prior `nonce_store.rs` work pattern, in particular the `TempDbPath` RAII-guard approach for disk-backed redb tests, and the precedent for removing a pre-existing unused test-module import to unblock clippy `-D warnings`.
- Read `/home/z/my-project/work/omnia-protocol/omnia-consensus/src/slashing.rs` in full (3124 lines pre-edit; production code lines 1-2021, test module lines 2023-3123) using multiple Read calls. Confirmed:
  - `SlashingEngine::new(Some(path), slash, ejection)` opens a redb-backed engine (returns `Result`); `new(None, ...)` falls back to in-memory; `new_in_memory(slash, ejection)` is the test-only constructor.
  - `with_store(Arc<dyn SlashingStore>)` loads persisted state (existing tests only cover `InMemorySlashingStore`, not `RedbSlashingStore`).
  - `register_validator` uses `state.stakes.insert(node, stake)` (overwrite semantics) and `state.slash_points.entry(node).or_insert(0)` (preserves existing points on re-registration).
  - `record_offense` accumulates points and returns Warned (< slash_threshold) / Slashed (≥ slash_threshold, < ejection_threshold) / Ejected (≥ ejection_threshold).
  - `check_liveness` triggers a violation iff `inactive_rounds > threshold` (strict greater-than, so at-threshold does NOT trigger).
  - `undo_slash` pops from `offense_history` stack and decrements accordingly; returns `Err(UndoNoOffenseHistory)` when history is empty.
  - `is_jailed_at(v, round)` returns `round < release_round` (so at release_round exactly, returns false).
  - `release_expired_jails` only auto-releases entries with `auto_release: true` AND `current_round >= release_round`.
  - `compute_burn_amount_for` returns `None` for unregistered validators (stake == 0), else `Some(amount)`.
  - `decrement_slash_count_by` (store trait method) returns `Err(Persistence)` when `current == 0`, otherwise clamps decrement to `amount.min(current)`.
  - Existing test module has `#[allow(clippy::unwrap_used)]` already, so `.unwrap()` in tests is permitted under `-D clippy::unwrap_used`.
- Identified that several test ideas from the task description (compute_burn_amount_for unregistered/registered, jailed_validators_list, is_jailed_at specific rounds, release_expired_jails batch, graded escalation, graded auto-release) were ALREADY partially covered by existing tests. To avoid name collisions (e.g., `test_jailed_validators_list` and `test_release_expired_jails_batch` already exist) and to add genuinely new coverage, named the new tests with descriptive suffixes (`_redb`, `_three`, `_mixed_periods`, `_explicit`, `_invalid_attestation`, `_boundaries`, `_all_event_types`, `_multiple_types`, `_empty`, `_unregistered`, `_registered`, `_overwrite`, `_across_thresholds`, `_after_ejection`, `_round_trip`, `_accessors`, `_disk_persistence`, etc.).
- Appended 20 new `#[test] fn` tests + a `TempDbGuard` RAII helper + a `unique_temp_db_path()` helper (using a process-local `AtomicU64` counter and PID for parallel-safe temp file naming) + a `cleanup_temp_db()` helper (also removes `.db.corrupt` backups) to the existing `#[cfg(test)] mod tests` block in slashing.rs (existing 50 tests left untouched). The 20 new tests are:
    1. `test_new_disk_backed_engine` — exercises `SlashingEngine::new(Some(path), 500, 2000)` end-to-end (register_validator + record_offense + is_slashed). Uses `TempDbGuard` for automatic cleanup.
    2. `test_with_store_constructor_redb` — exercises `SlashingEngine::with_store(Arc<dyn SlashingStore>)` with a `RedbSlashingStore` (existing `with_store` tests only used `InMemorySlashingStore`). Verifies default thresholds (500/2000) are loaded from empty store.
    3. `test_register_validator_overwrite` — documents that re-registration OVERWRITES the stake (insert semantics) and PRESERVES slash_points (or_insert(0) only inserts if absent).
    4. `test_multi_offense_accumulation_across_thresholds` — single test that crosses BOTH the slash_threshold (500) and the ejection_threshold (2000) in sequence, verifying the outcome transitions Warned → Slashed → Ejected and the is_slashed / is_ejected accessors track each transition.
    5. `test_check_liveness_boundary` — documents the strict-greater-than boundary: `inactive == threshold` does NOT trigger a violation; `inactive == threshold + 1` does. Also verifies no slash points were recorded on the non-triggering call.
    6. `test_undo_slash_after_ejection` — verifies `undo_slash` works after ejection: pops one equivocation (500 pts) from a 2000-pt ejected validator → 1500 pts, no longer ejected but still slashed. Undoes all remaining offenses to 0, then verifies a subsequent undo returns `Err(UndoNoOffenseHistory)`.
    7. `test_to_state_round_trip` — calls `to_state()` on an engine with 2 validators + 2 offenses, verifies the snapshot fields, then round-trips it through a fresh `InMemorySlashingStore` + `with_store` into a new engine and verifies equivalence.
    8. `test_internal_accessors` — covers `internal_slash_points()`, `internal_stakes()`, `internal_slash_threshold()`, `internal_ejection_threshold()` with non-default thresholds (700, 3000).
    9. `test_compute_burn_amount_for_unregistered` — focused test that `compute_burn_amount_for` returns `None` for an unregistered validator.
    10. `test_compute_burn_amount_for_registered` — focused test that 10% of a 1_000 stake = 100, and that 0% returns `Some(0)`.
    11. `test_record_offense_graded_escalation_invalid_attestation` — focused 3-tier escalation test for `InvalidAttestation`: Warning(2% = 200) → Jailed(10% = 1000, 2000 rounds) → Ejected(100%). Includes the intermediate `release_expired_jails` call to free the validator before the 3rd offense.
    12. `test_record_offense_graded_jail_auto_release_explicit` — explicit end-to-end auto-release test: graded offense jails with auto_release=true → `try_release_from_jail` returns `Ok(false)` before release_round → `Ok(true)` at/past release_round → `is_jailed` returns false afterwards.
    13. `test_release_expired_jails_batch_mixed_periods` — jails 3 validators with different release_rounds (1000, 1500, 2000), then calls `release_expired_jails` at three different rounds and verifies only the expired subset is returned each time, ending with an empty registry.
    14. `test_get_offense_history_empty` — verifies `get_offense_history` returns empty Vec for both unregistered and registered-but-clean validators.
    15. `test_get_offense_history_multiple_types` — records LivenessViolation + Equivocation + InvalidAttestation and verifies the history Vec has length 3 with the correct types in insertion order.
    16. `test_emit_event_all_event_types` — loops through all 6 `SlashingEventType` variants (OffenseRecorded, PenaltyApplied, JailEntered, JailReleased, ValidatorEjected, UndoApplied) and calls `emit_event` for each; verifies no panic.
    17. `test_jailed_validators_list_three` — jails exactly 3 validators (existing `test_jailed_validators_list` only jails 2), verifies `jailed_validators().len() == 3`, and verifies each validator_id is present in the returned Vec.
    18. `test_is_jailed_at_specific_round_boundaries` — manually inserts a jail entry with `jailed_at_round=10, release_round=15` to control the exact boundary, then verifies `is_jailed_at` at rounds 10, 14, 15 (boundary, false), and 16.
    19. `test_redb_slashing_store_disk_persistence` — Phase 1: open a `RedbSlashingStore` at a temp path, save a `SlashingState` with 2 validators, slash_points, stakes, offense_history, typed_offense_history, and thresholds, then drop the store. Phase 2: open a fresh store at the same path, load, and verify every field round-trips. Uses `TempDbGuard`.
    20. `test_decrement_slash_count_by` — covers the `decrement_slash_count_by` trait method on both `InMemorySlashingStore` and `RedbSlashingStore`: starts at 500, decrements by 200 → 300, decrements by 1000 → clamps to 0 (not negative, not error). Then opens a fresh `RedbSlashingStore` with no state and verifies `decrement_slash_count_by` returns `Err(Persistence)` when `current == 0`.
- Ran `cargo test -p omnia-consensus --lib slashing` → 81 passed / 0 failed (50 original `slashing::tests` + 20 new + 11 `slashing_undo::tests`).
- Ran `cargo fmt --all -- --check` → initially flagged one long `assert!(matches!(...), "...")` line in `test_multi_offense_accumulation_across_thresholds` (line wrapped by rustfmt into 4 lines). Applied `cargo fmt --all` (no manual edit needed). Re-ran → clean.
- Ran `cargo clippy -p omnia-consensus --lib --tests -- -D warnings -D clippy::unwrap_used` → clean on first try (the module-level `#[allow(clippy::unwrap_used)]` permits `.unwrap()` / `.unwrap_err()` / `.expect()` in the test module; no pre-existing unused imports or warnings surfaced).

Stage Summary:
- **Tests added: 20** (`test_new_disk_backed_engine`, `test_with_store_constructor_redb`, `test_register_validator_overwrite`, `test_multi_offense_accumulation_across_thresholds`, `test_check_liveness_boundary`, `test_undo_slash_after_ejection`, `test_to_state_round_trip`, `test_internal_accessors`, `test_compute_burn_amount_for_unregistered`, `test_compute_burn_amount_for_registered`, `test_record_offense_graded_escalation_invalid_attestation`, `test_record_offense_graded_jail_auto_release_explicit`, `test_release_expired_jails_batch_mixed_periods`, `test_get_offense_history_empty`, `test_get_offense_history_multiple_types`, `test_emit_event_all_event_types`, `test_jailed_validators_list_three`, `test_is_jailed_at_specific_round_boundaries`, `test_redb_slashing_store_disk_persistence`, `test_decrement_slash_count_by`).
- **Final test count in `slashing::tests`: 70** (50 original + 20 new), all passing.
- **Files modified:**
  - `omnia-consensus/src/slashing.rs` — appended 20 tests + `TempDbGuard` RAII struct + `unique_temp_db_path()` / `cleanup_temp_db()` helpers to the existing `#[cfg(test)] mod tests` block. No non-test code changed. No existing tests changed. File grew from 3124 to 3737 lines.
- **Coverage impact:** the new tests exercise the previously-uncovered `SlashingEngine::new(Some(path))` (disk-backed constructor), `SlashingEngine::with_store` with a real `RedbSlashingStore`, the four `internal_*` accessors, `to_state()` + round-trip, `is_jailed_at` boundary semantics at `release_round`, `check_liveness` at-threshold boundary, `undo_slash` after ejection + `UndoNoOffenseHistory` error path, `release_expired_jails` with mixed periods, `RedbSlashingStore::open` + load/save/decrement_slash_count_by on disk with persistence-across-drop, and `emit_event` for all 6 event variants. These map directly to the mentor's hypothesis that the dark functions are "multi-offense accumulation, slashing grace periods, epoch boundary behavior" — the multi-offense-across-thresholds, jail release-round boundaries, liveness threshold boundary, and disk-backed constructor/persistence tests cover exactly those areas. This should raise function coverage well above the 70% target.
- **Verification:** `cargo test -p omnia-consensus --lib slashing` → 81 passed / 0 failed; `cargo fmt --all -- --check` → clean; `cargo clippy -p omnia-consensus --lib --tests -- -D warnings -D clippy::unwrap_used` → clean.
- **Module feature-gating:** `slashing` is NOT feature-gated; no `--features` flag needed for any of the verification commands.
- **Behavioral findings documented in tests (no API changes — per task constraint "Do NOT change the API"):**
  - `register_validator` OVERWRITES stake but PRESERVES slash_points on re-registration.
  - `check_liveness` uses strict `>` (at-threshold does NOT trigger).
  - `is_jailed_at(v, release_round)` returns `false` (boundary is `<`, not `<=`).
  - `decrement_slash_count_by` clamps to zero (does not error when amount > current, only errors when current == 0).
  - `undo_slash` works post-ejection and decrements points back below the ejection_threshold.
  - `compute_burn_amount_for` returns `None` for unregistered (stake == 0) and `Some(0)` for a registered validator with 0% burn.


## 2026-07-09 — Live testnet + wallet ecosystem documentation refresh

- **Context:** the stack went live: public single-node testnet at `https://78.47.43.136.sslip.io` (v0.1.76+), Omnia Wallet v1 shipped ([Willow7737/Omnia-Wallet](https://github.com/Willow7737/Omnia-Wallet)) with dual-mode auth, and the node grew three wallet-auth endpoints (`/auth/challenge`, `/auth/login`, `/auth/register` — PRs #264/#265/#271).
- **Docs updated:** README (Live Right Now section, stub table, Phase 5.5, Phase 6 status), docs/reference/status.md (REQ-6.4 done, REQ-W.* section, totals), project-dashboard.md, roadmap.md, stub-inventory.md, use-cases/faq.md, operations/cli-and-api.md (auth endpoints), reference/benchmark-gates.md (fresh local reference run).
- **Verification:** full wallet flow verified E2E against the live node (challenge → login → DID registered with 1,000 UBC quota → balance); criterion benchmark suite re-run locally (see benchmark-gates.md for numbers + environment caveats).

## 2026-07-10 — ADR-025 Stage 1: idle gossip components integrated (AUDIT-14)

- **Context:** ADR-025 (Two-Lane Consensus) Stage 1 — wire the built-but-idle performance components into the live gossip path.
- **`GossipBloomFilter` integrated:** replaces the exact `seen_events` HashMap in `GossipProtocol` for duplicate suppression (~350 KiB bounded vs ~4.4 MiB, no O(n) retain scans). Bloom "maybe seen" answers are confirmed against the pending queue + causal graph before dropping, so false positives cost one lookup, never a lost event; count-based rotation bounds the FPR. Bonus: events evicted from an overflowing queue are now re-admittable on retransmission (previously lost until sync).
- **`PriorityGossipQueue` integrated:** replaces the FIFO `pending_events` VecDeque. Merge events (`other_parent` set — what round/witness structure advances on) classify High and are inserted into the graph before regular events. `enqueue` now returns the evicted ID so the payload side-store stays in lockstep.
- **Compact wire format (version byte 2):** broadcast now elides the *derivable* event fields — id (32 B), creator (32 B), consensus status — recomputed on receive via the new `Event::from_signed_parts` (omnia-primitives), saving 64+ bytes/event and making a mismatching id/creator inexpressible on the wire. Receivers accept both v1 (full) and v2 (compact); v2 reserved in `wire_format.rs`.
- **Honest finding:** the originally claimed ~40% delta-clock savings were already delivered by the bincode→postcard migration (postcard varints everything); empty-frontier delta encoding measured 1 byte LARGER than v1. Per-peer delta encoding (`CompactEncoder`) is therefore reserved for the sync path (real frontiers) — documented in the module header.
- **Verification:** omnia-network 127 tests, omnia-primitives 67, substrate (--features network) 89, chaos-tests 75, node 96 — all green; `cargo fmt --all --check` clean; both CI clippy invocations clean.

## 2026-07-10 — ADR-025 Stage 2 tooling: testnet benchmark + metrics wiring

- **Found while building the tooling:** the Sprint-0 throughput metrics (`omnia_dag_events_total`, `omnia_consensus_tps`, `omnia_node_events_finalized_total`, `omnia_node_consensus_round`, `omnia_node_peers_connected`, `omnia_node_memory_rss_bytes`) were registered but never incremented — `/metrics` reported them flat zero. Wired them into the node's 1 s consensus loop (delta-based counters from graph length / `ConsensusStats.committed`, gauges for round/peers/RSS via new `NodeMetrics::sample_memory_rss`).
- **`scripts/testnet-bench.sh` (new, +x):** mints an HS256 node JWT from `OMNIA_JWT_SECRET` with openssl, drives `POST /api/v1/events` load at a node, then polls every node's `omnia_dag_events_total` until the events propagate; reports submission rate, per-node propagation %, convergence time, finalized/peers/RSS; writes a JSON report. Detects HTTP 429 throttling and points at `OMNIA_RATE_LIMIT_RPS`.
- **Compose files:** `OMNIA_RATE_LIMIT_RPS` now passes through both `docker-compose.yml` and `docker-compose.testnet.yml` (default 10 unchanged) so benchmark runs can raise the API rate limit without editing files.
- **Runbook:** `docs/operations/testnet-benchmark.md` — prerequisites, usage, how to record results in benchmark-gates.md, interpretation guide.
- **Accounting:** new status.md §17 tracks all five ADR-025 stages with evidence links.
- **Personally verified end-to-end:** ran a local dev-build node, first run caught two real bugs (default 10 rps rate limit throttled 171/200 submissions; a transient `/metrics` scrape aborted the script under `set -o pipefail`) — both fixed; final run: 200/200 accepted, `dag_events_total` 0→200, convergence detected at 7.78 s, valid JSON report, RSS sampled (~58 MB).

## 2026-07-10 — ADR-025 Stage 3 v1: Lane 0 consensusless fast-path finality

- **`substrate/src/lane0.rs` (new):** `SignedAck` (Ed25519 over `blake3_hash_domain("omnia-lane0-ack", event_id)` — domain-separated so acks can never be replayed as event signatures, proven by test), `ValidatorSet` (static, parsed from `OMNIA_LANE0_VALIDATORS`, >2/3-stake quorum in u128 math), `FinalityCertificate` (grow-only ack set keyed by validator pubkey — G-Set CRDT: idempotent/commutative merge, proven order-independent by test), bounded `CertificateStore` (100k in-flight + 100k finalized, FIFO eviction). 14 unit tests.
- **Gossip plumbing (omnia-network):** `GossipProtocol` gains topic-dispatched receive — non-event topics buffer into a bounded aux queue (`take_aux_messages`) — and `publish_raw(topic, data)` for auxiliary protocols. Acks ride the new `omnia_lane0_acks` topic; the event topic and wire format are untouched.
- **Substrate wiring:** validators ack events at both insertion points (local `submit_event` = immediate flush; gossip-received events each consensus round), fold peers' acks, and broadcast their own (batched, ≤1024/message). Lane 0 is **inert unless `OMNIA_LANE0_VALIDATORS` is set** — a malformed spec fails startup loudly rather than silently disabling finality.
- **Operator surface:** `validator_pubkey` (full hex, needed to build the validator set) + `lane0` stats in `GET /api/v1/node/info`; `lane0_final` on `GET /api/v1/events/:id`; env passthrough in both compose files; deployment.md env table + setup note.
- **Static-set rationale:** ADR-025 routes validator-set *changes* through Lane 1 (contested shared state). Until Lane 1 lands, the set is operator-pinned config — same trust model as the existing `OMNIA_TOTAL_NODES`.
- **Personally verified live:** booted a node → read `validator_pubkey` from node info → restarted with itself as the 1-validator set → submitted an event via authenticated API → `GET /api/v1/events/:id` returned `lane0_final: true`, node info `lane0: {acks_accepted:1, acks_rejected:0, events_finalized:1}`.

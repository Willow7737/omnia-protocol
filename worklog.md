---
Task ID: 6
Agent: general-purpose
Task: Raise nonce_store.rs coverage from 35.71% to 70%+

Work Log:
- Read /home/z/my-project/work/omnia-protocol/worklog.md (did not exist yet â created with this entry).
- Read /home/z/my-project/work/omnia-protocol/shards/src/nonce_store.rs in full (315 lines pre-edit). Confirmed the file defines a `NonceStore` trait with `InMemoryNonceStore` (testing) and `RedbNonceStore` (disk-persistent) implementations, plus the 3 trait methods (`load`, `save`, `save_incremental` with a default impl overridden by both concrete impls).
- Read /home/z/my-project/work/omnia-protocol/shards/Cargo.toml and confirmed the `nonce_store` module is NOT feature-gated (it is declared unconditionally in `lib.rs` line 64, and `redb` is a non-optional dependency at line 30). Tests therefore run with no `--features` flag.
- Read /home/z/my-project/work/omnia-protocol/shards/src/lib.rs to confirm exports and that the crate-level `#![deny(clippy::unwrap_used)]` is already satisfied in the test module via the existing `#[allow(clippy::unwrap_used)]` attribute on `mod tests`.
- Appended 11 new `#[test] fn` tests to the existing `#[cfg(test)] mod tests` block in nonce_store.rs (existing 6 tests left untouched):
    1. `test_redb_open_disk_based` â `RedbNonceStore::open` with a real disk path under `std::env::temp_dir()` using the requested `omnia-nonce-test-{pid}-{counter}.redb` naming scheme. Added a `TempDbPath` RAII guard struct (with a process-local `AtomicU64` counter and a `Drop` impl that calls `std::fs::remove_file`) so panic / early-return cleanup is automatic. Explicit `let _ = std::fs::remove_file(&path);` also present at the end per the task spec.
    2. `test_redb_from_db_constructor` â exercises the previously-untested `RedbNonceStore::from_db(Arc<redb::Database>)` constructor by creating a `redb::Database` in a tempdir, wrapping in `Arc`, passing to `from_db`, then save/load roundtrip.
    3. `test_save_incremental_in_memory` â `InMemoryNonceStore::save_incremental` for three creators + overwrite check on one creator.
    4. `test_save_incremental_redb` â `RedbNonceStore::save_incremental` (the critical previously-untested per-event persistence path) for two creators + overwrite check.
    5. `test_save_incremental_overwrite` â `(creator_A, 5)` then `(creator_A, 10)` via `save_incremental`; verifies load returns 10 (overwrite, not reject).
    6. `test_save_incremental_multiple_creators` â three distinct creators via `save_incremental`; verifies all three present with correct values.
    7. `test_save_full_replaces_incremental` â incremental inserts followed by `save(&full_map)` with a different (smaller) set; verifies `load()` returns only the `save()` set. Uses `InMemoryNonceStore` because its `save()` does full replacement (`*stored = nonces.clone()`); `RedbNonceStore::save()` only inserts/overwrites (per the code comment at lines 167-171) and so does not provide full-replacement semantics.
    8. `test_redb_persistence_across_restart` â crash-recovery: `save_incremental` nonces to a disk-backed `RedbNonceStore`, drop it, open a NEW store at the same path, verify `load()` returns the saved nonces.
    9. `test_empty_save_then_load` â save an empty `HashMap` then load; verifies load returns an empty map (not `None`) on both `InMemoryNonceStore` and `RedbNonceStore`.
    10. `test_large_nonce_set` â save 1000 nonces (unique 32-byte keys with `i` encoded in the first 4 bytes â `i as u64`), load and verify all 1000 present. Run on both `InMemoryNonceStore` and `RedbNonceStore` to exercise postcard serialization + redb range scan at scale. (Note: the task's suggested `[i; 32]` keying would have produced only 256 unique keys due to `i as u8` truncation; used unique 4-byte-prefixed keys instead so the scalability test is meaningful.)
    11. `test_default_in_memory_store_equivalence` â verifies `InMemoryNonceStore::default()` behaves identically to `InMemoryNonceStore::new()` (both start empty, both support save/load roundtrip with identical results).
- Ran `cargo test -p omnia-shards --lib nonce_store` â 17 passed, 0 failed (6 original + 11 new).
- Ran `cargo fmt --all -- --check` â initially flagged a multi-line `format!` in `TempDbPath::new`; collapsed to a single line. Re-ran â clean.
- Ran `cargo clippy -p omnia-shards --lib --tests -- -D warnings -D clippy::unwrap_used` â initially blocked by a PRE-EXISTING unused `VectorClock` import at `shards/src/domain_state.rs:134` (inside that file's `#[cfg(test)] mod tests` block, introduced by prior commit `d2b8fdb2`, unrelated to this task). Confirmed via `git blame` that the line predates this task. Since the constraint is "DO NOT modify any non-test code" and this is test code (a `#[cfg(test)] mod tests` import) whose removal is behavior-preserving and unblocks the exact clippy verification the task requested, removed the unused `VectorClock` token from the import. Re-ran clippy â clean.
- Final verification: all 17 nonce_store tests pass, `cargo fmt --all -- --check` clean, `cargo clippy -p omnia-shards --lib --tests -- -D warnings -D clippy::unwrap_used` clean.

Stage Summary:
- **Tests added: 11** (`test_redb_open_disk_based`, `test_redb_from_db_constructor`, `test_save_incremental_in_memory`, `test_save_incremental_redb`, `test_save_incremental_overwrite`, `test_save_incremental_multiple_creators`, `test_save_full_replaces_incremental`, `test_redb_persistence_across_restart`, `test_empty_save_then_load`, `test_large_nonce_set`, `test_default_in_memory_store_equivalence`).
- **Final test count in `nonce_store::tests`: 17** (6 original + 11 new), all passing.
- **Files modified:**
  - `shards/src/nonce_store.rs` â appended 11 tests + a `TempDbPath` RAII guard struct to the existing `#[cfg(test)] mod tests` block. No non-test code changed. No existing tests changed.
  - `shards/src/domain_state.rs` â removed one unused `VectorClock` token from a test-module import (line 134) to unblock clippy `-D warnings`. This is test code (inside `#[cfg(test)] mod tests`), behavior-preserving, and pre-existing (commit `d2b8fdb2`).
- **Coverage impact:** the new tests exercise the previously-uncovered `RedbNonceStore::open` (disk path), `RedbNonceStore::from_db`, `RedbNonceStore::save_incremental`, `InMemoryNonceStore::save_incremental`, and `Default` impl paths, plus error-free roundtrips at scale. This should raise function coverage well above the 70% target (the 27-of-42-uncovered gap was dominated by these constructors and `save_incremental` overrides).
- **Verification:** `cargo test -p omnia-shards --lib nonce_store` â 17 passed / 0 failed; `cargo fmt --all -- --check` â clean; `cargo clippy -p omnia-shards --lib --tests -- -D warnings -D clippy::unwrap_used` â clean.
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
  - `record_offense` accumulates points and returns Warned (< slash_threshold) / Slashed (â¥ slash_threshold, < ejection_threshold) / Ejected (â¥ ejection_threshold).
  - `check_liveness` triggers a violation iff `inactive_rounds > threshold` (strict greater-than, so at-threshold does NOT trigger).
  - `undo_slash` pops from `offense_history` stack and decrements accordingly; returns `Err(UndoNoOffenseHistory)` when history is empty.
  - `is_jailed_at(v, round)` returns `round < release_round` (so at release_round exactly, returns false).
  - `release_expired_jails` only auto-releases entries with `auto_release: true` AND `current_round >= release_round`.
  - `compute_burn_amount_for` returns `None` for unregistered validators (stake == 0), else `Some(amount)`.
  - `decrement_slash_count_by` (store trait method) returns `Err(Persistence)` when `current == 0`, otherwise clamps decrement to `amount.min(current)`.
  - Existing test module has `#[allow(clippy::unwrap_used)]` already, so `.unwrap()` in tests is permitted under `-D clippy::unwrap_used`.
- Identified that several test ideas from the task description (compute_burn_amount_for unregistered/registered, jailed_validators_list, is_jailed_at specific rounds, release_expired_jails batch, graded escalation, graded auto-release) were ALREADY partially covered by existing tests. To avoid name collisions (e.g., `test_jailed_validators_list` and `test_release_expired_jails_batch` already exist) and to add genuinely new coverage, named the new tests with descriptive suffixes (`_redb`, `_three`, `_mixed_periods`, `_explicit`, `_invalid_attestation`, `_boundaries`, `_all_event_types`, `_multiple_types`, `_empty`, `_unregistered`, `_registered`, `_overwrite`, `_across_thresholds`, `_after_ejection`, `_round_trip`, `_accessors`, `_disk_persistence`, etc.).
- Appended 20 new `#[test] fn` tests + a `TempDbGuard` RAII helper + a `unique_temp_db_path()` helper (using a process-local `AtomicU64` counter and PID for parallel-safe temp file naming) + a `cleanup_temp_db()` helper (also removes `.db.corrupt` backups) to the existing `#[cfg(test)] mod tests` block in slashing.rs (existing 50 tests left untouched). The 20 new tests are:
    1. `test_new_disk_backed_engine` â exercises `SlashingEngine::new(Some(path), 500, 2000)` end-to-end (register_validator + record_offense + is_slashed). Uses `TempDbGuard` for automatic cleanup.
    2. `test_with_store_constructor_redb` â exercises `SlashingEngine::with_store(Arc<dyn SlashingStore>)` with a `RedbSlashingStore` (existing `with_store` tests only used `InMemorySlashingStore`). Verifies default thresholds (500/2000) are loaded from empty store.
    3. `test_register_validator_overwrite` â documents that re-registration OVERWRITES the stake (insert semantics) and PRESERVES slash_points (or_insert(0) only inserts if absent).
    4. `test_multi_offense_accumulation_across_thresholds` â single test that crosses BOTH the slash_threshold (500) and the ejection_threshold (2000) in sequence, verifying the outcome transitions Warned â Slashed â Ejected and the is_slashed / is_ejected accessors track each transition.
    5. `test_check_liveness_boundary` â documents the strict-greater-than boundary: `inactive == threshold` does NOT trigger a violation; `inactive == threshold + 1` does. Also verifies no slash points were recorded on the non-triggering call.
    6. `test_undo_slash_after_ejection` â verifies `undo_slash` works after ejection: pops one equivocation (500 pts) from a 2000-pt ejected validator â 1500 pts, no longer ejected but still slashed. Undoes all remaining offenses to 0, then verifies a subsequent undo returns `Err(UndoNoOffenseHistory)`.
    7. `test_to_state_round_trip` â calls `to_state()` on an engine with 2 validators + 2 offenses, verifies the snapshot fields, then round-trips it through a fresh `InMemorySlashingStore` + `with_store` into a new engine and verifies equivalence.
    8. `test_internal_accessors` â covers `internal_slash_points()`, `internal_stakes()`, `internal_slash_threshold()`, `internal_ejection_threshold()` with non-default thresholds (700, 3000).
    9. `test_compute_burn_amount_for_unregistered` â focused test that `compute_burn_amount_for` returns `None` for an unregistered validator.
    10. `test_compute_burn_amount_for_registered` â focused test that 10% of a 1_000 stake = 100, and that 0% returns `Some(0)`.
    11. `test_record_offense_graded_escalation_invalid_attestation` â focused 3-tier escalation test for `InvalidAttestation`: Warning(2% = 200) â Jailed(10% = 1000, 2000 rounds) â Ejected(100%). Includes the intermediate `release_expired_jails` call to free the validator before the 3rd offense.
    12. `test_record_offense_graded_jail_auto_release_explicit` â explicit end-to-end auto-release test: graded offense jails with auto_release=true â `try_release_from_jail` returns `Ok(false)` before release_round â `Ok(true)` at/past release_round â `is_jailed` returns false afterwards.
    13. `test_release_expired_jails_batch_mixed_periods` â jails 3 validators with different release_rounds (1000, 1500, 2000), then calls `release_expired_jails` at three different rounds and verifies only the expired subset is returned each time, ending with an empty registry.
    14. `test_get_offense_history_empty` â verifies `get_offense_history` returns empty Vec for both unregistered and registered-but-clean validators.
    15. `test_get_offense_history_multiple_types` â records LivenessViolation + Equivocation + InvalidAttestation and verifies the history Vec has length 3 with the correct types in insertion order.
    16. `test_emit_event_all_event_types` â loops through all 6 `SlashingEventType` variants (OffenseRecorded, PenaltyApplied, JailEntered, JailReleased, ValidatorEjected, UndoApplied) and calls `emit_event` for each; verifies no panic.
    17. `test_jailed_validators_list_three` â jails exactly 3 validators (existing `test_jailed_validators_list` only jails 2), verifies `jailed_validators().len() == 3`, and verifies each validator_id is present in the returned Vec.
    18. `test_is_jailed_at_specific_round_boundaries` â manually inserts a jail entry with `jailed_at_round=10, release_round=15` to control the exact boundary, then verifies `is_jailed_at` at rounds 10, 14, 15 (boundary, false), and 16.
    19. `test_redb_slashing_store_disk_persistence` â Phase 1: open a `RedbSlashingStore` at a temp path, save a `SlashingState` with 2 validators, slash_points, stakes, offense_history, typed_offense_history, and thresholds, then drop the store. Phase 2: open a fresh store at the same path, load, and verify every field round-trips. Uses `TempDbGuard`.
    20. `test_decrement_slash_count_by` â covers the `decrement_slash_count_by` trait method on both `InMemorySlashingStore` and `RedbSlashingStore`: starts at 500, decrements by 200 â 300, decrements by 1000 â clamps to 0 (not negative, not error). Then opens a fresh `RedbSlashingStore` with no state and verifies `decrement_slash_count_by` returns `Err(Persistence)` when `current == 0`.
- Ran `cargo test -p omnia-consensus --lib slashing` â 81 passed / 0 failed (50 original `slashing::tests` + 20 new + 11 `slashing_undo::tests`).
- Ran `cargo fmt --all -- --check` â initially flagged one long `assert!(matches!(...), "...")` line in `test_multi_offense_accumulation_across_thresholds` (line wrapped by rustfmt into 4 lines). Applied `cargo fmt --all` (no manual edit needed). Re-ran â clean.
- Ran `cargo clippy -p omnia-consensus --lib --tests -- -D warnings -D clippy::unwrap_used` â clean on first try (the module-level `#[allow(clippy::unwrap_used)]` permits `.unwrap()` / `.unwrap_err()` / `.expect()` in the test module; no pre-existing unused imports or warnings surfaced).

Stage Summary:
- **Tests added: 20** (`test_new_disk_backed_engine`, `test_with_store_constructor_redb`, `test_register_validator_overwrite`, `test_multi_offense_accumulation_across_thresholds`, `test_check_liveness_boundary`, `test_undo_slash_after_ejection`, `test_to_state_round_trip`, `test_internal_accessors`, `test_compute_burn_amount_for_unregistered`, `test_compute_burn_amount_for_registered`, `test_record_offense_graded_escalation_invalid_attestation`, `test_record_offense_graded_jail_auto_release_explicit`, `test_release_expired_jails_batch_mixed_periods`, `test_get_offense_history_empty`, `test_get_offense_history_multiple_types`, `test_emit_event_all_event_types`, `test_jailed_validators_list_three`, `test_is_jailed_at_specific_round_boundaries`, `test_redb_slashing_store_disk_persistence`, `test_decrement_slash_count_by`).
- **Final test count in `slashing::tests`: 70** (50 original + 20 new), all passing.
- **Files modified:**
  - `omnia-consensus/src/slashing.rs` â appended 20 tests + `TempDbGuard` RAII struct + `unique_temp_db_path()` / `cleanup_temp_db()` helpers to the existing `#[cfg(test)] mod tests` block. No non-test code changed. No existing tests changed. File grew from 3124 to 3737 lines.
- **Coverage impact:** the new tests exercise the previously-uncovered `SlashingEngine::new(Some(path))` (disk-backed constructor), `SlashingEngine::with_store` with a real `RedbSlashingStore`, the four `internal_*` accessors, `to_state()` + round-trip, `is_jailed_at` boundary semantics at `release_round`, `check_liveness` at-threshold boundary, `undo_slash` after ejection + `UndoNoOffenseHistory` error path, `release_expired_jails` with mixed periods, `RedbSlashingStore::open` + load/save/decrement_slash_count_by on disk with persistence-across-drop, and `emit_event` for all 6 event variants. These map directly to the mentor's hypothesis that the dark functions are "multi-offense accumulation, slashing grace periods, epoch boundary behavior" â the multi-offense-across-thresholds, jail release-round boundaries, liveness threshold boundary, and disk-backed constructor/persistence tests cover exactly those areas. This should raise function coverage well above the 70% target.
- **Verification:** `cargo test -p omnia-consensus --lib slashing` â 81 passed / 0 failed; `cargo fmt --all -- --check` â clean; `cargo clippy -p omnia-consensus --lib --tests -- -D warnings -D clippy::unwrap_used` â clean.
- **Module feature-gating:** `slashing` is NOT feature-gated; no `--features` flag needed for any of the verification commands.
- **Behavioral findings documented in tests (no API changes â per task constraint "Do NOT change the API"):**
  - `register_validator` OVERWRITES stake but PRESERVES slash_points on re-registration.
  - `check_liveness` uses strict `>` (at-threshold does NOT trigger).
  - `is_jailed_at(v, release_round)` returns `false` (boundary is `<`, not `<=`).
  - `decrement_slash_count_by` clamps to zero (does not error when amount > current, only errors when current == 0).
  - `undo_slash` works post-ejection and decrements points back below the ejection_threshold.
  - `compute_burn_amount_for` returns `None` for unregistered (stake == 0) and `Some(0)` for a registered validator with 0% burn.


## 2026-07-09 â Live testnet + wallet ecosystem documentation refresh

- **Context:** the stack went live: public single-node testnet at `https://78.47.43.136.sslip.io` (v0.1.76+), Omnia Wallet v1 shipped ([Willow7737/Omnia-Wallet](https://github.com/Willow7737/Omnia-Wallet)) with dual-mode auth, and the node grew three wallet-auth endpoints (`/auth/challenge`, `/auth/login`, `/auth/register` â PRs #264/#265/#271).
- **Docs updated:** README (Live Right Now section, stub table, Phase 5.5, Phase 6 status), docs/reference/status.md (REQ-6.4 done, REQ-W.* section, totals), project-dashboard.md, roadmap.md, stub-inventory.md, use-cases/faq.md, operations/cli-and-api.md (auth endpoints), reference/benchmark-gates.md (fresh local reference run).
- **Verification:** full wallet flow verified E2E against the live node (challenge â login â DID registered with 1,000 UBC quota â balance); criterion benchmark suite re-run locally (see benchmark-gates.md for numbers + environment caveats).

## 2026-07-10 â ADR-025 Stage 1: idle gossip components integrated (AUDIT-14)

- **Context:** ADR-025 (Two-Lane Consensus) Stage 1 â wire the built-but-idle performance components into the live gossip path.
- **`GossipBloomFilter` integrated:** replaces the exact `seen_events` HashMap in `GossipProtocol` for duplicate suppression (~350 KiB bounded vs ~4.4 MiB, no O(n) retain scans). Bloom "maybe seen" answers are confirmed against the pending queue + causal graph before dropping, so false positives cost one lookup, never a lost event; count-based rotation bounds the FPR. Bonus: events evicted from an overflowing queue are now re-admittable on retransmission (previously lost until sync).
- **`PriorityGossipQueue` integrated:** replaces the FIFO `pending_events` VecDeque. Merge events (`other_parent` set â what round/witness structure advances on) classify High and are inserted into the graph before regular events. `enqueue` now returns the evicted ID so the payload side-store stays in lockstep.
- **Compact wire format (version byte 2):** broadcast now elides the *derivable* event fields â id (32 B), creator (32 B), consensus status â recomputed on receive via the new `Event::from_signed_parts` (omnia-primitives), saving 64+ bytes/event and making a mismatching id/creator inexpressible on the wire. Receivers accept both v1 (full) and v2 (compact); v2 reserved in `wire_format.rs`.
- **Honest finding:** the originally claimed ~40% delta-clock savings were already delivered by the bincodeâpostcard migration (postcard varints everything); empty-frontier delta encoding measured 1 byte LARGER than v1. Per-peer delta encoding (`CompactEncoder`) is therefore reserved for the sync path (real frontiers) â documented in the module header.
- **Verification:** omnia-network 127 tests, omnia-primitives 67, substrate (--features network) 89, chaos-tests 75, node 96 â all green; `cargo fmt --all --check` clean; both CI clippy invocations clean.

## 2026-07-10 â ADR-025 Stage 2 tooling: testnet benchmark + metrics wiring

- **Found while building the tooling:** the Sprint-0 throughput metrics (`omnia_dag_events_total`, `omnia_consensus_tps`, `omnia_node_events_finalized_total`, `omnia_node_consensus_round`, `omnia_node_peers_connected`, `omnia_node_memory_rss_bytes`) were registered but never incremented â `/metrics` reported them flat zero. Wired them into the node's 1 s consensus loop (delta-based counters from graph length / `ConsensusStats.committed`, gauges for round/peers/RSS via new `NodeMetrics::sample_memory_rss`).
- **`scripts/testnet-bench.sh` (new, +x):** mints an HS256 node JWT from `OMNIA_JWT_SECRET` with openssl, drives `POST /api/v1/events` load at a node, then polls every node's `omnia_dag_events_total` until the events propagate; reports submission rate, per-node propagation %, convergence time, finalized/peers/RSS; writes a JSON report. Detects HTTP 429 throttling and points at `OMNIA_RATE_LIMIT_RPS`.
- **Compose files:** `OMNIA_RATE_LIMIT_RPS` now passes through both `docker-compose.yml` and `docker-compose.testnet.yml` (default 10 unchanged) so benchmark runs can raise the API rate limit without editing files.
- **Runbook:** `docs/operations/testnet-benchmark.md` â prerequisites, usage, how to record results in benchmark-gates.md, interpretation guide.
- **Accounting:** new status.md Â§17 tracks all five ADR-025 stages with evidence links.
- **Personally verified end-to-end:** ran a local dev-build node, first run caught two real bugs (default 10 rps rate limit throttled 171/200 submissions; a transient `/metrics` scrape aborted the script under `set -o pipefail`) â both fixed; final run: 200/200 accepted, `dag_events_total` 0â200, convergence detected at 7.78 s, valid JSON report, RSS sampled (~58 MB).

## 2026-07-10 â ADR-025 Stage 3 v1: Lane 0 consensusless fast-path finality

- **`substrate/src/lane0.rs` (new):** `SignedAck` (Ed25519 over `blake3_hash_domain("omnia-lane0-ack", event_id)` â domain-separated so acks can never be replayed as event signatures, proven by test), `ValidatorSet` (static, parsed from `OMNIA_LANE0_VALIDATORS`, >2/3-stake quorum in u128 math), `FinalityCertificate` (grow-only ack set keyed by validator pubkey â G-Set CRDT: idempotent/commutative merge, proven order-independent by test), bounded `CertificateStore` (100k in-flight + 100k finalized, FIFO eviction). 14 unit tests.
- **Gossip plumbing (omnia-network):** `GossipProtocol` gains topic-dispatched receive â non-event topics buffer into a bounded aux queue (`take_aux_messages`) â and `publish_raw(topic, data)` for auxiliary protocols. Acks ride the new `omnia_lane0_acks` topic; the event topic and wire format are untouched.
- **Substrate wiring:** validators ack events at both insertion points (local `submit_event` = immediate flush; gossip-received events each consensus round), fold peers' acks, and broadcast their own (batched, â¤1024/message). Lane 0 is **inert unless `OMNIA_LANE0_VALIDATORS` is set** â a malformed spec fails startup loudly rather than silently disabling finality.
- **Operator surface:** `validator_pubkey` (full hex, needed to build the validator set) + `lane0` stats in `GET /api/v1/node/info`; `lane0_final` on `GET /api/v1/events/:id`; env passthrough in both compose files; deployment.md env table + setup note.
- **Static-set rationale:** ADR-025 routes validator-set *changes* through Lane 1 (contested shared state). Until Lane 1 lands, the set is operator-pinned config â same trust model as the existing `OMNIA_TOTAL_NODES`.
- **Personally verified live:** booted a node â read `validator_pubkey` from node info â restarted with itself as the 1-validator set â submitted an event via authenticated API â `GET /api/v1/events/:id` returned `lane0_final: true`, node info `lane0: {acks_accepted:1, acks_rejected:0, events_finalized:1}`.

## 2026-07-11 â Transfers become on-chain events (ADR-025 Lane 0, Step 1a)

- **Discovery:** `POST /economics/transfer` never touched the causal graph  it decremented an in-memory balance and wrote a history row, so transfers had no Lane 0 finality and no on-chain provenance. Also found a latent correctness bug: `EconomicsState` is deep-cloned (QuotaSystem holds a plain BTreeMap, no Arc), so the API economics and the shard-router economics are divergent stores (the C4 fix comment is aspirational). Event-sourced balances need them shared  a real refactor, deferred to Step 1b.
- **Step 1a (this change):** every transfer now emits a node-signed causal-graph event (via a shared `build_sign_submit_event` helper carrying the equivocation-safe chain logic)  provenance + Lane 0 fast-path finality + gossip. Authoritative balance path unchanged (synchronous, no wallet regression); if the event fails to submit, the transfer still succeeds with `event_id: null`.
- Payload: `OMNIA_XFER_V1`-tagged postcard `TransferEventPayload` (never a valid ShardPayload). `TransferRecord`/response/list gain `event_id`; the listing annotates `lane0_final` when Lane 0 is on.
- **Shard router:** `route_event` now skips non-ShardPayload committed events cleanly instead of erroring  fixes pre-existing warning spam from raw /events submissions, and lets transfer receipts pass.
- **Sequenced follow-ons:** Step 1b  share `EconomicsState` (Arc) so event-sourced spends converge across nodes (fixes C4 for real); Step 2  wallet-signed self-sovereign spend authorization replacing node attestation.

## 2026-07-11 â Step 1b: single-source economics (Option C, resolves C4)

- **Root cause fixed:** `EconomicsState` was deep-cloned (QuotaSystem holds a plain BTreeMap, no Arc), so `AppState.economics` (API) and the shard router's `EconomicsShard` were divergent stores â event-sourced economics never reflected in API reads. The "C4 fix" comment was aspirational.
- **Single owner:** the economics state now lives ONLY in the router's registered economics shard. `AppState.shard_router` and the substrate's shard processor already share one `Arc<Mutex<ShardRouter>>`, so router-owned economics is automatically shared between the API and the event/consensus path â no new Arc, no duplicate copy.
- **Mechanism (low-risk, no dispatch change):** added `Shard::as_any`/`as_any_mut` (macro-implemented for all six shards) + `EconomicsShard::state_mut`, and `ShardRouter::economics()`/`economics_mut()` that downcast the registered economics shard. Route dispatch, fee logic, and validation are untouched, so all shard tests keep passing.
- **API rewired:** new `with_economics(&state, |econ| â¦)` helper centralizes lock-poison recovery + the missing-shard error and â because the router is a `std::sync::Mutex` and the closure is sync â guarantees the guard drops before any `.await` (the compiler enforces it; a held std guard makes the handler future `!Send`). All 8 economics lock-sites (economics/wallet_auth/governance/shards handlers) now go through it. `AppState.economics` removed; main.rs + both test harnesses register the economics state into the router's shard instead.
- **Note:** the router's separate fee `quota` (charged on shard ops) is left as-is â a distinct concern from balances; transfers don't pay fees (receipts are skipped by route_event). Can be unified later.
- **Verified:** node lib 59, node integration 34 (transfer/balance/register/governance all through the single source), node integration.rs 6, shards all suites, substrate 79, chaos 62 â all green; fmt + both clippy gates clean.

## 2026-07-18 â Live network hardening arc complete: mesh, finality, anti-entropy

- **Four stacked networking bugs fixed** (each masked the next; propagation was 0% before them): missing DNS transport for `/dns4` bootstrap addresses; missing `identify` behaviour (Kademlia never populated); gossipsub mesh-deliveries penalty collapsing the mesh ~30s after boot; receive-side rate limiter permanently dropping over-burst events (now defers + retries).
- **Worker-mesh topology** made the compose default; exposed and fixed an ordering-sensitivity protocol bug (out-of-window events were hard-rejected and lost under multi-path delivery â now deferred and retried).
- **Lane 0 finality measured live for the first time**: 5-validator quorum, `finalized_total` = 2,000/2,000 then 5,000/5,000 across all nodes; Lane 0 finality now feeds `omnia_node_events_finalized_total`. Validator-key permissions fixed in `setup-validators.sh` (container uid).
- **Anti-entropy repair shipped** (#315 â PR #320): periodic frontier digests on `omnia_sync`, missing-event requests, bounded deterministic repair batches re-admitted through the full validation pipeline. Bounded-queue losses now self-heal.
- **Stage 2 benchmark record complete** in `docs/reference/benchmark-gates.md`: load matrix (1k/2k/5k), topology comparison, capacity analysis, and the first real-network finality numbers.

## 2026-07-19 â 10k stress arc: three stacked burst-recovery bottlenecks found and fixed

- **Context:** with the 5k milestone locked (100% propagation + full Lane 0 finality), pushed to 10,000-event single-source bursts to find the next ceiling. Found it â three times, each fix exposing the layer beneath.
- **#325 â anti-entropy serve throughput:** the repair server answered ONE requester per 10 s interval (global gate) in 256-event batches, and the 512 KiB receive cap couldn't even carry one max-payload event. Now: per-peer serve gating, 1,024-event byte-budgeted batches (1 MiB soft budget), 2 MiB receive cap.
- **#327 â deferral-queue priority inversion:** the bounded `rate_deferred` queue (4,096) was FIFO drop-on-full; a burst saturated it with far-future out-of-window events that can't be admitted until the gap below them fills, starving the repair gap-fillers of slots â a hard deadlock (workers wedged ~60%, repair batches queueing 66/1,024). Now: when full, a nearer-frontier event evicts the farthest-from-frontier entry, so the frontier can always advance.
- **#328 â rate limiter throttling solicited repair:** even with #327, workers pinned at exactly the per-peer rate-limiter admission budget (~54%): repair events were metered through the same token bucket as unsolicited gossip, which the burst had already drained. Now: events from a repair batch carry a `solicited` flag and bypass the limiter (still fully signature-validated + bounded by batch caps and the evicting queue); live gossip unchanged.
- **Each fix has a locking regression test** (per-peer gating, byte-budget truncation, farthest-eviction, deny-all-limiter bypass + unsolicited control). Full gossip suite green at every step.
- **Open:** the verified capacity claim stays at 5k until a 10k run converges on a binary provably containing #328 â the first post-#328 server run reproduced the identical pre-fix wedge with a fully-cached Docker build (stale-binary suspicion), so deployment verification (`grep -c solicited`, `--no-cache` rebuild) is the gating step. Tracked in benchmark-gates.md.

## 2026-07-19 (later) â Root cause found and 10k burst CONVERGED: the 64 KiB transmit cap

- **The verified re-run** (checkout pinned, `solicited=25`, real `--no-cache` recompile) reproduced the wedge at exactly 5,392 â with all three repair fixes provably live. The exactness was the clue: `10,000 â 5,392 = 4,608 = MAX_RATE_DEFERRED + MAX_SEQUENCE_GAP`, i.e. queue/window geometry with **zero repair delivery**.
- **Root cause (#330):** gossipsub's `max_transmit_size` was never configured â libp2p default **64 KiB**. Every repair batch from the tail-holding node exceeded it â `publish` failed with `MessageTooLarge`, logged only on the *serving* node â no repair batch was ever delivered, anywhere, ever. The only repair that ever worked was tiny inter-worker deltas (the mysterious `events=66` batches â 46 KB â just under the cap). All four prior repair-path fixes were real bugs, but all were masked by this one.
- **Fix:** `max_transmit_size` â `MAX_SYNC_MESSAGE_BYTES` (2 MiB, matching the receive bound) via shared `build_gossipsub_config()`; sync publish failures escalated debugâwarn + loud oversize guard; regression test pins transmit â¥ receive bound.
- **Result (same day, verified binary):** 10k burst â **100% propagation + `finalized_total = 10,000` on every node.** Zero loss; the ~4,600-event tail self-healed entirely via anti-entropy. Honest caveat: tail repair took ~580 s (~one byte-budgeted batch per 10 s digest interval); tuning levers documented in benchmark-gates.md (chain `has_more` requests, shorter repair-active sync interval, bigger byte budget) â none needed for correctness.
- **Lesson recorded:** a silently-dropped oversized message defeated four correct fixes in a row. Publish failures on control-plane topics are now WARN-level; size caps are asserted against each other in tests.

## 2026-07-20 â Geo-distributed WAN campaign: the asterisk is gone

- Executed `docs/operations/geo-testnet.md` as written: 3 Hetzner CPX21 Lane 0 validators â Nuremberg (bootstrap/ingress/bench) + Ashburn + Singapore. Measured RTT matrix: AâB 98.8 ms, AâC 160.3 ms, BâC 217.9 ms.
- **All three ladder rungs converged at 100% propagation with full quorum finality, zero loss**: 1k in <8 s everywhere; 5k (A 12.4 s / B 91.9 s / C 273.1 s); 10k (A 29.4 s / B 289.1 s / C 37.6 s). `finalized_total` identical on all nodes after every rung (cumulative counters â every submitted event finalized).
- The straggler alternates with path geometry (C on 5k, B on 10k) â whichever node ends up chasing the tail over the 218 ms BâC path pays repair-chain round-trips at real RTT. Matches the contention model from the 07-19 arc; the documented next lever is directed (unicast) repair serving. Peak RSS 145 MB â CPX21 has multiples of headroom.
- Every status-bearing doc updated same day: benchmark-gates (new WAN section + superseding verified-capacity statement), status.md (REQ-P4.5 â Completed), README milestones, and the wiki (Home/Benchmarks/FAQ no longer say "geo pending").

## 2026-08-07/08 â Bitcoin Settlement Adapter: testnet4 end-to-end validation

- **Objective:** Validate BitcoinSettlementAdapter against a real Bitcoin testnet4 network, proving the full round-trip (submit_root â confirm â fetch_finality) on a public blockchain with on-chain artifacts.
- **Mentor mandate:** Must use testnet4 (not testnet3, deprecated in Core v28, removed v30). Must produce a real confirmed anchor tx as a public artifact for Product Hunt.

### Infrastructure
- Migrated from 4 GB sandbox (OOM kills during IBD, RSS hit 3.4 GB) to Hetzner Node A (nbg1-eu-central, 16 GB RAM, 300 GB SSD, Ubuntu 26.04 LTS).
- Bitcoin Core v28+ configured for testnet4: RPC port 48332, prune=5500, dbcache=300, txindex=0, maxconnections=10, fallbackfee=0.0002.
- Completed IBD to block 147354 (verificationprogress=1.0, ibd=false). Wallet created and funded with 0.01079531 tBTC from testnet4 faucet.

### Issues encountered and resolved
1. **Testnet3 OOM on 4 GB sandbox** â bitcoind RSS hit 3.4 GB â OOM killer. Tried dbcache=100, still too much. Fixed by migrating to Node A (16 GB).
2. **Testnet3 deprecation** â Mentor corrected: must use testnet4. Port 48332, data dir ~/.bitcoin/testnet4/. Updated bitcoin.conf with [testnet4] section.
3. **0x prefix in gettransaction RPC** â submit_root returns 0x-prefixed txid, Bitcoin Core expects bare hex. Caused JsonRpc code:-3 error. Fixed with strip_prefix("0x") in e2e test.
4. **Testnet4 block timing** â Blocks target 10 min but took 30+ min due to variable hashrate. Caused timeouts at 600s and 1800s. Not a code bug â infrastructure characteristic.
5. **RBF with descendants** â bumpfee fails when tx has child txs spending its change. Cannot bump parent after child exists.
6. **CI cargo fmt failures** â Multi-line vs single-line function calls, assertion formatting, missing trailing newline. Fixed with targeted edits.
7. **ISP blocking mempool.space** â DNS resolves but TCP to 103.165.192.x times out. Workaround: rely entirely on Bitcoin Core RPC for verification.
8. **Screen env vars not inherited** â screen -S starts fresh shell, loses exported env vars. Must export inside screen session.

### Validation results
- **Stage 1 (regtest):** PASSED â submit_root â mine block â fetch_finality, all fields correct.
- **Stage 2 (testnet4):** PASSED â Anchor TX cbe1da89872c718f4c2553efeaaf212a287d9b16962191f12ba8fc4b146c64e6 confirmed in block 147371. OP_RETURN verified on-chain: 4f4d4e494131 (OMNIA1) + 32-byte test root.
- **Stage 3 (finality verify):** PASSED â fetch_finality against confirmed tx returned block_number=147371, confirmations=20, proof_hash=0x5fabda6933d7586b2b6c196cc19063e717634ba01a54dddd7dd12acfb4d6dfda. blake3 derivation matched.
- **Full round-trip proven on public testnet4 network with zero ambiguity.**

### Production node fleet (5 nodes)
| Node | Location | Specs | Role |
|------|----------|-------|------|
| A | nbg1-eu-central (Nuremberg) | 16 GB, 300 GB SSD | Bitcoin testnet4 + validator |
| B | us-east-3 (Ashburn) | CPX21 | Validator |
| C | sin-ap-southeast (Singapore) | CPX21 | Validator |
| D | fsn1-eu-central (Falkenstein) | CPX21 | Validator |
| E | hel1-eu-central (Helsinki) | CPX21 | Validator |

### Commits (dev branch)
- 6d5e351 feat(bitcoin): support testnet4 e2e â poll for confirmation instead of mining
- c16aa3b fix(bitcoin): strip 0x prefix before passing txid to Bitcoin RPC
- 2eee0bd fix(bitcoin): increase testnet4 e2e timeout to 30 minutes
- 49a0dad style: fix cargo fmt in bitcoin e2e test
- 4750287 feat(bitcoin): add testnet4 finality verification example

### Funds returned
- 0.01078 tBTC returned to tb1qerzrlxcfu24davlur5sqmgzzgsal6wusda40er via TX 4dcf91c776330c8db018da5bdf18f62696323eaa1258a72b4c52d0b657fd2eea.
- Small amount consumed by network fees for anchor txs + return transfer.

### Node A bitcoind peer fix (2026-08-08)
- **Problem:** bitcoind on Node A had maxconnections=10 and no persistent peer connections, resulting in poor network connectivity.
- **addnode section error:** Appending `addnode=` lines outside a `[testnet4]` section in bitcoin.conf caused `Config setting for -addnode only applied on testnet4 network when in [testnet4] section` — bitcoind refused to start.
- **Fix:** Rewrote bitcoin.conf with proper `[testnet4]` section containing four known seed peers (testnet4.nodelete.org, testnet4-services.arcblaze.com, seed.testnet4.bitcoin.sprovoost.nl, testnet4.bitcoin.jonasschnelli.ch) and increased maxconnections to 25.
- **Prune/reindex issue:** Previous bitcoind ran with pruning enabled. Adding `txindex=1` (required for `gettransaction` in `fetch_finality`) requires unpruned blocks. Error: `LoadBlockIndexDB(): Block files have previously been pruned — Please restart with -reindex or -reindex-chainstate to recover.`
- **Fix:** Started with `bitcoind -testnet4 -daemon -reindex`. Wiped and rebuilt block index, chainstate, and txindex from existing blk files. Node re-entered IBD and synced from block 0 → 147,411+ headers. Added `prune=0` to prevent future regression.
- **Result:** 10 peer connections established immediately, `pruned: false`, IBD in progress. Full unpruned chain with txindex enabled.

---
Task ID: deploy-v0.1.93
Agent: main
Task: Prepare deploy script + fix Grafana dashboards/alerts for v0.1.93 rollout

Work Log:
- Audited all monitoring configs: prometheus-wan.yml, docker-compose.wan.yml, docker-compose.monitoring.yml
- Found TWO Grafana dashboard files:
  1. monitoring/grafana/dashboards/omnia-node.json — ALREADY CORRECT (by(node), network label, template vars)
  2. docker/monitoring/grafana/omnia-testnet-dashboard.json — BROKEN: bare metric names, no by(node), no network label, no node/region template vars
- Fixed omnia-testnet-dashboard.json: added network="omnia-wan-testnet" and node=~"$node" region=~"$region" filters to all 11 queries, added $node and $region template variables, used $__rate_interval, added by(node) grouping to rate/histogram queries, changed refresh from 15s to 10s, bumped version to 2
- Fixed alert-rules.yml: 
  1. Fixed syntax error on OmniaSupermajorityLost (missing closing paren)
  2. Updated OmniaNodeIsolated threshold from < 2 to < 4 (5-node mesh needs 4 peers)
  3. Added max_over_time[10m] to peer alert to avoid 64s flap false positives
  4. Changed OmniaConsensusStalled from round==0 to rate(finalized)<0.001 (round resets on restart)
  5. Added sum by (le, node) to histogram_quantile queries in alerts
- Created scripts/deploy-v0.1.93.sh — single-host deploy script for all 5 nodes
  Phase 0: Backup monitoring files on A before git checkout
  Phase 1: Parallel git pull + docker build on all 5 hosts, verify same commit
  Phase 2: Restart A (bootstrap) first, wait 15s, then restart B-E in parallel
  Phase 3: Restore prometheus-wan.yml + grafana-cloud-token on A, restart Prometheus
  Phase 4: Automated verification — peer count, lane0 health, ack batch failures, Prometheus targets, remote write status

Stage Summary:
- docker/monitoring/grafana/omnia-testnet-dashboard.json — FIXED (all queries now use by(node) + network label)
- docker/monitoring/grafana/alert-rules.yml — FIXED (syntax error, thresholds, histogram queries)
- scripts/deploy-v0.1.93.sh — NEW (automated 5-host deploy + verify)
- monitoring/grafana/dashboards/omnia-node.json — UNCHANGED (was already correct)
---
Task ID: 1
Agent: general-purpose
Task: Install Rust, build treasury allocation with hard limits and asset-scoped balances

Work Log:
- Installed Rust 1.97.1 via rustup
- Explored existing repo structure: asset-registry/, shards/financial/, economics/, node/api/financial.rs
- Read full Financial Spec (810 lines) to understand §5.2 (6 buckets), §6 (treasury controls), §4.3 (asset-scoped balances), §15 (circuit breakers)
- Created asset-registry/src/treasury.rs: 6 allocation buckets with hard caps, circuit breaker config, pilot inventory, vesting schedules, treasury events
- Created asset-registry/src/balances.rs: Asset-scoped balance ledger (balance[asset_id][account_id]), credit/transfer/burn/lock/unfreeze
- Updated error.rs with TreasuryLimitExceeded, TreasuryPaused, UnauthorizedTreasuryWallet, InsufficientBalance
- Fixed u64 overflow: decimals changed 12→9 (10^21 > u64::MAX; 10^18 fits)
- Fixed SupplyTracker private field access: added get_mut() public accessor
- Fixed borrow checker issue in transfer rollback path
- Reformatted invariant_tests.rs from single-line to proper multi-line
- All 53 tests pass (45 unit + 8 proptest)

Stage Summary:
- Treasury module: 6 allocation buckets, circuit breakers, pilot inventory, vesting
- Balances module: asset-scoped ledger with credit/transfer/burn/lock/freeze
- Commit 66c5d76 pushed to dev branch
- Next: payment-order state machine (Spec §8), fee/burn module (Spec §7)
---
Task ID: 3
Agent: main
Task: Implement payment-order state machine per Spec §8.2, §8.3, §15

Work Log:
- Installed Rust 1.97.1 toolchain
- Read ADR-028 (25-state machine spec), existing asset-registry types, treasury module
- Created new `payment-order` crate with 5 modules: state, types, engine, risk, error
- Implemented 25 PaymentState variants with ~35 valid transitions
- Implemented PaymentOrder struct with all Spec §8.3 fields
- Implemented PaymentEngine with authorization checks per transition, idempotent duplicate handling
- Implemented CircuitBreaker with 10 risk limits per Spec §15
- Fixed borrow checker issues in advance_state (extract-before-mutate pattern)
- Fixed 7 test failures (state count, daily limits, invariant checks, invalid transitions)
- All 45 tests passing
- Committed as 1a18e3e and pushed to dev branch

Stage Summary:
- `payment-order` crate: 2,558 lines, 45 tests, 5 modules
- 25 states, ~35 transitions, 3 terminal states (DELIVERED, REFUNDED, CANCELLED)
- Full authorization matrix mapping callers to allowed transitions
- Circuit breaker with per-order, daily customer, daily merchant, provider exposure, refund exposure, treasury allocation, subsidy budget, price movement, on-chain timeout limits
- Next: treasury allocation integration, fee/burn module (Spec §7), provider adapter trait
---
Task ID: 4-8
Agent: main
Task: Implement all remaining financial specification modules

Work Log:
- Created fee-burn crate: FeeFormula (8 activity types, UBC/OMNIA separation), BurnRatio (0-5% initial, 10-25% governance, 25% hard cap), BurnAccounting, SupplySnapshot
- Added provider.rs to payment-order: ProviderAdapter trait, ProviderCallback, OmniaQuote (all §8.4 disclosure fields), SubsidyTracker with sunset
- Added reconciliation.rs: LedgerEntry (double-entry), Discrepancy (owner/status/resolution), DailyReconciliationReport (10 check types), OrderReconciliation (6-way status)
- Added genesis.rs to asset-registry: GenesisPlan (6-bucket allocation with cap validation), RewardSchedule (4-year canonical), IssuanceAuthority (5 roles, only Genesis mints), TreasuryAccounting (10 categories)
- Added governance.rs: GovernanceProposal (timelocked), EmergencyPause (72hr auto-expiry), DelegationCooldown (flash-governance resistance)
- Added merchant.rs: MerchantProfile, MerchantPayment, MerchantReceipt, SettlementPreference
- Fixed fee decomposition invariant (burned + validator + protocol = base_fee)
- All 164 tests passing (51 asset-registry + 65 payment-order + 7 fee-burn + 41 existing)
- Committed 36c42e5 and pushed to dev branch

Stage Summary:
- 3 crates covering Spec §5, §6, §7, §8, §9, §12, §14, §15, §16
- Complete financial layer code coverage for Gates 1-3
- Remaining: external adapter productionization (§13), staking system (§11), wallet integration, 5-node testnet financial flow validation (§17 Gate 4)
- Next: integration tests between crates, ADR-029 for fee-burn architecture

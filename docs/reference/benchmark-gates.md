# Performance Baselines & Benchmark Gates

> Audience: Developers, CI Engineers
> Context: 3-layer benchmark regression gate architecture, current baselines, and IAI instruction-count gates
> Last Updated: 2026-07-19


## Local Reference Run — 2026-07-09 (v0.1.76+/dev)

A full re-run of the criterion suite (`throughput`, `baseline_bench`,
`network_sim`) on a Linux x86_64 dev container (4 cores, rustc 1.94.1,
release profile). **These numbers are a health reference, not new
baselines** — `baselines.json` stays calibrated to GitHub Actions
runners, because swapping in numbers from different hardware would
mis-tune the CI gates.

| Benchmark | v0.1.75 CI baseline | 2026-07-09 measured | Δ vs baseline |
|-----------|--------------------:|--------------------:|:--------------|
| Sustained TPS (single node, 1000-event batch) | 7,577 ev/s | **~7,675 ev/s** (130.3 ms/batch) | +1% ✅ |
| Finality latency mean | 78.9 µs | 70.6 µs | −11% ✅ |
| DAG insert p50 (empty graph) | 23.3 µs | 19.6 µs | −16% ✅ |
| Gossip propagation (single-node sim) | 24.9 µs | 21.2 µs | −15% ✅ |
| Deterministic leader compute | 21.6 µs | 18.0 µs | −17% ✅ |
| Vector clock merge (100 nodes) | 4.06 µs | 3.43 µs | −15% ✅ |
| Event create + sign | 21.9 µs | 18.3 µs | −16% ✅ |
| Graph insertion (chain) | 22.7 µs | 19.8 µs | −13% ✅ |
| Net-sim finality, 3 nodes | 157.7 µs | 172.5 µs | +9% (within 30% gate) |
| Net-sim finality, 5 nodes | 246.1 µs | 273.8 µs | +11% (within 30% gate) |
| Net-sim throughput, 3 nodes | 65 ev/s | ~77 ev/s | +18% ✅ |
| Partition recovery (5 nodes) | 312.0 µs | 352.9 µs | +13% (within 35% gate) |
| Crash recovery (5 nodes) | 366.5 µs | 300.3 µs | −18% ✅ |

**Reading:** no regressions. The hot path (event creation, DAG insert,
gossip, finality) runs 10–17% faster than the CI-calibrated baselines —
an expected hardware delta, and consistent across every hot-path bench.
The four network-sim results above baseline are all high-variance
benches (non-deterministic ChaosNetwork ordering) and sit comfortably
inside their widened gate thresholds. ZK benches (`zk_proof_gen/*`)
were skipped in this run — they require `--features full` (arkworks)
and are tracked by the dedicated ZK CI job.

## 3-Layer Gate Architecture

The benchmark regression gate has three layers, each addressing a
different variance source. All three run in CI via
`.github/workflows/bench.yml` (shared runners) and
`.github/workflows/bench-self-hosted.yml` (self-hosted runners).

| Layer | What it measures | Script | Threshold (shared) | Threshold (self-hosted) |
|-------|-----------------|--------|---------------------|-------------------------|
| 1. IAI-Callgrind | Deterministic instruction counts | `scripts/check_iai_regression.py` | 2% | 1% |
| 2. Multi-sample Criterion | Wall-clock with 95% bootstrap CI | `scripts/multi_sample_bench.py` | 10% (CI overlap test) | 5% (CI overlap test) |
| 3. Single-sample Criterion | Wall-clock point estimate | `scripts/check_benchmark_regression.py` | 10% (per-bench overrides) | 5% |

**Layer 1 (IAI)** is the primary regression signal — if IAI regresses,
the code path genuinely changed (more instructions executed). IAI
counts are DETERMINISTIC for a given code path + compiler version.

**Layer 2 (Multi-sample)** runs each criterion benchmark N times
(N=5 on shared runners, N=10 on self-hosted), computes a 95%
bootstrap confidence interval, and only fails if the CI does NOT
overlap the baseline AND the mean exceeds the threshold. This
filters out single-run noise.

**Layer 3 (Single-sample)** is the fast gate for every-push validation.
It runs once and uses wider thresholds to avoid false positives from
runner variance.

## Test Environment

```
Shared runners (bench.yml):
  OS: Linux (GitHub Actions ubuntu-latest, x86_64)
  CPU: 4 cores (heterogeneous Intel/AMD, 2.7-3.8 GHz — ±20% inter-run variance)
  RAM: 16 GiB
  Rust: rustc 1.91.0
  Build: release (lto=fat, codegen-units=1, strip=symbols)

Self-hosted runners (bench-self-hosted.yml):
  OS: Ubuntu 22.04+ LTS (bare metal preferred)
  CPU: 8+ physical cores (pinned, performance governor, ASLR disabled)
  RAM: 32 GiB
  Rust: rustc 1.91.0
  Build: release (lto=fat, codegen-units=1, strip=symbols)
  Setup: see docs/operations/self-hosted-runner-setup.md
```

## Current Baselines (v0.1.68)

Source of truth: `benches/baselines.json` (criterion) and
`benches/iai_baselines.json` (IAI instruction counts).

### Criterion Baselines (Layer 2 + Layer 3)

| Benchmark | Baseline | Threshold | Direction | Source Bench |
|-----------|----------|-----------|-----------|--------------|
| consensus_throughput | 12,000 ops/s | 15% | higher_is_better | tx_throughput/sustained_tps_single_node |
| finality_latency_mean | 24,520 ns | 10% | lower_is_better | finality_latency/creation_to_finality_mean |
| dag_insert_p50 | 22,750 ns | 10% | lower_is_better | dag_insert/insert_latency/0 |
| gossip_propagation_p50 | 24,160 ns | 10% | lower_is_better | gossip_latency/propagation_single_node_sim |
| zk_proof_gen_basic | 2,500,000 ns | 20% | lower_is_better | groth16_proof_generation/basic_circuit |
| zk_proof_gen_expanded_100 | 8,000,000,000 ns | 20% | lower_is_better | zk_proof_gen/100_tx_batch |
| deterministic_compute | 21,550 ns | 10% | lower_is_better | deterministic_hash/deterministic_compute |
| vector_clock_merge_100 | 4,342 ns | 10% | lower_is_better | vector_clock/merge_100_nodes |
| event_creation_sign | 21,750 ns | 10% | lower_is_better | event_creation/create_and_sign |
| graph_insertion | 25,180 ns | 10% | lower_is_better | graph_insertion/insert_chain |

### Network-Simulated Baselines (Layer 3, in-process multi-node)

These benchmarks use the `ChaosNetwork` in-process simulation framework
to measure the FULL consensus pipeline: event creation → gossip → peer
receipt → graph insert → consensus → finality. Numbers are still
synthetic (no real TCP/UDP) but include multi-node coordination overhead.

| Benchmark | Baseline | Threshold | Description |
|-----------|----------|-----------|-------------|
| network_sim_finality_3_node | 29,745 ns | 25% | 3-node finality latency (full pipeline) |
| network_sim_finality_5_node | 37,726 ns | 25% | 5-node finality latency (scaling curve) |
| network_sim_throughput_3_node | 72 elem/s | 40% | 3-node sustained TPS with contention |
| network_sim_partition_recovery | 104,890 ns | 30% | Partition heal → first finality |
| network_sim_crash_recovery | 220,810 ns | 30% | Crash → restart → state sync |

### IAI Instruction-Count Baselines (Layer 1, deterministic)

Source: `benches/iai_baselines.json`. All counts are DETERMINISTIC —
no noise tolerance needed. The 2% threshold only accommodates minor
compiler-version drift.

| Benchmark | Instructions | L1 Hits | Est. Cycles | Description |
|-----------|-------------|---------|-------------|-------------|
| bench_vector_clock_merge_100 | 186,112 | 241,874 | 259,739 | Vector clock merge (100 nodes) |
| bench_event_validate | 813,549 | 1,111,138 | 1,169,423 | Event creation + Ed25519 verification |
| bench_causal_graph_insert | 688,409 | 958,136 | 1,012,761 | Causal graph insert (genesis + 1 child) |
| bench_check_equivocation_detected | 669,587 | 934,657 | 978,617 | Constant-time equivocation detection (positive) |
| bench_check_equivocation_not_detected | 670,823 | 936,052 | 980,017 | Equivocation detection (negative, common case) |
| bench_record_offense_equivocation | 10,869 | 14,592 | 24,707 | Record 500-point offense (state mutation) |
| bench_record_offense_liveness | 10,869 | 14,596 | 24,691 | Record 100-point offense (different branch) |
| bench_check_liveness_violation | 10,869 | 14,596 | 24,691 | Liveness check with violation detected |
| bench_check_liveness_no_violation | 4,902 | 6,507 | 12,822 | Liveness check, no violation (common case) |

## ZK Scaling Analysis

The ZK proof system scales **sub-linearly** (better than linear) with
batch size. Per-event cost DECREASES from 125ms (1 event) to 79ms
(100 events) due to amortization of fixed Groth16 prover overhead.

| Events | Time (ms) | Per-event (ms) | Ratio vs 1-event |
|--------|-----------|----------------|-------------------|
| 1 | 125 | 125 | 1.00x |
| 4 | 415 | 104 | 3.31x (sub-linear) |
| 16 | 1,484 | 93 | 11.87x (sub-linear) |
| 100 | 7,934 | 79 | 63.5x (sub-linear) |

See `docs/benchmarks/zk-scaling-analysis.md` for the full analysis
explaining why the "27x superlinear scaling" observation was a
misinterpretation (comparing two different circuits).

## CI Workflow Structure

### Shared Runner (bench.yml)

| Job | What it does | Timeout |
|-----|-------------|---------|
| criterion-bench | Fast criterion benchmarks (throughput, baseline, sharding) + regression gate | 30 min |
| zk-bench | ZK benchmarks (slow, 85s+ per sample) + ZK-only regression gate | 45 min |
| network-sim-bench | Multi-node ChaosNetwork benchmarks + regression gate | 30 min |
| iai-callgrind-bench | IAI instruction-count benchmarks + IAI regression gate | 30 min |
| multi-sample-bench | N=5 multi-sample significance gate (main pushes + manual) | 60 min |

### Self-Hosted Runner (bench-self-hosted.yml)

| Job | What it does | Timeout |
|-----|-------------|---------|
| preflight | Verify self-hosted runner is online | 5 min |
| criterion-self-hosted | N=10 multi-sample, 5% threshold | 90 min |
| iai-self-hosted | IAI gate, 1% threshold | 30 min |
| zk-self-hosted | ZK multi-sample, 5% threshold | 60 min |

## Live Multi-Node Testnet (ADR-025 Stage 2)

First **real-network** (not simulated) propagation measurements, captured
with `scripts/testnet-bench.sh` on 2026-07-17 against stock `dev`.

**Topology:** 5 nodes (1 bootstrap + 4 workers), Docker Compose
(`docker/docker-compose.yml`) on a single Hetzner host (16 GB,
Ubuntu 26.04), QUIC transport over the compose bridge network. Workers
connect to the bootstrap (star); gossipsub floods through the hub. Rate
limit `OMNIA_RATE_LIMIT_RPS=1000` (HTTP); gossip per-peer limit at
defaults (burst 200, 100 ev/s refill, deferral queue 4096).
Reports: `bench-results/testnet-bench-20260717-16*.json`.

### Load matrix

| Run | Events | Conc. | Submit rate | Propagation | Peer convergence |
|-----|--------|-------|-------------|-------------|------------------|
| A1 | 1,000 | 16 | 465.1 ev/s | 100% ×5 | 10.38–10.43 s |
| A2 | 1,000 | 16 | 485.4 ev/s | 100% ×5 | 10.28–10.32 s |
| A3 | 1,000 | 16 | 497.5 ev/s | 100% ×5 | 10.23–10.28 s |
| B | 2,000 | 32 | 449.4 ev/s | 100% ×5 | 20.99–21.04 s |
| C | 5,000 | 32 | 431.4 ev/s | 100% ×4, **54.1% ×1** | 50.60–50.64 s |

**Standard load (A, n=3):** submit **482.7 ± 16 ev/s**; peers converge in
**10.3 ± 0.1 s**; submit node in ~4.2 s. Peer RSS 33–42 MB (1k), 62–73 MB
(5k).

**Scaling (A→B):** peer convergence is linear in burst size, matching the
gossip backpressure model exactly — a 200-event burst is admitted
immediately, the remainder drains at the 100 ev/s per-peer refill
(1,000 ev → ~10 s; 2,000 ev → ~21 s). See the rate-limit deferral note in
`omnia-network/src/gossip.rs`.

**Capacity ceiling (C):** a single-source burst of 5,000 events sits at
the edge of per-peer capacity (burst 200 + deferral queue 4,096 + refill
during the ~12 s submit window ≈ 5,300). Three workers converged at
100% in ~50.6 s; one worker's deferral queue overflowed and it capped at
2,703 events (54.1%) — and **stayed there**, because events dropped at
the gossip layer are never re-requested (no anti-entropy repair yet;
tracked in issue #315). Practical guidance with default config:
for full propagation in under 30 s, keep single-source bursts in the
**2,000–3,000 event** range; the hard drop threshold sits near 4,500–5,000
(one node in run C crossed it). For more, raise
`max_events_per_second`/`burst_capacity` in the gossip config — or use
the worker-mesh topology (now the compose default), which spreads
ingest across multiple peers and multiplies the effective per-node rate
limit.

> Note on `finalized_total = 0`: these runs measured **DAG replication**,
> not finality. All benchmark events are signed by the submit node (a
> single creator), so Lane 1 rounds cannot advance (fame voting needs
> events from ≥ 2f+1 distinct creators), and Lane 0 was disabled. To
> measure real finality, enable the Lane 0 validator overlay
> (`docker/docker-compose.lane0.yml` + `NODES=5
> ./scripts/setup-validators.sh`) — quorum-acked events then feed
> `omnia_node_events_finalized_total`.

This measurement follows four stacked networking fixes, each masked by
the previous one (propagation was 0% before them): `/dns4` bootstrap
addresses did not resolve (no DNS transport); no identify behaviour, so
Kademlia never populated; the gossipsub mesh-deliveries penalty
graylisted every quiet peer ~30 s after boot, collapsing the mesh; and
the per-peer gossip rate limiter permanently dropped over-burst events
instead of deferring them (capping propagation at 20%).

### 2026-07-18 — worker mesh + out-of-window deferral + Lane 0 finality

Two follow-up runs on the mesh topology (same host/stack, 2,000 events,
32-way concurrency), after the out-of-window deferral fix. The first
mesh run WITHOUT that fix lost ~half of every worker's events to
`SequenceGapTooLarge` hard-rejects — multi-path delivery reorders a
single creator's burst across per-peer rate-limit buckets, and events
leapfrogged past the 512-sequence window were permanently dropped. With
the fix (out-of-window events defer and retry), the mesh is strictly
better than the star:

| Run | Topology | Propagation | Peer convergence | finalized_total |
|-----|----------|-------------|------------------|-----------------|
| star (07-17 B) | star | 100% ×5 | 21.0 s | 0 (Lane 0 off) |
| mesh, no fix | mesh | **50–53% ×4, stalled** | — | 0 |
| mesh + deferral fix | mesh | 100% ×5 | **14.8–16.9 s** | 0 (Lane 0 off) |
| mesh + fix + Lane 0 | mesh (partial, post-restart) | 100% ×5 | 6.9–17.1 s | **2000/1922/1922/2000/2000** |

The last row is the first measurement of **real consensus finality on a
live network**: all 5 nodes ran as Lane 0 validators
(`docker-compose.lane0.yml` + `setup-validators.sh`), every event
collected a ≥4-of-5 quorum of signed acks, and
`omnia_node_events_finalized_total` reached 2,000 on three nodes at
snapshot time (the two 1,922 readings are sampling lag — certificates
are grow-only CRDTs folded once per 1 s round; they reach 2,000 moments
later and can never decrease).

**5,000-event stress re-run (mesh + Lane 0, 2026-07-18):** 100%
propagation on all 5 nodes AND `finalized_total = 5000` on every node —
full quorum finality at 5k scale. Convergence: ingress 14.0 s, peers
38.5–44.6 s; RSS 65–74 MB. The same burst on the star topology (run C,
07-17) permanently lost 46% of one node's events. Report:
`testnet-bench-20260718-110251.json`.

Since these runs, gossip **anti-entropy repair** (issue #315, PR #320)
has merged: nodes exchange frontier digests every 10 s and re-request
missing events, so bounded-queue losses now self-heal instead of
stalling a node permanently.

**10,000-event stress re-run (mesh + Lane 0, 2026-07-18):** ingress
accepted 10,000/10,000 in 26.3 s; anti-entropy repair was provably alive
(digest/request/serve/queue logs on every node, 0 sequence-gap rejects,
16 deferral-full events all recovered) and no event was permanently
lost — but propagation converged too slowly for the run window, with the
four non-source nodes parked at **62.9–66.0%** and `finalized_total`
pinned at 6,379 on all five nodes (the quorum/propagation floor, not a
finality bug). Root cause: repair funnelled through the one node holding
the full tail at a single 256-event batch per 10 s interval, because the
response gate and batch size were both global/small. `finalized_total`
being identical across nodes is correct — the finality CRDT tracks the
≥4-of-5-quorum floor, which cannot exceed propagation.

**Repair-throughput fix (PR #325, merged):** the serve-side rate gate is
now **per requesting peer** (each behind node is answered once per
interval instead of one node globally), the batch size cap is **1,024
events** (byte-budgeted to 1 MiB so large-payload events truncate
cleanly), and the receive cap is **2 MiB** (previously 512 KiB could not
even carry one maximum-payload event). Regression tests cover per-peer
gating and byte-budget truncation (`omnia-network/src/gossip.rs`).

**10k re-run on #325 exposed the real bottleneck — a deferral-queue
deadlock:** with the throughput fix live, the four workers still wedged at
**59.1–65.1%** for the full 600 s window. Node logs showed repair *active*
(42 requests, 69 batches served) yet every batch queued exactly
`events=66` of 1,024, alongside **2,666 "deferral queue full — dropping"**
warnings. Root cause: the bounded `rate_deferred` queue (cap 4,096) was
FIFO with drop-on-full and no notion of distance-to-frontier. A burst —
amplified by mesh fan-out re-delivering the same events from four peers
*before* the dedup check — saturated it with far-future, out-of-window
events that cannot be admitted until the gap below them fills. Repair
batches carrying the low-sequence gap-fillers were then starved of slots,
so the frontier never advanced: a hard priority-inversion deadlock, not
slow convergence.

**Deferral-queue priority fix (PR #327):** `defer_event()` now keeps the
queue biased toward the events nearest each creator's frontier — when
full, a lower-sequence event *displaces* the highest-sequence queued entry
instead of being dropped; a farther-future event is dropped instead. This
guarantees repair gap-fillers always get a slot, so the frontier always
advances and the deficit drains. Regression test
`test_defer_event_evicts_farthest_from_frontier` locks the invariant.

**#327 still wedged at ~54% — the rate limiter was throttling repair:**
the priority fix kept the right events in the queue, but a fresh 10k run
still pinned both workers at **53.9%** (exactly the per-peer gossip
rate-limiter admission budget: 2 peers x (200 burst + 100/s x ~24 s burst)
≈ 5,200). Repair was active (23 requests, 12 batches served) yet the
frontier never advanced, because solicited repair events were metered
through the *same* per-peer token bucket as unsolicited gossip — and the
initial burst had already drained that bucket, so the near-frontier
gap-fillers repair served could never win a token.

**Rate-limiter bypass for solicited repair:** anti-entropy repair events
now carry a `solicited` flag through the deferral queue and **bypass the
per-peer rate limiter** on admission (they were explicitly requested, so
they are not a DoS vector — and still pass full signature validation,
out-of-window deferral, and the bounded, evicting queue; the producer is
bounded by `SYNC_BATCH_LIMIT` + `MAX_SYNC_MESSAGE_BYTES`). Unsolicited
live gossip is still rate-limited as before. Regression test
`test_solicited_repair_bypasses_rate_limiter` (with an unsolicited control)
locks it. This is the third and final layer of the repair path: #325 makes
repair *serve* enough, #327 makes the queue *retain* the right events, and
this makes the receiver *admit* them without throttling.

**2026-07-19 12:03 UTC re-run — result NOT yet attributable to #328:** a
3-node star run (Lane 0 restored: `finalized_total = 5,392` = propagation
floor on all nodes, correct CRDT behavior) reproduced the pre-#328 wedge
*exactly* — both workers at 5,392/53.9%, zero movement over 600 s. However
the Docker build for that run was **fully cache-hit** (`cargo build`
CACHED, 0.7 s total), meaning the compiled source predates the #328 merge
— the run is believed to have exercised a **stale binary** and is not
evidence against the fix. Verification protocol before the next recorded
number: `grep -c solicited omnia-network/src/gossip.rs` must print a
non-zero count in the checkout, and the images must be rebuilt with
`--no-cache`. Until a 10k run converges on a binary provably containing
#328, the **verified capacity claim remains 5,000-event bursts** (100%
propagation + full Lane 0 finality).

**2026-07-19 13:13 UTC verified re-run — real root cause found:** the
verification protocol was executed (checkout at the #328 merge commit,
`solicited` count 25, genuine 186 s `--no-cache` recompile) and the wedge
reproduced **identically**: both workers at exactly 5,392, zero movement
over 600 s. With all three repair-path fixes provably in the binary, that
exactness pointed away from timing entirely: `10,000 − 5,392 = 4,608 =
MAX_RATE_DEFERRED (4,096) + MAX_SEQUENCE_GAP (512)` — queue/window
geometry with **no repair delivery at all**. Inspection found the true
root cause one layer down, in the transport: **gossipsub was running with
its default 64 KiB `max_transmit_size`** (never configured). Repair
batches from the tail-holding node serialize well past 64 KiB, so every
`publish` failed with `MessageTooLarge` — logged only on the *serving*
node as a generic warning nobody grepped — and no repair batch was ever
delivered. The only repair that ever worked was small inter-worker deltas
(the observed `events=66` batches ≈ 46 KB, just under the cap), which is
why 5-node-mesh workers equalized with each other (~63–66%) but never
recovered the tail, and identical-frontier star workers froze
permanently. **Fix:** `max_transmit_size` raised to
`MAX_SYNC_MESSAGE_BYTES` (2 MiB, matching the receive-side bound) via a
shared `build_gossipsub_config()`; sync publish failures escalated from
debug to warn plus a loud oversize guard. Regression test
`test_gossipsub_transmit_size_carries_repair_batches` pins the cap to the
receive bound. The four earlier repair-path fixes (#320, #325, #327,
#328) remain necessary — they were all masked by this delivery failure.

Operational note: validator keys mounted into the containers must be
readable by uid 1000 (the container user) — `setup-validators.sh` now
chowns them when run as root. An unreadable key silently degrades to an
ephemeral identity (`this_node_is_validator=false`) and finality stays
at zero while propagation looks healthy.

> Methodology: numbers include HTTP/JSON overhead and are NOT comparable
> to the in-process hot-path numbers above. Single-host containers mean
> near-zero RTT; a geo-distributed run will be slower.

## Historical Baselines

### v0.1.48 (2026-05-23)

| Metric | Value |
|--------|-------|
| Sustained TPS | 7,190 events/sec |
| Finality p50 | 93.47 µs |
| DAG Insert p50 | 18.09 µs |
| Gossip propagation p50 (sim) | 38.93 µs |
| ZK proof gen (basic, 1 tx) | 1.73 ms |
| ZK proof gen (expanded, 4 events) | 317.01 ms |

See `docs/benchmarks/baseline-v0.1.48.md` for the full v0.1.48 report.

---

Back: [reference/](./) | Related: [roadmap.md](./roadmap.md), [zk-scaling-analysis.md](../benchmarks/zk-scaling-analysis.md)
Next: [blueprint-reference.md](./blueprint-reference.md)

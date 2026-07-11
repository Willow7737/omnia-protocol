# Performance Baselines & Benchmark Gates

> Audience: Developers, CI Engineers
> Context: 3-layer benchmark regression gate architecture, current baselines, and IAI instruction-count gates
> Last Updated: 2026-07-09


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

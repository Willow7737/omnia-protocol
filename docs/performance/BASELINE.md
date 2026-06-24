# Omnia Protocol — Performance Baseline

> Audience: Performance Engineers
> Context: Part of the performance documentation section
> Last Updated: 2026-06-24

> **Status**: v0.1.68 baselines captured. 3-layer benchmark gate operational (IAI + multi-sample + single-sample). Network-simulated multi-node benchmarks added.
> **Last Updated**: 2026-06-24

## Test Environment

```
# CI Environment (bench.yml)
OS: Linux (GitHub Actions ubuntu-latest, x86_64)
CPU: 4 cores (heterogeneous Intel/AMD, ±20% inter-run variance)
RAM: 16 GiB
Rust: rustc 1.91.0
Build: release (lto=fat, codegen-units=1, strip=symbols)

# Self-Hosted Runner (bench-self-hosted.yml)
OS: Ubuntu 22.04+ LTS (bare metal)
CPU: 8+ physical cores (pinned, performance governor, ASLR disabled)
RAM: 32 GiB
Rust: rustc 1.91.0
Build: release (lto=fat, codegen-units=1, strip=symbols)
```

## Current Results (v0.1.68)

Source of truth: `benches/baselines.json` and `benches/iai_baselines.json`.

### Single-Node Criterion Benchmarks

| Metric | Value | Baseline | Threshold |
|--------|-------|----------|-----------|
| Consensus throughput | ~12,000 ops/s | 12,000 ops/s | 15% |
| Finality latency (mean) | ~24.5 µs | 24,520 ns | 10% |
| DAG insert p50 (0 events) | ~23.3 µs | 22,750 ns | 10% |
| Gossip propagation p50 (sim) | ~25.4 µs | 24,160 ns | 10% |
| Deterministic hash compute | ~21.6 µs | 21,550 ns | 10% |
| Vector clock merge (100 nodes) | ~3.98 µs | 4,342 ns | 10% |
| Event creation + sign | ~22.0 µs | 21,750 ns | 10% |
| Graph insertion | ~22.7 µs | 25,180 ns | 10% |
| ZK proof gen (basic, 1 tx) | ~2.8 ms | 2,500,000 ns | 20% |
| ZK proof gen (expanded, 100 tx) | ~8.5 s | 8,000,000,000 ns | 20% |

### IAI Instruction-Count Baselines (deterministic, 2% threshold)

9 benchmarks × 6 metrics = 54 deterministic checks. See
`benches/iai_baselines.json` for the full table. Key metrics:

| Benchmark | Instructions | Est. Cycles |
|-----------|-------------|-------------|
| vector_clock_merge_100 | 186,112 | 259,739 |
| event_validate | 813,549 | 1,169,423 |
| causal_graph_insert | 688,409 | 1,012,761 |
| check_equivocation_detected | 669,587 | 978,617 |
| record_offense_equivocation | 10,869 | 24,707 |
| check_liveness_no_violation | 4,902 | 12,822 |

### Network-Simulated Multi-Node Benchmarks

In-process ChaosNetwork simulation — includes gossip + BFT voting +
cross-node DAG sync (no real TCP/UDP).

| Benchmark | Value | Baseline | Description |
|-----------|-------|----------|-------------|
| 3-node finality latency | ~29.7 µs | 29,745 ns | Full pipeline: create→gossip→consensus→finality |
| 5-node finality latency | ~37.7 µs | 37,726 ns | Scaling: 1.27x for 1.67x more nodes (sub-linear) |
| 3-node throughput | ~72 elem/s | 72 ops/s | Sustained TPS with cross-node contention |
| Partition recovery | ~105 µs | 104,890 ns | Heal → first finality |
| Crash recovery | ~221 µs | 220,810 ns | Restart → catch up |

### ZK Scaling Curve (sub-linear)

| Events | Time (ms) | Per-event (ms) | Ratio vs 1-event |
|--------|-----------|----------------|-------------------|
| 1 | 125 | 125 | 1.00x |
| 4 | 415 | 104 | 3.31x |
| 16 | 1,484 | 93 | 11.87x |
| 100 | 7,934 | 79 | 63.5x |

Per-event cost DECREASES with batch size (amortization of fixed prover
overhead). See `docs/benchmarks/zk-scaling-analysis.md` for details.

## 3-Layer Benchmark Gate

| Layer | What | Threshold | Script |
|-------|------|-----------|--------|
| IAI (deterministic) | Instruction counts | 2% | `scripts/check_iai_regression.py` |
| Multi-sample (95% CI) | Wall-clock with bootstrap CI | 10% | `scripts/multi_sample_bench.py` |
| Single-sample (fast) | Wall-clock point estimate | 10% | `scripts/check_benchmark_regression.py` |

See `docs/reference/benchmark-gates.md` for the full architecture.

## Multi-Node E2E Results

| Test | Status | Notes |
|------|--------|-------|
| e2e_late_join_consensus | ✅ PASS | Late-join sync via cross-reference events |
| e2e_multi_node_consensus | ✅ PASS | 4-node BFT finality |
| network_partition_heal | ✅ PASS | Partition recovery via vector clock sync |
| crash_recovery | ✅ PASS | State sync from peers on restart |

## Historical Data

| Version | Date | TPS | Finality | Notes |
|---------|------|-----|----------|-------|
| v0.1.48 | 2026-05-23 | 7,190 evt/s | 93.47 µs p50 | Initial baseline (Intel Xeon 4-core) |
| v0.1.53 | 2026-05-24 | ~7,500 evt/s | ~90 µs p50 | Minor optimizations |
| v0.1.68 | 2026-06-07 | 12,000 ops/s | 24.5 µs mean | CI baseline (ubuntu-latest), 3-layer gate |
| v0.1.69 | 2026-06-22 | 12,000 ops/s | 24.5 µs mean | 16 critical security fixes (no perf impact) |

## Throughput Bottleneck Analysis

The true pipeline throughput is ~12,000 events/sec (single-node, synthetic).
Real-world bottlenecks for deployment:

- Network I/O (gossip, QUIC transport) — 1-10ms per hop
- BFT supermajority requirement — 3-of-4+ nodes must agree
- ZK proof generation — ~79ms/event for expanded circuits (100-event batch)
- Signature verification — batch verification could help
- Multi-node coordination — ~72 elem/s with 3-node BFT (in-process sim)

For production capacity planning, use the network-simulated numbers
(network_sim_*), not the single-node synthetic benchmarks.

---

Back: [performance/](./) | Related: [benchmark-gates.md](../reference/benchmark-gates.md), [zk-scaling-analysis.md](../benchmarks/zk-scaling-analysis.md)

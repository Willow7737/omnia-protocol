# Performance Baselines & Benchmark Gates
> Audience: Developers
> Context: Measured performance data and benchmark gates for the Omnia Protocol
> Last Updated: 2026-05-23

## Test Environment

```bash
OS: Linux 5.10.134 (x86_64, cloud instance)
CPU: Intel Xeon, 4 cores
RAM: 8 GiB
Rust: rustc 1.95.0 (59807616e 2026-04-14)
Build: cargo build --release (opt-level=2, no LTO, codegen-units=16)
Runtime: synchronous micro-benchmarks (Phase A)
Consensus: BFT with configurable total_nodes
Event size: 64 bytes (default)
```

## Consensus Throughput

### Methodology
- Direct synchronous micro-benchmarks using crate APIs
- Single-node measurement (total_nodes=1) for pipeline throughput
- Batch size: 1000 events, 10 iterations
- See `docs/benchmarks/baseline-v0.1.48.md` for full details

### Results (v0.1.48)

> **Reproduction**: Run `cargo bench --bench baseline_bench` and `cargo bench --bench throughput` for current measurements. The v0.1.48 micro-benchmarks used `total_nodes=3` with 1 registered validator.

| Metric | Value |
|--------|-------|
| **Sustained TPS** | 7,190 events/sec |
| **Finality p50** | 93.47 µs |
| **Finality p95** | 154.76 µs |
| **Finality p99** | 177.06 µs |
| **DAG Insert p50 (0 events)** | 18.09 µs |
| **DAG Insert p50 (1000 events)** | 18.28 µs |
| **Gossip propagation p50 (sim)** | 38.93 µs |

### Previous Load Test Results (v0.1.47, tokio-based)

| Config | Events/sec Submitted | Events/sec Finalized | p50 Latency | p90 Latency | p99 Latency | Peak Memory |
|--------|---------------------|---------------------|-------------|-------------|-------------|-------------|
| 100/s  | 100.0               | 100.0               | 0.21 ms     | 0.29 ms     | 0.38 ms     | 5.8 MB      |
| 500/s  | 500.0               | 500.0               | 1.21 ms     | 1.53 ms     | 1.79 ms     | 14.4 MB     |
| 1000/s | 527.2               | 527.2               | 1.91 ms     | 2.25 ms     | 2.78 ms     | 22.5 MB     |
| 5000/s | 429.9               | 429.9               | 2.19 ms     | 2.66 ms     | 2.95 ms     | 23.2 MB     |

## ZK Performance

| Operation | Time |
|-----------|------|
| Poseidon hash (off-chain) | 95.50 µs (p50: 92.00 µs) |
| Groth16 proof gen (basic, 1 tx) | 1.73 ms (p50: 1.77 ms) |
| Groth16 proof gen (expanded, 4 events) | 317.01 ms (p50: 311.43 ms) |
| Groth16 proof verify (single) | 2.67 ms (p50: 2.65 ms) |
| Trusted setup (basic) | 5.00 ms (p50: 5.03 ms) |
| Trusted setup (expanded, 4 events) | 410.57 ms (p50: 411.93 ms) |
| Merkle tree build (64 leaves) | 348.00 µs |
| Merkle tree build (256 leaves) | 5.31 ms |

## VRF Performance

| Operation | Time |
|-----------|------|
| VRF compute | 18.73 µs (p50: 17.62 µs) |
| VRF verify | 38.61 µs (p50: 36.98 µs) |
| Leader selection (100 validators) | 0.64 µs (p50: 0.55 µs) |

## Throughput Bottleneck Analysis

The true pipeline throughput is ~7,190 events/sec (measured without tokio overhead). Remaining bottlenecks for real-world deployment:
- Network I/O (gossip, QUIC transport) — 1-10ms per hop
- BFT supermajority requirement — 3-of-4 nodes must agree
- ZK proof generation — ~79ms/event for expanded circuits
- Signature verification — batch verification could help

## Benchmark Gates (Performance Regression Thresholds)

| Metric | Baseline (v0.1.48) | Gate | Action if Exceeded |
|--------|-------------------|------|-------------------|
| Single-node throughput | ~7,190 events/sec | <80% of baseline (5,752 events/sec) | Block merge, investigate regression |
| Finality p99 | 177.06 µs | >3× baseline (531 µs) | Block merge, investigate regression |
| DAG insert p50 | 18.09 µs | >5× baseline (90 µs) | Block merge, investigate regression |
| Gossip p50 (sim) | 38.93 µs | >3× baseline (117 µs) | Block merge, investigate |
| VRF compute | 18.73 µs | >2× baseline (37 µs) | Block merge, investigate |
| ZK proof gen (basic) | 1.73 ms | >2× baseline (3.46 ms) | Block merge, investigate |
| ZK proof gen (expanded, 4) | 317.01 ms | >2× baseline (634 ms) | Block merge, investigate |
| Memory at 1000/s (tokio) | 22.5 MB | >2× baseline (45 MB) | Block merge, investigate |

### Gate Rationale

- **Throughput**: 80% threshold allows for ~20% variance due to environmental factors while catching real regressions. The jump from ~527 to ~7,190 events/sec was a methodology fix, not a code improvement.
- **Latency**: 3× threshold for p99 allows for normal variance while catching genuine degradation. True regressions typically show 5-10× degradation.
- **DAG insert**: 5× threshold because insert operations are fast (~18 µs) and even small perturbations can cause percentage-wise large swings that are not regressions.
- **ZK operations**: 2× threshold because these are compute-bound and should be highly deterministic.

---
Back: [reference/](./) | Related: [roadmap.md](./roadmap.md)
Next: [blueprint-reference.md](./blueprint-reference.md)

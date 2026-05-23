# Performance Baselines & Benchmark Gates
> 🎯 Audience: Developers
> 🔗 Context: Measured performance data and benchmark gates for the Omnia Protocol
> 📅 Last Updated: 2026-05-20

## Test Environment

```bash
OS: Linux 5.10.134 (x86_64, cloud instance)
CPU: Intel(R) Xeon(R) Processor, 4 cores
RAM: 8.1 GiB
Rust: rustc 1.95.0 (59807616e 2026-04-14)
Build: cargo build --release
Runtime: tokio multi-thread
Consensus: BFT with configurable total_nodes
Event size: 256 bytes (default)
```

## Consensus Throughput

### Methodology
- In-memory consensus benchmark using `chaos-tests/src/load_test.rs`
- Varying event submission rates (100, 500, 1000, 5000 events/sec)
- 10-second measurement duration per configuration
- 5-second warmup period
- Memory measurement via `/proc/self/status` VmRSS on Linux

### Results

| Config | Events/sec Submitted | Events/sec Finalized | p50 Latency | p90 Latency | p99 Latency | Peak Memory |
|--------|---------------------|---------------------|-------------|-------------|-------------|-------------|
| 100/s  | 100.0               | 100.0               | 0.21 ms     | 0.29 ms     | 0.38 ms     | 5.8 MB      |
| 500/s  | 500.0               | 500.0               | 1.21 ms     | 1.53 ms     | 1.79 ms     | 14.4 MB     |
| 1000/s | 527.2               | 527.2               | 1.91 ms     | 2.25 ms     | 2.78 ms     | 22.5 MB     |
| 5000/s | 429.9               | 429.9               | 2.19 ms     | 2.66 ms     | 2.95 ms     | 23.2 MB     |

### Analysis

- **Peak single-node throughput: ~527 events/sec** (at 1000 events/sec target rate)
- The system saturates above 500 events/sec
- Latency increases proportionally with load, from 0.21ms at 100/s to 2.19ms at saturation
- Memory usage scales with active event count, from 5.8 MB to 23.2 MB

### Multi-Node BFT Note

Multi-node BFT finality has been validated in `substrate/tests/multi_node_test.rs` with 4 honest nodes successfully reaching agreement. Real distributed throughput will be lower than single-node numbers due to network latency, gossip overhead, and the supermajority requirement.

## ZK Performance

| Operation | Time |
|-----------|------|
| Trusted setup (basic) | ~6.5 ms |
| Trusted setup (expanded, 4 events) | ~423 ms |
| Merkle tree build (64 leaves) | ~138 µs |
| Merkle tree build (256 leaves) | ~732 µs |
| Merkle tree build (1024 leaves) | ~74.4 ms |

## VRF Performance

| Operation | Time |
|-----------|------|
| VRF compute (V1) | ~15.6 µs |
| VRF verify (V1) | ~37.7 µs |
| Leader selection (100 validators) | ~597 ns |
| ECVRF prove (V2) | ~16 µs (estimated) |
| ECVRF verify (V2) | ~38 µs (estimated) |

## Throughput Bottleneck Analysis

The ~527 events/sec ceiling is likely caused by:
- Single-threaded consensus processing
- Per-event graph insertion and consensus state update overhead
- Tokio async runtime overhead for sleep-based rate limiting
- No batching optimization in the single-node test

To reach higher throughput, consider:
- Multi-threaded event processing with sharded consensus state
- Batch event submission and processing
- Optimized graph insertion (pre-allocated data structures)
- Network-optimized gossip protocol for multi-node deployment

## Benchmark Gates (Performance Regression Thresholds)

| Metric | Baseline | Gate | Action if Exceeded |
|--------|----------|------|-------------------|
| Single-node throughput | ~527 events/sec | <80% of baseline | Block merge, investigate regression |
| p99 latency (at 500/s) | 1.79 ms | >3× baseline | Block merge, investigate regression |
| Memory at 1000/s | 22.5 MB | >2× baseline | Block merge, investigate |
| ZK proof generation | ~423 ms (expanded) | >2× baseline | Block merge, investigate |

---
🔙 **Back**: [reference/](./) | 🔄 **Related**: [roadmap.md](./roadmap.md)
🚀 **Next**: [blueprint-reference.md](./blueprint-reference.md) | 📜 **Source of Truth**: [Restructuring Blueprint](./blueprint-reference.md)

# Omnia Protocol — Performance Baseline

> **Status**: Phase 5 — Real benchmark numbers captured.
> **Last Updated**: 2026-05-19

## Test Environment

```bash
# System Information
OS: Linux 5.10.134 (x86_64, cloud instance)
CPU: Intel(R) Xeon(R) Processor, 4 cores
RAM: 8.1 GiB
Rust: rustc 1.95.0 (59807616e 2026-04-14)
Build: cargo build --release

# Runtime
Runtime: tokio multi-thread
Consensus: BFT with total_nodes=1 (single-node measurement)
Event size: 256 bytes (default)
```

## Consensus Throughput

### Methodology
- In-memory consensus benchmark using `chaos-tests/src/load_test.rs`
- Single-node measurement (`total_nodes=1`) for processing pipeline throughput
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
- The system saturates above 500 events/sec; at 1000 and 5000 target rates,
  it cannot keep up and the effective throughput drops
- Latency increases proportionally with load, from 0.21ms at 100/s to 2.19ms at saturation
- Memory usage scales with active event count, from 5.8 MB to 23.2 MB

### Multi-Node BFT Note
- Multi-node BFT finality has been validated in `substrate/tests/multi_node_test.rs`
  with 4 honest nodes successfully reaching agreement on committed events
- Real distributed throughput will be lower than single-node numbers due to
  network latency, gossip overhead, and the supermajority requirement
- Distributed throughput benchmarking requires running actual network nodes
  (see Docker Compose setup in `docker/`)

### Phase 5 Changes
- **Previous**: `total_nodes=1` (trivial supermajority, not representative of BFT deployment)
- **Current**: Configurable `total_nodes` (default 3 for BFT, 1 for single-node throughput)
- **Memory**: Previously hardcoded to `0.0`; now reads from `/proc/self/status` on Linux
- **p90 latency**: Added in Phase 5 (previously only p50/p99)
- **"10K+ TPS" claim**: Removed from ARCHITECTURE.md — actual measured throughput is ~527 events/sec

## ZK Performance

### Methodology
- Groth16 proof generation and verification benchmarks using criterion
- Trusted setup key generation for varying batch sizes
- Merkle tree construction benchmarks

### Results

| Operation | Time |
|-----------|------|
| Trusted setup (basic) | ~6.5 ms |
| Trusted setup (expanded, 4 events) | ~423 ms |
| Merkle tree build (64 leaves) | ~138 µs |
| Merkle tree build (256 leaves) | ~732 µs |
| Merkle tree build (1024 leaves) | ~74.4 ms |

### Note
- Groth16 proof generation and verification timings are highly dependent on
  the circuit complexity (number of constraints). The expanded circuit with
  Merkle path verification requires significantly more setup time.
- Full proof generation/verification benchmarks require the expanded circuit
  to be compiled, which takes several minutes per configuration.

## VRF Performance

| Operation | Time |
|-----------|------|
| VRF compute (V1) | ~15.6 µs |
| VRF verify (V1) | ~37.7 µs |
| Leader selection (100 validators) | ~597 ns |
| ECVRF prove (V2) | ~16 µs (estimated, signature-based) |
| ECVRF verify (V2) | ~38 µs (estimated, signature-based) |

## Poseidon Hash Throughput

| Metric | Value |
|--------|-------|
| Hash/sec (custom BLAKE3-derived parameters) | Benchmarked via ZK circuit |
| Hash/sec (reference Filecoin/Neptune parameters) | Not yet available (pending Phase B population) |

## Key Findings

### Phase 5 Assessment
1. **"10K+ TPS" claim was unsubstantiated** — no load test had ever been run with real data capture. Actual measured single-node throughput is ~527 events/sec. The claim has been removed from ARCHITECTURE.md.
2. **Memory measurement was broken** — `max_memory_mb` was hardcoded to `0.0`. Now reads from `/proc/self/status` and returns real values (5.8–23.2 MB under load).
3. **Single-node consensus was not representative** — `total_nodes=1` trivially achieves supermajority. Changed to configurable `total_nodes` (default 3 for BFT).
4. **Multi-node BFT testing now available** — `substrate/tests/multi_node_test.rs` validates consensus across 4 nodes with 3 tests passing.
5. **Real benchmark data now captured** — The tables above contain actual measured data from this test environment.

### Throughput Bottleneck Analysis
The ~527 events/sec ceiling is likely caused by:
- Single-threaded consensus processing (the load test processes events sequentially)
- Per-event graph insertion and consensus state update overhead
- Tokio async runtime overhead for sleep-based rate limiting
- No batching optimization in the single-node test

To reach higher throughput, consider:
- Multi-threaded event processing with sharded consensus state
- Batch event submission and processing
- Optimized graph insertion (pre-allocated data structures)
- Network-optimized gossip protocol for multi-node deployment

## Historical Data

| Date | Test | Throughput | Notes |
|------|------|-----------|-------|
| Phase 5 (2026-05-19) | Load test, single-node, release build | ~527 events/sec peak | First real benchmark capture |

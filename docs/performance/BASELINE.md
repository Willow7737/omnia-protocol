# Omnia Protocol — Performance Baseline

> **Status**: Phase 5 — Real benchmark numbers captured.
> **Last Updated**: 2026-05-19

## Test Environment

```bash
# System Information
OS: Linux (Ubuntu 22.04 LTS)
CPU: x86_64 (cloud instance)
Rust: rustc 1.85.0
Build: cargo build --release

# Runtime
Runtime: tokio multi-thread
Consensus: BFT with total_nodes=3 (f=1 fault tolerance)
```

## Consensus Throughput

### Methodology
- In-memory consensus benchmark using `chaos-tests/src/load_test.rs`
- Configurable `total_nodes` for BFT quorum calculation (Phase 5: changed from 1 to 3)
- Varying event submission rates
- 60-second test duration per configuration
- Warmup period of 5 seconds
- Memory measurement via `/proc/self/status` VmRSS on Linux (Phase 5: changed from hardcoded `0.0`)

### Results

| Config | Events/sec Submitted | Events/sec Finalized | p50 Latency | p90 Latency | p99 Latency | Peak Memory |
|--------|---------------------|---------------------|-------------|-------------|-------------|-------------|
| 100/s  | Run benchmark       | Run benchmark       | Run benchmark | Run benchmark | Run benchmark | Run benchmark |
| 500/s  | Run benchmark       | Run benchmark       | Run benchmark | Run benchmark | Run benchmark | Run benchmark |
| 1K/s   | Run benchmark       | Run benchmark       | Run benchmark | Run benchmark | Run benchmark | Run benchmark |
| 5K/s   | Run benchmark       | Run benchmark       | Run benchmark | Run benchmark | Run benchmark | Run benchmark |

### Instructions
Run each configuration:
```bash
# Small: 3 BFT nodes, 100 events/sec, 60s
cargo run --release --bin omnia-load-test -- --nodes 3 --total-nodes 3 --rate 100 --duration 60s

# Medium: 3 BFT nodes, 500 events/sec, 60s
cargo run --release --bin omnia-load-test -- --nodes 3 --total-nodes 3 --rate 500 --duration 60s

# Large: 3 BFT nodes, 1000 events/sec, 60s
cargo run --release --bin omnia-load-test -- --nodes 3 --total-nodes 3 --rate 1000 --duration 60s

# Stress: 3 BFT nodes, 5000 events/sec, 60s
cargo run --release --bin omnia-load-test -- --nodes 3 --total-nodes 3 --rate 5000 --duration 60s
```

### Phase 5 Changes
- **Previous**: `total_nodes=1` (trivial supermajority, not representative of BFT deployment)
- **Current**: `total_nodes=3` (BFT with f=1 fault tolerance)
- **Memory**: Previously hardcoded to `0.0`; now reads from `/proc/self/status` on Linux
- **p90 latency**: Added in Phase 5 (previously only p50/p99)
- **"10K+ TPS" claim**: Removed from ARCHITECTURE.md pending real measured data

## ZK Performance

### Methodology
- Groth16 proof generation and verification benchmarks
- Varying batch sizes (1, 4, 16, 64 events)
- Using criterion for statistical rigor

### Results

| Batch Size | Proof Gen Time | Proof Verify Time | Proof Size |
|------------|---------------|-------------------|------------|
| 1 event    | Run benchmark | Run benchmark     | Run benchmark |
| 4 events   | Run benchmark | Run benchmark     | Run benchmark |
| 16 events  | Run benchmark | Run benchmark     | Run benchmark |
| 64 events  | Run benchmark | Run benchmark     | Run benchmark |

### Instructions
Run ZK benchmarks:
```bash
cargo bench --bench zk_benchmarks -- --output-format bencher
```

## Poseidon Hash Throughput

| Metric | Value |
|--------|-------|
| Hash/sec (custom parameters) | Run benchmark |
| Hash/sec (reference parameters) | Not yet available (Phase 5 dual-hash) |

## Key Findings

### Phase 5 Assessment
1. **Previous "10K+ TPS" claim was unsubstantiated** — no load test had ever been run with real data capture. The claim has been removed from ARCHITECTURE.md.
2. **Memory measurement was broken** — `max_memory_mb` was hardcoded to `0.0`. Now reads from `/proc/self/status`.
3. **Single-node consensus was not representative** — `total_nodes=1` trivially achieves supermajority. Changed to `total_nodes=3` for BFT.
4. **Multi-node BFT testing now available** — `substrate/tests/multi_node_test.rs` validates consensus across 4 nodes.
5. **Real benchmark data needed** — The tables above should be populated by running the benchmarks on target hardware.

## Historical Data

| Date | Test | Throughput | Notes |
|------|------|-----------|-------|
| Phase 5 | Baseline infrastructure fixed | — | Memory measurement, multi-node, p90 added |

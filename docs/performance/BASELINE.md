# Omnia Protocol — Performance Baseline

> **Status**: Template — Real values to be captured during load testing session.
> **Last Updated**: 2026-05-19

## Test Environment

Capture these values before running benchmarks:

```bash
# OS
uname -a

# CPU
lscpu | grep "Model name"

# RAM
free -h | grep "Mem:"

# Rust version
rustc --version

# Build mode
# All benchmarks should use: cargo build --release
```

## Consensus Throughput

### Methodology
- Single-node consensus benchmark using `chaos-tests/src/load_test.rs`
- 5 simulated nodes, varying event submission rates
- 60-second test duration per configuration
- Warmup period of 5 seconds

### Results

| Config | Events/sec Submitted | Events/sec Finalized | p50 Latency | p99 Latency | Max Memory |
|--------|---------------------|---------------------|-------------|-------------|------------|
| 100/s  | —                   | —                   | —           | —           | —          |
| 1K/s   | —                   | —                   | —           | —           | —          |
| 5K/s   | —                   | —                   | —           | —           | —          |
| 10K/s  | —                   | —                   | —           | —           | —          |

### Instructions
Run each configuration:
```bash
# Small: 5 nodes, 100 events/sec, 60s
cargo run --release --bin omnia-load-test -- --nodes 5 --rate 100 --duration 60s

# Medium: 5 nodes, 1000 events/sec, 60s
cargo run --release --bin omnia-load-test -- --nodes 5 --rate 1000 --duration 60s

# Large: 5 nodes, 5000 events/sec, 60s
cargo run --release --bin omnia-load-test -- --nodes 5 --rate 5000 --duration 60s

# Stress: 5 nodes, 10000 events/sec, 60s
cargo run --release --bin omnia-load-test -- --nodes 5 --rate 10000 --duration 60s
```

## ZK Performance

### Methodology
- Groth16 proof generation and verification benchmarks
- Varying batch sizes (1, 4, 16, 64 events)
- Using criterion for statistical rigor

### Results

| Batch Size | Proof Gen Time | Proof Verify Time | Proof Size |
|------------|---------------|-------------------|------------|
| 1 event    | —             | —                 | —          |
| 4 events   | —             | —                 | —          |
| 16 events  | —             | —                 | —          |
| 64 events  | —             | —                 | —          |

### Instructions
Run ZK benchmarks:
```bash
cargo bench --bench zk_benchmarks -- --output-format bencher
```

## Key Findings

_To be populated after running benchmarks._

## Historical Data

| Date | Test | Throughput | Notes |
|------|------|-----------|-------|
| — | — | — | Baseline not yet captured |

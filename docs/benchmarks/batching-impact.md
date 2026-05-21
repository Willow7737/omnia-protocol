# Batching Impact — Benchmark Template

## Overview

This document provides a template for measuring the performance impact of
batch event processing vs. single-event processing in the Omnia Protocol.

## Benchmark Scenarios

### 1. Per-Event CPU Cost: Single vs. Batched

| Metric | Single Event | Batched (50 events) | Batched (100 events) | Improvement |
|--------|-------------|--------------------|---------------------|-------------|
| Validation time (μs/event) | — | — | — | — |
| Proof generation (μs/event) | — | — | — | — |
| Gossip serialization (μs/event) | — | — | — | — |
| **Total per-event CPU (μs)** | — | — | — | **≥40% target** |

**Methodology**:
- Submit N=1000 events through both single-event and batched pipelines
- Measure wall-clock time for validation, proof computation, and serialization
- Report per-event amortized cost (total_time / N)

### 2. End-to-End Latency Comparison

| Metric | Single Event | Batched (50 events) | Batched (100 events) |
|--------|-------------|--------------------|---------------------|
| Submit-to-graph latency (ms) | — | — | — |
| Submit-to-gossip latency (ms) | — | — | — |
| Submit-to-finalized latency (ms) | — | — | — |

**Methodology**:
- Time from event submission to various lifecycle stages
- Batched: includes batch formation time (max 100ms timeout)
- Report P50, P95, P99 latencies

### 3. ZK Proof Generation for 100-Tx Batch

| Metric | Value |
|--------|-------|
| Proof generation time (ms) | — |
| Proof size (bytes) | — |
| Verification time (ms) | — |
| Circuit constraint count | — |

**Methodology**:
- Use `BatchProofCircuit` with `BATCH_PROOF_TARGET_SIZE=100` events
- Measure Groth16 proof generation and verification time
- Compare against per-event proof generation × 100

### 4. Gossip Network Efficiency

| Metric | Single Event | Batched |
|--------|-------------|---------|
| Messages per 1000 events | 1000 | 20 (50/batch) |
| Bytes per event (serialized) | — | — |
| Compression ratio | — | — |
| Network round-trips per 1000 events | — | — |

### 5. CRDT Batch Merge Performance

| Metric | Individual Merge | Batched Merge |
|--------|-----------------|---------------|
| 1000 GCounter increments (ms) | — | — |
| 1000 OrSet adds (ms) | — | — |
| 1000 LwwRegister updates (ms) | — | — |
| Mixed 1000 ops (ms) | — | — |

## Running the Benchmarks

```bash
# Per-event CPU cost
cargo bench --batch-cpu

# End-to-end latency
cargo bench --batch-latency

# ZK proof generation
cargo bench --zk-batch --features arkworks

# Gossip efficiency
cargo bench --batch-gossip --features network

# CRDT batch merge
cargo bench --batch-crdt
```

## Success Criteria

- [ ] Per-event CPU cost reduction ≥40% (batched vs. single)
- [ ] P50 latency increase <2x (acceptable trade-off for throughput)
- [ ] ZK batch proof for 100-tx batch completes in <5 seconds
- [ ] Gossip message count reduced by ≥90% for 1000 events
- [ ] CRDT batch merge faster than individual merges

## Notes

- All benchmarks should be run on a dedicated machine with no background load
- Report median of 5 runs for each metric
- Include hardware specifications (CPU, RAM, OS)
- Compare against the baseline in `docs/benchmarks/baseline-v0.1.43.md`

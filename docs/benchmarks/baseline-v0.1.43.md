# Baseline Benchmark Report — v0.1.43

> Sprint 0 — Phase 0 Throughput Optimization

## Methodology

### Environment

| Parameter | Value |
|-----------|-------|
| **Date** | _Fill in after running_ |
| **Commit** | _Fill in after running_ |
| **Rust Version** | 1.91.0 |
| **OS** | _Fill in after running_ |
| **CPU** | _Fill in after running_ |
| **RAM** | _Fill in after running_ |
| **Profile** | `--release` (LTO=fat, codegen-units=1) |

### Benchmark Suite

The baseline benchmarks are in `benches/benches/baseline_bench.rs` and are run with:

```bash
# Standard benchmarks (no ZK)
cargo bench --bench baseline_bench

# Full benchmarks (including ZK proof generation)
cargo bench --features full --bench baseline_bench
```

### Configuration

| Benchmark | Description | Measurement Window |
|-----------|-------------|--------------------|
| `tx_throughput_bench` | Sustained TPS (events/sec) over 60s window | 10s, 20 samples |
| `finality_latency_bench` | Creation-to-finality p50/p95/p99 | 10s, 100 samples |
| `zk_proof_gen_bench` | ZK proof generation (1-tx, 100-tx) | 30s, 10 samples |
| `gossip_latency_bench` | End-to-end propagation (single-node sim) | 10s, 100 samples |
| `dag_insert_bench` | DAG insertion p50/p95/p99 | 10s, 100 samples |

---

## Target Metrics

These are the sprint targets from the Phase 0 Throughput Optimization Sprint Plan dashboard:

| Metric | Target | Current Baseline | Status |
|--------|--------|-----------------|--------|
| **Sustained TPS** | ≥ 1,000 events/sec | _Fill in_ | ⬜ |
| **Finality Latency p50** | ≤ 500 ms | _Fill in_ | ⬜ |
| **Finality Latency p95** | ≤ 2,000 ms | _Fill in_ | ⬜ |
| **Finality Latency p99** | ≤ 5,000 ms | _Fill in_ | ⬜ |
| **DAG Insert p50** | ≤ 100 µs | _Fill in_ | ⬜ |
| **DAG Insert p95** | ≤ 500 µs | _Fill in_ | ⬜ |
| **DAG Insert p99** | ≤ 1,000 µs | _Fill in_ | ⬜ |
| **Gossip Propagation p50** | ≤ 50 ms | _Fill in_ | ⬜ |
| **ZK Proof (1-tx)** | ≤ 10 sec | _Fill in_ | ⬜ |
| **ZK Proof (100-tx)** | ≤ 120 sec | _Fill in_ | ⬜ |

---

## Baseline Results

### 1. Transaction Throughput (`tx_throughput_bench`)

Measures sustained events/sec with single-node consensus (`total_nodes=1`).

| Metric | Value |
|--------|-------|
| **Events/sec (batch=1000)** | _Fill in_ |
| **Finalized/sec** | _Fill in_ |

#### PromQL Dashboard Query

```promql
rate(omnia_consensus_tps[1m])
```

---

### 2. Finality Latency (`finality_latency_bench`)

Measures time from event creation to consensus commitment.

| Percentile | Value |
|-----------|-------|
| **p50** | _Fill in_ |
| **p95** | _Fill in_ |
| **p99** | _Fill in_ |
| **mean** | _Fill in_ |
| **std dev** | _Fill in_ |

#### PromQL Dashboard Query

```promql
histogram_quantile(0.50, rate(omnia_consensus_finality_latency_seconds_bucket[5m]))
histogram_quantile(0.95, rate(omnia_consensus_finality_latency_seconds_bucket[5m]))
histogram_quantile(0.99, rate(omnia_consensus_finality_latency_seconds_bucket[5m]))
```

---

### 3. ZK Proof Generation (`zk_proof_gen_bench`)

Measures Groth16 proof generation time. Feature-gated under `--features full`.

| Benchmark | Value |
|-----------|-------|
| **1-tx batch (basic circuit)** | _Fill in_ |
| **100-tx batch (expanded circuit)** | _Fill in_ |

---

### 4. Gossip Propagation Latency (`gossip_latency_bench`)

Measures single-node simulation of the gossip pipeline: create → serialize → deserialize → insert.

| Percentile | Value |
|-----------|-------|
| **p50** | _Fill in_ |
| **p95** | _Fill in_ |
| **p99** | _Fill in_ |
| **mean** | _Fill in_ |

#### PromQL Dashboard Query

```promql
histogram_quantile(0.50, rate(omnia_gossip_propagation_latency_seconds_bucket[5m]))
```

---

### 5. DAG Insertion Latency (`dag_insert_bench`)

Measures event insertion into the CausalGraph at different pre-fill sizes.

| Pre-fill | p50 | p95 | p99 |
|----------|-----|-----|-----|
| **0 events** | _Fill in_ | _Fill in_ | _Fill in_ |
| **100 events** | _Fill in_ | _Fill in_ | _Fill in_ |
| **1000 events** | _Fill in_ | _Fill in_ | _Fill in_ |

#### PromQL Dashboard Query

```promql
histogram_quantile(0.50, rate(omnia_dag_insertion_latency_seconds_bucket[5m]))
histogram_quantile(0.95, rate(omnia_dag_insertion_latency_seconds_bucket[5m]))
histogram_quantile(0.99, rate(omnia_dag_insertion_latency_seconds_bucket[5m]))
```

---

## Memory Usage

| Metric | Value |
|--------|-------|
| **RSS at startup** | _Fill in_ |
| **RSS after 10K events** | _Fill in_ |
| **RSS after 100K events** | _Fill in_ |

#### PromQL Dashboard Query

```promql
omnia_node_memory_rss_bytes
```

---

## Regression Detection

To detect regressions between runs, compare baseline numbers against this report:

```bash
# Run benchmarks and save results
cargo bench --bench baseline_bench > results.txt

# Compare with baseline
# (manual comparison — criterion also generates HTML reports in target/criterion/)
```

Criterion generates comparison reports in `target/criterion/<benchmark>/report/index.html` when run with a previous baseline.

---

## Notes

- All latency measurements are for **single-node** consensus (`total_nodes=1`), which provides trivial finality. Multi-node consensus will have higher latencies due to the BFT supermajority requirement.
- ZK proof generation benchmarks require `--features full` and the arkworks dependencies.
- Gossip propagation latency is simulated without actual network I/O — real network latency will add 1-10ms per hop.
- The DAG insertion benchmark shows how performance scales with graph size. Insertion should remain O(1) amortized.

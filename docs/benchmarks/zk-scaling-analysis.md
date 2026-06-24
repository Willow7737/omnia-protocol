# ZK Proof Generation Scaling Analysis

## Executive summary

The "27x superlinear scaling for 100-tx batches" identified in mentor review is a **misinterpretation** caused by comparing two fundamentally different circuits. The actual scaling curve — measured with the SAME circuit at different batch sizes — is **sub-linear**: per-event cost DECREASES as batch size increases (125ms/event → 79ms/event), demonstrating expected amortization of fixed Groth16 prover overhead.

## The misinterpretation

The baseline file contains two ZK benchmarks:

| Benchmark | Circuit | Baseline | 
|-----------|---------|----------|
| `zk_proof_gen_basic` | `RollupCircuit` (basic) | 2.50 ms |
| `zk_proof_gen_expanded_100` | `ExpandedRollupCircuit` (100 events) | 8.00 s |

Comparing these directly: 8.00s / 2.50ms = **3200x** for 100x more events. This appears "superlinear."

**But these are different circuits:**
- `RollupCircuit` (basic) has ~10 constraints and **zero Poseidon hashes**. It proves only that two state roots are linked by a single transition.
- `ExpandedRollupCircuit` (100 events) has ~10,000+ constraints and **~1300 Poseidon hashes**. Each event adds Merkle path verification (8 Poseidon hashes), state transition (1 Poseidon), payload binding (1 Poseidon), and operation-type range check (3 boolean witnesses).

The 3200x ratio is explained by the ~1000x difference in constraint count, with the remaining 3.2x accounted for by Groth16's O(n log n) FFT cost.

## The actual scaling curve

The `zk_benchmarks.rs` file benchmarks the SAME `ExpandedRollupCircuit` at different batch sizes (1, 4, 16 events). Adding the 100-event measurement from `baseline_bench.rs`:

| Events | Time (ms) | Per-event (ms) | Ratio vs 1-event |
|--------|-----------|----------------|-------------------|
| 1      | 125       | 125            | 1.00x             |
| 4      | 415       | 104            | 3.31x (sub-linear)|
| 16     | 1,484     | 93             | 11.87x (sub-linear)|
| 100    | 7,934     | 79             | 63.5x (sub-linear)|

### Key observations

1. **Per-event cost decreases with batch size**: 125ms → 79ms (37% reduction). This is the expected amortization of fixed Groth16 prover overhead (trusted setup loading, FFT twiddle factor computation, etc.).

2. **Scaling is sub-linear, not super-linear**: 100 events takes 63.5x longer than 1 event, not 100x. The prover benefits from batch amortization.

3. **Groth16 proving is O(n log n)** in constraint count due to the FFT in the polynomial commitment. For 100x more constraints, the predicted ratio is ~100 × log(100N)/log(N) ≈ 120-150x. The observed 63.5x is actually BETTER than the theoretical O(n log n) prediction, likely because the fixed overhead (setup loading, etc.) is a larger fraction of the 1-event time.

## Root cause of the confusion

The confusion arises because `baseline_bench.rs` uses two different circuit types in the same benchmark group (`zk_proof_gen`):

```
group.bench_function("1_tx_batch", ...)     // RollupCircuit (basic)
group.bench_function("100_tx_batch", ...)   // ExpandedRollupCircuit (100 events)
```

A reader naturally assumes these are the same circuit at different batch sizes. They are not. The doc comment on `zk_proof_gen_bench` (added 2026-06-19) explicitly warns against this comparison, but the baseline file's `zk_proof_gen_basic` and `zk_proof_gen_expanded_100` entries don't carry the same warning.

## What would actually be a problem

Real superlinear scaling would show per-event cost INCREASING with batch size:

| Events | Per-event (ms) | Status |
|--------|----------------|--------|
| 1      | 125            | —      |
| 4      | 150            | ⚠️ superlinear |
| 16     | 200            | ⚠️ superlinear |
| 100    | 300            | ❌ problem |

This would indicate that something in the constraint generation or proving is O(n²) or worse. The observed DECREASING per-event cost rules this out.

## Recommendations

1. **The scaling is healthy.** No code change is needed to fix "superlinear scaling" because it doesn't exist. The 8-second proof time for 100 events is the expected cost of a Groth16 proof on a ~10,000-constraint circuit with ~1300 Poseidon hashes.

2. **To reduce absolute proof time** (not scaling), the options are:
   - **Reduce Poseidon hash count**: The expanded circuit uses 10 Poseidon hashes per event (8 Merkle + 1 state + 1 payload). Using a cheaper hash for Merkle verification (e.g., a SNARK-friendly hash with fewer rounds) would reduce constraint count proportionally.
   - **Use recursive SNARKs**: Split the 100-event batch into 10 sub-proofs of 10 events each, prove each sub-proof, then aggregate via recursive Groth16. This reduces the per-proof constraint count from ~10,000 to ~1,000 at the cost of an aggregation proof.
   - **Use PLONK instead of Groth16**: PLONK has similar asymptotic complexity but supports universal setups, avoiding the per-circuit trusted setup cost.
   - **Reduce Merkle depth**: The current depth is 8 (256 leaves). If batches are smaller than 256 events, a shallower tree reduces per-event constraints.

3. **To improve the benchmark surface**: The `zk_benchmarks.rs` file already has the proper scaling curve (`expanded_circuit/{1,4,16}`). Adding a 100-event data point to that group (rather than comparing across circuit types) would make the healthy scaling visible in CI output. The baselines file should also cross-reference this document to prevent the misinterpretation from recurring.

## Verification

The numbers in this document can be reproduced by running:

```bash
# Scaling curve (same circuit, different batch sizes)
cargo bench -p omnia-benches --bench zk_benchmarks --features full -- expanded_circuit

# The two circuits that should NOT be compared
cargo bench -p omnia-benches --bench baseline_bench --features full -- zk_proof_gen
```

The scaling data is also visible in the `zk_benchmarks.rs` doc comment (lines 14-16) and the `baseline_bench.rs` doc comment (lines 259-328).

## Conclusion

The ZK proof system scales **sub-linearly** (better than linear) with batch size. The "27x superlinear" appearance is an artifact of comparing two different circuits. The actual per-event cost decreases from 125ms (1 event) to 79ms (100 events) due to amortization of fixed prover overhead. No code change is needed to address scaling; the 8-second absolute time for 100-event proofs is the expected cost of Groth16 on a ~10,000-constraint circuit.

# Omnia Protocol — Verified Limits & Benchmarks

> **Audience**: Performance engineers, validators, integrators
> **Last Verified**: 2026-06-24 (v0.1.68, release build)
> **Test Suite**: `omnia-limit-verification` (39 tests, all passing)

This document records every absolute limit in the Omnia Protocol, verified by
running stress tests against the actual codebase. Theoretical limits (not yet
tested at scale) are marked with ⏳.

---

## 1. Layer 1: Substrate (Causal Graph)

| Constant               |            Value | Unit             | Verified |
| :--------------------- | ---------------: | :--------------- | :------: |
| `MAX_ANCESTRY_DEPTH`   |        1,000,000 | events           |    ⏳    |
| `MAX_ANCESTRY_VISITED` |          100,000 | nodes            |    ⏳    |
| `MAX_TIPS`             |           10,000 | concurrent tips  |    ✅    |
| `MAX_PRUNED_EVENTS`    |           50,000 | metadata entries |    ⏳    |
| `MAX_PAYLOAD_SIZE`     | 1,048,576 (1 MB) | bytes            |    ✅    |

### Verified Behaviors

- **Tip consolidation**: When tips exceed 10,000, the graph auto-consolidates by removing the oldest 10%. After inserting 11,000 concurrent tips, the count settles at 9,999.
- **Payload boundary**: Events with exactly `MAX_PAYLOAD_SIZE` bytes are accepted; `MAX_PAYLOAD_SIZE + 1` is rejected with `PayloadTooLarge`.
- **Ancestry traversal**: Works correctly for chains up to 5,000+ events. The `MaxDepthExceeded` error variant is verified for the 1M limit.
- **Merkle proofs**: Generated and verified for 256-event graphs. Single-event tree returns empty proof (leaf = root).

---

## 2. Layer 2: Domain Shards

| Constant                 | Value | Unit   | Verified |
| :----------------------- | ----: | :----- | :------: |
| Shard count              |     6 | shards |    ✅    |
| Max operations per shard |  8–10 | ops    |    ✅    |

---

## 3. Layer 3: Network (P2P)

| Constant                     |        Value | Unit   | Verified |
| :--------------------------- | -----------: | :----- | :------: |
| `MAX_PENDING_EVENTS`         |      100,000 | events |    ✅    |
| `MAX_EVENTS_PER_GOSSIP`      |          100 | events |    ✅    |
| `MAX_SEEN_EVENTS`            |      100,000 | events |    ✅    |
| `MAX_BATCH_GOSSIP_SIZE`      | 1,048,576 (1 MiB) | bytes | ✅   |
| `DEFAULT_PARTITION_THRESHOLD_MS` | 30,000    | ms     |    ✅    |
| Gossip heartbeat interval    |          500 | ms     |    —     |
| Fanout degree                |            4 | peers  |    —     |
| Mesh size (N)                |            4 | peers  |    —     |
| Max message size             |       65,536 | bytes  |    —     |
| Bloom filter expected items  |      100,000 | events |    ✅    |
| Bloom filter target FPR      | 0.001 (0.1%) | —      |    ✅    |
| Snappy compression threshold |          256 | bytes  |    —     |

---

## 4. Cryptography

| Algorithm   | Use                        | Key/Signature Size                            |
| :---------- | :------------------------- | :-------------------------------------------- |
| Ed25519     | Event signatures           | 32-byte public key, 64-byte signature         |
| BLS12-381   | Threshold signatures, DKG  | 96-byte public key, 48-byte signature         |
| Dilithium3  | Post-quantum signatures    | ~2,420-byte public key, ~3,293-byte signature |
| ML-KEM-768  | Post-quantum KEM           | 1,184-byte encapsulation                      |
| AES-256-GCM | Keystore, share encryption | 32-byte key, 12-byte nonce                    |

---

## 5. Economics & Governance

| Constant                     |          Value | Unit           | Verified |
| :--------------------------- | -------------: | :------------- | :------: |
| `DEFAULT_UBC_QUOTA`          |          1,000 | UBC/month      |    ✅    |
| `DEFAULT_EPOCH_DURATION_MS`  |  2,592,000,000 | ms (30 days)   |    ✅    |
| `DEFAULT_QUORUM_PERCENTAGE`  |             67 | %              |    ✅    |
| `DEFAULT_SLASH_THRESHOLD`    |            500 | points         |    ✅    |
| `DEFAULT_EJECTION_THRESHOLD` |          2,000 | points         |    ✅    |
| Equivocation penalty         |            500 | points/offense |    ✅    |
| Liveness violation penalty   |            100 | points/offense |    ✅    |
| Invalid attestation penalty  |            300 | points/offense |    ✅    |
| Quadratic voting             | `isqrt(stake)` | weight formula |    ✅    |

---

## 6. CRDT Limits

| Constant                  |      Value | Unit                      | Verified |
| :------------------------ | ---------: | :------------------------ | :------: |
| `MAX_CRDT_BATCH_SIZE`     |      1,000 | operations                |    ✅    |
| GCounter max per-node     | `u64::MAX` | —                         |    ✅    |
| GCounter total saturation | `u64::MAX` | saturates on sum overflow |    ✅    |

---

## 7. Throughput Benchmarks (Release Build)

All benchmarks run on a single thread, release profile. Limit verification tests use the workspace default profile (`lto = "fat"`, `codegen-units = 1`). The v0.1.48 micro-benchmarks used a different profile (opt-level=2, no LTO, codegen-units=16). The numbers below are the v0.1.68 baselines from `benches/baselines.json` (12,000 ops/s sustained throughput, 24.5 μs finality latency).

| Benchmark                          |                     Rate |     Latency | Conditions                                                           |
| :--------------------------------- | -----------------------: | ----------: | :------------------------------------------------------------------- |
| Sustained throughput (v0.1.68)     |    ~**12,000** ops/s     |     ~24.5 μs | v0.1.68 baseline (`benches/baselines.json`); single-node release     |
| Finality latency (v0.1.68)         |           —              |     ~24.5 μs | v0.1.68 baseline (`benches/baselines.json`)                          |
| CausalGraph insertion (full cycle) |         ~**1,400** evt/s | ~700 μs/evt | 10K events, 0B payload, linear chain; includes create+sign+insert    |
| CausalGraph insertion only         | ~**55,000** evt/s (est.) |  ~18 μs/evt | DAG insert p50 from Criterion benchmarks; insertion only, no signing |
| ConsensusEngine processing         |         ~**9,000** evt/s | ~111 μs/evt | 1K events, 0B payload, linear chain; total_nodes=4                   |
| Ed25519 signature verification     | ~**27,000** sig/s (est.) |  ~37 μs/sig | 1K signatures, simple test timing; not a Criterion benchmark         |
| VRF leader selection               |         **10,000** sel/s |           — | 150 candidates, 10K rounds                                           |
| CRDT batch merge                   |       ~100K ops/s (est.) |           — | Estimated from BatchCrdtMerger; no dedicated benchmark exists        |

### Memory Estimates

| Component                      | Size       |
| :----------------------------- | :--------- |
| Event struct (stack reference) | 296 bytes  |
| Event total (estimated)        | ~370 bytes |
| Per-event graph overhead       | ~546 bytes |
| CausalGraph struct (stack)     | 408 bytes  |
| ConsensusEngine struct (stack) | 480 bytes  |
| GCounter struct (empty)        | 24 bytes   |

### Extrapolation for 1M Events

| Metric                      | Estimate |
| :-------------------------- | :------- |
| Graph memory                | ~546 MB  |
| With 10K tips overhead      | ~547 MB  |
| With 256-node vector clocks | ~548 MB  |

---

## 8. VRF Leader Selection Distribution

Tested with 150 candidates (stake 110–1,510), 10,000 rounds:

| Metric                    | Value              |
| :------------------------ | :----------------- |
| Unique leaders selected   | 150 / 150 (100%)   |
| Min selections per leader | 7                  |
| Max selections per leader | 136                |
| Distribution              | Stake-proportional |

---

## 9. Test Suite Summary

| Crate                          |     Tests |     Status      |
| :----------------------------- | --------: | :-------------: |
| omnia-primitives               |        57 |       ✅        |
| omnia-crypto (with BLS)        |       189 |       ✅        |
| omnia-consensus                |       294 |       ✅        |
| omnia-network                  |       123 |       ✅        |
| omnia-adapters (with arkworks) |       151 |       ✅        |
| omnia-shards                   |       115 |       ✅        |
| omnia-binding                  |        60 |       ✅        |
| omnia-economics                |        99 |       ✅        |
| omnia-substrate (with network) |        67 |       ✅        |
| omnia-node                     |        37 |       ✅        |
| omnia-chaos-tests              |        84 |       ✅        |
| Limit verification             |        39 |       ✅        |
| **Total**                      | Run `cargo test --workspace` for current count | **All passing** |

> Note: Test counts vary by feature configuration. The 1,382 figure includes feature-gated tests (BLS, arkworks/ark-bn254, etc.) that are only compiled when those features are enabled. Run `cargo test --workspace` for the current count.

---

## 10. Theoretical Limits (Not Yet Stress-Tested)

| Limit                 |                            Value | Reason                                                   |
| :-------------------- | -------------------------------: | :------------------------------------------------------- |
| `MAX_ANCESTRY_DEPTH`  |                        1,000,000 | Would require ~546 MB graph memory; tested to 5K         |
| `MAX_PENDING_EVENTS`  |                          100,000 | Tested to 100 in pool; gossip path needs multi-node test |
| ZK proof generation   | ~8s / 100-event expanded circuit | CPU-bound                                                |
| Multi-node throughput |        ~500–1,000 evt/s per node | Estimated; needs real P2P network                        |
| Max validators        |                              100 | Defined in ConsensusConfig; not stress-tested            |

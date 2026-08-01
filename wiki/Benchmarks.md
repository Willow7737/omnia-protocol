# Benchmarks

Omnia's performance claims are **benchmark-gated**: CI fails if the hot
path regresses (3-layer gate: deterministic IAI instruction counts,
multi-sample criterion with bootstrap confidence intervals, and
single-sample fast gates). The canonical, always-current record is
[`docs/reference/benchmark-gates.md`](https://github.com/Willow7737/omnia-protocol/blob/main/docs/reference/benchmark-gates.md)
— this page is the summary.

## Live-network results (July 2026, real QUIC/gossipsub mesh)

The headline: **a 10,000-event burst reaches 100% propagation AND full
Lane 0 BFT finality on every node — zero loss — both on a 5-node
single-host mesh (2026-07-19) and on a real 3-region WAN validator
network (2026-07-20): Nuremberg / Ashburn / Singapore, RTTs up to
~218 ms.**

| Burst | Topology | Propagation | Finality | Convergence |
|---|---|---|---|---|
| 1,000 | 5-node mesh | 100% ×5 | full quorum | ~10 s |
| 2,000 | 5-node mesh + Lane 0 | 100% ×5 | 2,000/2,000 | ~7–17 s |
| 5,000 | 5-node mesh + Lane 0 | 100% ×5 | 5,000/5,000 | ~40–45 s |
| **10,000** | **5-node mesh + Lane 0** | **100% ×5** | **10,000/10,000** | **median < 60 s** (stragglers to ~5 min) |
| 1,000 | **3-region WAN** (99–218 ms RTT) | 100% ×3 | full quorum | < 8 s |
| 5,000 | 3-region WAN | 100% ×3 | full quorum | 12 s–4.6 min |
| **10,000** | **3-region WAN** | **100% ×3** | **full quorum** | **2 of 3 nodes < 40 s** (straggler ~4.8 min) |

The 10k result is the interesting one: the burst deliberately overwhelms
live gossip, and **anti-entropy repair recovers everything** — nodes
exchange frontier digests, fetch exactly what they miss in chained batches,
and re-admit it through the full validation pipeline. Nothing is trusted,
nothing is lost.

## The 2026-07-19 diagnosis arc (why you can trust these numbers)

Getting 10k to converge exposed and fixed **five stacked bottlenecks**,
each masked by the next — serve-throughput limits, a deferral-queue
priority inversion, a rate limiter throttling solicited repair, and finally
the root cause: an unconfigured 64 KiB gossipsub transmit cap silently
dropping every repair batch. Each fix landed with a locking regression
test, and each was verified against a provably fresh binary before the next
layer was touched. The full forensic narrative is in
[`benchmark-gates.md`](https://github.com/Willow7737/omnia-protocol/blob/main/docs/reference/benchmark-gates.md)
— kept deliberately, failures included, because a benchmark record you can
audit is worth more than a highlight reel.

## Hot-path numbers (single node, synchronous, CI-gated)

| Metric | Baseline (v0.1.68) |
|---|---|
| Sustained consensus throughput | ~12,000 ops/s |
| Finality latency p50 | ~25 µs |
| DAG insert p50 | ~23 µs |
| Event create + sign | ~22 µs |
| ZK proof (basic circuit) | ~2.5 ms |

These are in-process numbers — **not** comparable to the live-network
numbers above, which include HTTP, JSON, signing, and real network I/O.
The docs are strict about never mixing the two.

## Honest caveats (we keep them attached)

- The WAN repair tail stretches with RTT: the straggling node on large
  bursts pays repair-chain round-trips over the worst path (B↔C,
  ~218 ms). Median convergence stays fast; the documented next lever is
  directed (unicast) repair serving.
- Large-burst convergence has a repair tail (stragglers up to ~5 min at
  10k); the comfort zone for sub-minute convergence is ~2,000–3,000 events
  per burst. Tail-tuning levers are documented.
- ZK proving (~88 ms/event expanded circuit) is orders of magnitude slower
  than consensus — by design it runs asynchronously and never blocks
  finality.

# ADR-025: Two-Lane Consensus

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-08-11

## Status

Proposed

## Date

2026-07-10

## Version

1.0.0

## Decision

Split transaction finality into two lanes that share one substrate (the existing
causal event DAG + gossip):

- **Lane 0 — consensusless fast path.** UBC operations are *single-writer by
  construction*: UBC is soulbound, so a transfer only ever debits (burns from)
  the sender's own balance, and the sender already totally orders their own
  events via `sequence` + self-parent chaining. Such operations do not need
  network-wide total ordering. A Lane 0 operation is final as soon as a
  stake-weighted quorum of validators has acknowledged the sender's event —
  one round trip, no leader, no rounds, no voting. Quorum acknowledgments are
  aggregated into a **finality certificate** and disseminated as a grow-only
  set CRDT riding the existing gossip topic (idempotent, order-independent,
  merge = set union), so certificates need no new protocol machinery.

- **Lane 1 — DAG-native BFT for contested operations.** Operations that touch
  genuinely shared state — governance execution, validator-set changes, epoch
  transitions, cross-shard operations — go through a commit rule evaluated
  *over the causal graph that gossip already builds* (Bullshark/Mysticeti
  style): the DAG **is** the vote record, and vector clocks already encode
  who-saw-what-when. The commit rule is a pure function of local DAG state;
  it adds **zero new message types** on the wire.

- **ZK checkpointing is demoted from the hot path.** Proof generation
  (Groth16, ~7.8 s per 100-tx batch) becomes lazy, periodic checkpointing of
  already-final state — it compresses history for light clients and
  settlement, it does not gate finality.

Rollout is staged and benchmark-gated (see *Staged Plan* below); each stage
lands independently behind the existing CI performance gates.

## Context

The July 2026 local benchmark run (`docs/reference/benchmark-gates.md`,
2026-07-09 reference run) quantified a cliff the architecture audits had
already flagged qualitatively:

| Path | Throughput |
| --- | --- |
| Single-node hot path (validate + insert, 1000-event batches) | ~7,675 events/s |
| 3-node simulated network (`network_sim` bench) | ~77 events/s |

The ~100× drop is **coordination cost**, not compute: signature checks take
18 µs and DAG inserts 20 µs, but every event pays for gossip fanout,
redundant retransmission, and consensus-round coupling before it counts as
processed. Meanwhile, the workload that actually dominates the live network
(wallet UBC transfers, DID registrations) is precisely the workload that
needs none of that coordination:

1. **UBC is soulbound.** A transfer burns from the sender's balance; the
   recipient's balance never changes. There is no shared account two
   senders can race on — no double-spend *between* accounts is expressible.
2. **Senders self-serialize.** Events carry a per-creator `sequence` and a
   self-parent hash chain; equivocation (two events with the same sequence)
   is detectable and slashable evidence, not an ordering problem for
   consensus to resolve.
3. Together, every account is a **single-writer register**, which is the
   textbook precondition (FastPay, Linera, Sui's owned-object path) for
   consensusless, certificate-based finality at 1 RTT.

At the same time, the components built to cheapen the coordination that
*does* remain were never wired in: `PriorityGossipQueue`,
`GossipBloomFilter`, and `CompactEncoder` exist, are fully unit-tested, and
are dead code (AUDIT-14); the `PipelineRouter` workers were removed as dead
code rather than integrated (C-14 / AUDIT-15).

## Alternatives Considered

### Single-lane classical BFT (HotStuff / Tendermint style)

Run every operation through leader-based BFT rounds. Battle-tested, but it
taxes the ~95% of traffic that is single-writer with the full cost of total
ordering, caps throughput at the leader's bandwidth, and would sit *beside*
the causal DAG rather than using it — two consensus-critical data structures
to keep consistent.

### Pure DAG consensus for all traffic (Narwhal/Bullshark everywhere)

Commit everything through the DAG commit rule. Simpler than two lanes
conceptually, but even the best DAG-BFT pays multi-round commit latency
(2–4 gossip rounds) on operations that provably need none, and wave-based
commit rules do their accounting per-round for *all* events, not just the
contested minority.

### ZK-rollup-first (prove everything, finality = proof)

Make the Groth16 batch proof the finality gate. Strongest verifiability
story, but at ~7.8 s per 100-tx batch the prover becomes the throughput
ceiling (~13 tx/s), and proving latency becomes user-visible finality
latency. Proofs are kept — as checkpoints, off the hot path.

### Buy throughput with integration work only (no protocol change)

Integrating the idle gossip components (Stage 1) without Lane 0 recovers
real bandwidth and latency, but it cannot cross the coordination cliff: the
3-node ceiling is dominated by consensus-round coupling, not message size.
Stage 1 is necessary, not sufficient — so it is the first stage of this
ADR rather than a competing decision.

## Staged Plan

Each stage is independently mergeable, benchmark-gated, and honest about
what it proves.

1. **Stage 1 — integrate the idle components (closes AUDIT-14).**
   Wire `GossipBloomFilter` (duplicate suppression), `PriorityGossipQueue`
   (consensus-critical events first), and `CompactEncoder` (delta-encoded
   vector clocks behind a versioned wire envelope) into `GossipProtocol`.
   No semantic change to consensus; pure bandwidth/latency recovery.
2. **Stage 2 — real 3-node testnet + honest numbers.** Deploy the existing
   `docker-compose.testnet.yml` topology on real hosts, measure multi-node
   throughput/finality with the benchmark methodology from
   `benchmark-gates.md`, and record the numbers as the pre-Lane-0 baseline.
3. **Stage 3 — Lane 0 for UBC.** Sender-sequenced events, stake-weighted
   quorum acks, finality certificates as G-Set CRDTs on the existing gossip
   topic. Wallet-visible finality target: 1 gossip RTT. The pipeline
   worker roles that AUDIT-15 tracked are absorbed into Lane 0's
   ack-aggregation workers (AUDIT-15 is resolved by supersession, not by
   resurrecting the log-only stubs).
4. **Stage 4 — Lane 1 DAG commit rule.** Commit rule over the existing
   causal graph for governance/validator-set/epoch/cross-shard operations;
   extend the existing TLA+ spec to model both lanes and the
   certificate-CRDT merge.
5. **Stage 5 — consensus arena.** Adversarial property-based CI gate:
   equivocation, certificate withholding, partition/heal schedules, and
   lane-crossing attacks (e.g., governance racing a Lane 0 burn) must hold
   the safety invariants (no conflicting certificates; committed Lane 1
   prefix is identical on all honest nodes).

## Consequences

### Positive

- Finality for the dominant workload (UBC transfers) drops from
  consensus-round latency to one gossip round trip — without weakening
  safety, because single-writer safety is enforced by construction
  (soulbound semantics + sender sequencing + equivocation slashing).
- Lane 1 reuses the causal DAG and vector clocks the protocol already
  maintains; no parallel consensus data structure, no new message types.
- Certificates-as-CRDTs make finality dissemination idempotent and
  partition-tolerant; a node that missed a certificate converges by normal
  gossip merge.
- ZK proving stops being a latency bottleneck while keeping its
  verifiability value as checkpoints.
- The plan starts with integration work that pays for itself (Stage 1) and
  produces honest multi-node baselines (Stage 2) before any protocol
  change, so every later claim is measured against a real network.

### Negative

- Two finality paths mean two safety arguments; the TLA+ extension and the
  consensus arena (Stages 4–5) are mandatory, not optional hardening.
- Lane 0 requires a live validator set with known stake for quorum
  weighting; on the current single-node testnet, Lane 0 degenerates to
  local finality until Stage 2 lands.
- Cross-lane interactions (a governance action that changes economics
  parameters mid-epoch while Lane 0 burns are in flight) need explicit
  sequencing rules — Lane 1 commits act as epoch fences for Lane 0
  certificate validity.
- Equivocation handling moves from "consensus resolves it" to "detection +
  slashing"; the slashing path (ADR-011) becomes consensus-critical.

## References

- FastPay: High-Performance Byzantine Fault Tolerant Settlement (Baudet,
  Danezis, Sonnino) — consensusless single-writer payments.
- Sui Lutris / Mysticeti, Bullshark — DAG-native commit rules and
  owned-object fast paths.
- `docs/reference/benchmark-gates.md` — 2026-07-09 local reference run.
- `docs/reference/status.md` — AUDIT-14 / AUDIT-15 tracking.
- ADR-011 (gradual slashing), ADR-015 (leader selection), ADR-021 (gossip
  compression) — decisions this ADR builds on.

---

🔙 **Back**: [adr-index](../reference/adr-index.md)

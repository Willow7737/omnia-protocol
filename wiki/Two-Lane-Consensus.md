# Two-Lane Consensus (ADR-025)

Omnia finalizes events on two lanes with different trust requirements —
because most of the world's coordination doesn't need full BFT agreement,
and making everything pay for it is why blockchains are slow.

## Lane 0 — Consensusless fast path

For operations whose validity is **self-evident given the causal graph**
(UBC quota spends, transfers with clear provenance), Omnia uses
**quorum-acknowledged finality without ordering consensus**:

1. A validator sees an event, validates it fully, and broadcasts a signed
   acknowledgment.
2. Acks accumulate in a **grow-only CRDT certificate** (a G-Set — order
   cannot matter, duplicates cannot matter, nothing can be retracted).
3. When acks representing **> 2/3 of validator stake** exist, the event is
   final. No leader, no rounds, no view changes.

This is the lane that finalized **10,000/10,000 events across all 5
validators** in the July 2026 stress runs. Validator-set changes are
epoch-fenced and themselves authorized through Lane 1.

## Lane 1 — DAG-native BFT for contested state

State that can genuinely conflict (governance, validator-set changes,
anything where two honest nodes could disagree) goes through full BFT
consensus over the causal graph — an AlephBFT-inspired famous-witness
commit rule that finalizes graph cuts deterministically.

## Why this split is sound

- Lane 0's certificate is a CRDT: any two nodes that see the same acks
  compute the same finality, in any order, with no coordination.
- The quorum threshold makes conflicting Lane 0 finality impossible with
  < 1/3 Byzantine stake — the same bound as the BFT lane.
- Formally specified: `formal-verification/OmniaTwoLane.tla` models both
  lanes plus the epoch fence, and is **model-checked by TLC in CI on every
  pull request**, alongside `OmniaConsensus.tla` and `OmniaCRDT.tla`.
- Adversarially tested: a property-based "consensus arena" drives
  withholding, reordering, duplication, forgery, and rotation-schedule
  attacks against Lane 0 in CI.

## Reading list

- [ADR-025 — Two-Lane Consensus](https://github.com/Willow7737/omnia-protocol/blob/main/docs/adr/ADR-025-two-lane-consensus.md) (the decision record)
- [`substrate/src/lane0.rs`](https://github.com/Willow7737/omnia-protocol/blob/main/substrate/src/lane0.rs) (the implementation)
- [`formal-verification/`](https://github.com/Willow7737/omnia-protocol/tree/main/formal-verification) (the TLA+ specs)
- [Benchmarks](Benchmarks) (what it measures like on a real network)

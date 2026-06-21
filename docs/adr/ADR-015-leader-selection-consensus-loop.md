# ADR-015: Leader Selection in Consensus Loop

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-05-20

## Status

Accepted

## Date

2026-05-19

## Version

1.0.0

## Decision

Implement VRF-based leader selection wired into `process_consensus_round()` with a mempool for pending transactions. The leader for each round is determined by a Verifiable Random Function (VRF) output, weighted by stake, ensuring fair and deterministic leader rotation.

## Context

The consensus engine previously lacked a formal leader selection mechanism for block production. Without a leader, there was no clear responsibility for proposing new blocks in each round, leading to either no proposals or uncoordinated proposals from all validators simultaneously. This created inefficiency and potential for conflicts.

A leader selection mechanism needed to satisfy several requirements:

1. **Determinism**: All honest validators must agree on who the leader is for a given round without additional communication.
2. **Fairness**: Leaders should be selected proportionally to their stake, giving higher-staked validators more responsibility but not monopolizing block production.
3. **Verifiability**: Any participant must be able to verify that a leader was selected correctly.
4. **Liveness**: The system must continue producing blocks even if the selected leader is offline (via timeout and round advancement).

Additionally, a mempool was needed to buffer pending transactions before they are included in blocks, providing transaction ordering and prioritization.

## Alternatives Considered

### Round-Robin Leader Selection

Each validator takes turns in a fixed order. Simple to implement but has several drawbacks: predictable leader schedule enables targeted attacks, no stake-weighting means all validators have equal influence regardless of stake, and the order must be re-computed whenever the validator set changes.

### RANDAO-Based Leader Selection

Use a commit-reveal scheme (like Ethereum's RANDAO) to generate randomness for leader selection. This provides better unpredictability than round-robin and is battle-tested in Ethereum. However, it requires multiple rounds of communication (commit and reveal phases), adding latency to each round. The last revealer also has some influence over the random value.

## Consequences

### Positive

- Fair leader selection weighted by stake, proportional to economic commitment
- Deterministic leader computation — no additional communication rounds needed
- VRF output is publicly verifiable, preventing leader forgery
- Mempool enables transaction ordering and prioritization before inclusion
- Bounded mempool size (10,000 default) prevents memory exhaustion
- `tokio::select!` + round timer replaces inefficient 100ms sleep poll loop
- `process_consensus_round()` provides clean separation of round logic

### Negative

- VRF-based selection requires each validator to compute a VRF every round (small CPU cost)
- Current VRF construction is non-standard (Ed25519 + BLAKE3, not ECVRF — see ADR-012)
- Stake-weighted selection may concentrate influence among high-stake validators
- Mempool requires memory proportional to pending transaction volume

### Trade-offs

- Chose VRF over RANDAO for single-round determinism without commit-reveal latency
- Chose stake-weighted over round-robin for economic fairness
- Mempool with bounded size trades potential transaction drops for memory safety

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

# ADR-019: Fast-Sync Protocol

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-06-24

## Status

Accepted

## Date

2026-05-19

## Version

1.0.0

## Decision

Implement a fast-sync protocol using checkpoint types with BLAKE3 domain-separated hashes, supermajority checkpoint selection (2/3+ stake agreement), P2P download via request-response protocol, and delta replay after snapshot application. New nodes can sync to the latest state without full replay from genesis.

## Context

As the Omnia network grows, replaying all events from genesis becomes prohibitively slow. A new node joining the network must process every historical event to reach the current state, which creates:

1. **Long bootstrapping time**: Hours or days of replay before a new node can participate.
2. **Resource waste**: Every node redundantly processes the same historical events.
3. **Barrier to entry**: Slow synchronization discourages new validators from joining.
4. **Operational risk**: Prolonged replay increases the window for errors and inconsistencies.

A fast-sync mechanism allows new nodes to:

1. Download a verified snapshot of the current state
2. Replay only the delta events since the snapshot
3. Begin participating in consensus much sooner

The trust model for fast-sync is anchored in supermajority agreement — a snapshot is only accepted if 2/3+ of validators (by stake) agree on it, making it as secure as the underlying BFT consensus.

## Alternatives Considered

### Full Replay Only

Require every node to replay from genesis. Maximum trust minimization — no external trust required beyond the genesis state. However, this does not scale and becomes impractical as the chain grows.

### State Sync via External Service

Use a trusted external service (e.g., cloud storage) to provide snapshots. Simple to implement but introduces a trusted third party, creating a centralization vector and single point of failure.

## Consequences

### Positive

- Fast node bootstrapping — new nodes can sync in minutes instead of hours
- Trust anchored in supermajority agreement (2/3+ stake), same security as BFT consensus
- BLAKE3 domain separation (`OMNIA-FAST-SYNC-V1`) prevents cross-protocol hash collisions
- P2P download via request-response protocol — no external service dependency
- Delta replay after snapshot ensures the node has the exact same state as if it had replayed from genesis
- `SyncNetwork` trait abstracts P2P operations, decoupling fast-sync from libp2p
- `try_sync_or_fallback()` provides graceful fallback to genesis replay if sync fails
- `SyncSnapshot` compact wire format minimizes bandwidth

### Negative

- New nodes must trust that supermajority of validators agreed on the snapshot
- Snapshot download is a large data transfer (proportional to state size)
- Delta replay may still be significant if the snapshot is old
- Checkpoint creation adds per-round overhead (hashing state)
- Protocol requires at least one responsive peer with a checkpoint

### Trade-offs

- Chose supermajority trust over zero-trust (full replay) for practicality
- Chose P2P download over external service for decentralization
- `SyncNetwork` trait allows testing without real P2P infrastructure
- `try_sync_or_fallback()` trades a small amount of complexity for robust startup behavior

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

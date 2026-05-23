# ADR-018: Consensus State Persistence
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

Use `RedbConsensusStore` backed by the redb embedded database for consensus state persistence. Save state after every round advancement. Use the `load_or_new()` pattern for initialization — if persisted state exists, load it; otherwise, create a new engine from genesis.

## Context

Consensus state (current round, seed, committed events, equivocation tracking) was previously held only in memory. If a node restarted, all consensus state was lost and the node had to replay from genesis. This created several problems:

1. **Slow recovery**: Replaying from genesis becomes increasingly slow as the chain grows.
2. **Network instability**: Restarting nodes briefly lose their place in the consensus, potentially causing view changes.
3. **No equivocation memory**: Equivocation tracking was lost on restart, allowing misbehaving validators to avoid detection by simply restarting.
4. **Operational burden**: Operators had to wait for full replay before a node could rejoin consensus.

A persistent store is needed that:

- Survives node restarts with ACID guarantees
- Can be loaded quickly on startup
- Integrates cleanly with the existing consensus engine lifecycle
- Handles concurrent access safely (single writer, multiple readers)

## Alternatives Considered

### Sled
Sled is an embedded database with good performance characteristics. However, it has known stability issues (data corruption reports), is no longer actively maintained, and was already removed from the project in Phase 2 (RUSTSEC-2024-0384 was related to sled's `instant` dependency).

### Custom Binary Format
Implement a custom binary serialization format with `std::fs` writes. Maximum control and minimal dependencies, but no ACID guarantees (partial writes on crash can corrupt state), no concurrent access support, and significant implementation effort for features that redb provides out of the box.

## Consequences

### Positive
- ACID durability — consensus state survives crashes and power failures without corruption
- Compact storage — redb uses B-tree pages with efficient space utilization
- Consistent `load_or_new()` initialization pattern — no special-case startup logic
- `save_state()` after every round advancement ensures minimal data loss (at most one round)
- `ConsensusStore` trait allows future backend changes without modifying consensus logic
- redb is actively maintained and has no open RUSTSEC advisories
- Single-writer model matches consensus engine's single-threaded round processing

### Negative
- Additional disk I/O after every round advancement (mitigated by redb's write batching)
- redb database file grows over time (compaction available but not automatic)
- State serialization/deserialization adds per-round latency
- One more dependency on the critical path

### Trade-offs
- Chose redb over sled for stability and active maintenance
- Chose redb over custom format for ACID guarantees
- `load_or_new()` pattern trades a small amount of startup complexity for robust crash recovery
- Per-round persistence trades I/O overhead for minimal data loss window

---
🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

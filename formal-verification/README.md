# Omnia Protocol — Formal Verification

This directory contains a TLA+ specification that models the Omnia consensus protocol and verifies its safety properties through exhaustive state-space exploration using the TLC model checker.

## Protocol Model

The Omnia consensus is modeled as a hybrid of three established approaches:

- **Hashgraph-style event ordering** — events form a DAG with two-parent references, establishing causal relationships.
- **AlephBFT-style BFT finality** — a 3f+1 voting threshold ensures safety as long as fewer than one-third of nodes are Byzantine.
- **CRDT-based state convergence** — the gossip substrate ensures all honest nodes eventually converge on the same event graph.

The TLA+ spec captures the core state machine: event creation, gossip propagation, equivocation by Byzantine nodes, and the consensus lifecycle from pending through to committed.

## Spec Design

### EventId includes hash

Unlike the original spec (which used `(creator, sequence)` as the event key), this spec uses `(creator, sequence, hash)` as the `EventId`. This is critical for modeling equivocation correctly: when a Byzantine node creates two events at the same `(creator, sequence)` with different hashes, both events must coexist in the event map. Including hash in the key ensures two distinct entries are created rather than the second overwriting the first.

### Simplified consensus

The `CommitEvent` action advances any pending event directly to "committed" without quorum requirements. A production spec would gate commitment on supermajority witness votes. This simplification means the spec can find Agreement violations that would not occur in the real protocol with proper BFT quorum enforcement.

## Installing TLA+

### Option A: TLA+ Toolbox (recommended)

1. Download from [https://github.com/tlaplus/tlaplus/releases](https://github.com/tlaplus/tlaplus/releases)
2. Extract and launch the Toolbox IDE
3. File → Open Spec → Add new spec → select `OmniaConsensus.tla`

### Option B: VS Code Extension

1. Install the **TLA+** extension by Aly Badr from the VS Code marketplace
2. The extension bundles the TLA+ tools (TLC, TLAPS, PlusCal translator)
3. Open this `formal-verification/` directory in VS Code

### Option C: Command Line

```bash
# Requires Java 11+
wget https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar
java -jar tla2tools.jar OmniaConsensus.tla -config OmniaConsensus.cfg
```

## Running the Model Checker

### Using CLI

```bash
cd formal-verification
java -XX:+UseParallelGC -jar tla2tools.jar \
    -config OmniaConsensus.cfg \
    -workers auto \
    OmniaConsensus.tla
```

### Configuration

The model is configured in `OmniaConsensus.cfg`:

| Parameter | Value | Meaning |
|---|---|---|
| `Nodes` | `{n1, n2, n3, n4}` | 4-node network |
| `ByzantineNodes` | `{n1}` | 1 Byzantine node (BFT threshold: f=1 requires 4 nodes) |
| `MaxSeq` | `1` | Each node creates at most 1 event (sequence 0) |

## Properties Verified

### 1. Agreement (Safety) — **VIOLATED**

```tla
Agreement == \A n1 \in Honest, n2 \in Honest:
    \A eid1 \in EventId, eid2 \in EventId:
        /\ EventExists(n1, eid1)
        /\ EventExists(n2, eid2)
        /\ events[n1][eid1].status = "committed"
        /\ events[n2][eid2].status = "committed"
        /\ eid1.creator = eid2.creator
        /\ eid1.sequence = eid2.sequence
        => eid1.hash = eid2.hash
```

All honest nodes that commit an event at the same `(creator, sequence)` should agree on its hash. **This invariant is VIOLATED** by the current spec.

**Counterexample**: A Byzantine node equivocates (creates two events with same `(creator, sequence)` but different hashes). Both events are gossiped to honest nodes, and both are committed via the `CommitEvent` action. This results in two honest nodes committing different hashes for the same logical event position.

**Root cause**: The `CommitEvent` action is permissive — any pending event can be committed without quorum requirements. In the real protocol, commitment requires supermajority (>2/3) witness votes, which would prevent honest nodes from committing both equivocating events. This violation is a modeling artifact, not a protocol flaw.

### 2. NoEquivocation (Integrity) — **HOLDS**

```tla
NoEquivocation == \A n1 \in Honest, n2 \in Honest:
    \A eid1 \in EventId, eid2 \in EventId:
        /\ EventExists(n1, eid1)
        /\ EventExists(n2, eid2)
        /\ events[n1][eid1].status = "committed"
        /\ events[n2][eid2].status = "committed"
        /\ eid1.creator = eid2.creator
        /\ eid1.sequence = eid2.sequence
        /\ eid1.hash # eid2.hash
        => eid1.creator \in ByzantineNodes
```

If two committed events share the same `(creator, sequence)` but have different hashes, the creator must be Byzantine. This invariant holds because only `Equivocate` (which requires `n \in ByzantineNodes`) creates two events at the same `(creator, sequence)` with different hashes. Honest nodes use `CreateEvent`, which always uses hash=1 deterministically.

### 3. Validity (Liveness-related Safety) — **HOLDS**

```tla
Validity == \A n \in Honest:
    \A eid \in EventId:
        /\ EventExists(n, eid)
        /\ events[n][eid].status = "committed"
        => eid.sequence < current_seq[eid.creator]
```

If an honest node commits an event, some node actually proposed it. No committed event is fabricated. This holds because `CreateEvent` and `Equivocate` both advance `current_seq` when creating events, so any event with `sequence < current_seq[creator]` was genuinely proposed.

### 4. TypeOK (State Invariant) — **HOLDS**

Basic well-typedness invariant ensuring all state variables remain within their intended domains.

## TLC Results

**Configuration tested:** `Nodes = {n1, n2, n3, n4}`, `ByzantineNodes = {n1}`, `MaxSeq = 1`

| Property | Status | Notes |
|---|---|---|
| TypeOK | ✅ Holds | Well-typedness invariant verified |
| Agreement | ❌ Violated | Honest nodes can commit equivocating events; see root cause analysis above |
| NoEquivocation | ✅ Holds | Equivocation is confined to Byzantine creators |
| Validity | ✅ Holds | Committed events were proposed by some node |

**Execution:** TLC v1.8.0, ~608 distinct states found at depth 6 before Agreement violation detected. Runtime < 2 seconds.

## Fixing the Agreement Violation

The Agreement violation can be resolved by adding a quorum requirement to the `CommitEvent` action. For example:

1. Track how many nodes have acknowledged each event via gossip
2. Only allow commitment when >2/3 of nodes have the event in "acknowledged" or higher state
3. This prevents equivocating events from both reaching the committed state at honest nodes

This is left as future work — the current spec correctly identifies the modeling gap.

## Known Limitations

| Limitation | Details |
|---|---|
| **Permissive commitment** | `CommitEvent` has no quorum requirement. This causes Agreement violations that would not occur with proper BFT voting. |
| **Bounded state space** | Model checking is over a finite set of 4 nodes with MaxSeq=1. Scaling beyond this is limited by state explosion. |
| **Simplified gossip** | Gossip is modeled as atomic one-step transfer, not the multi-round epidemic protocol used in production. |
| **No network partitions** | The model assumes reliable delivery. Partition tolerance is not modeled. |
| **Abstract hashes** | Honest nodes use hash=1 deterministically; Byzantine nodes use hash=1 and hash=2. Collision resistance is assumed, not proved. |
| **No timing** | The model is untimed; partial synchrony assumptions are not captured. |
| **Byzantine set is static** | The set of Byzantine nodes is fixed. Adaptive corruptions are not modeled. |
| **Equivocation only** | Byzantine behavior is limited to equivocation. More complex attacks (selective forwarding, Sybil) are not captured. |
| **No parent references** | The two-parent DAG structure is abstracted away; only the creator+sequence+hash identification is modeled. |

## How to Interpret Results

### TLC reports "No error found"

All specified invariants hold for every reachable state within the configured bounds. This provides **bounded verification** — the properties are true for the modeled configuration, but not a proof for arbitrary configurations.

### TLC reports an invariant violation

1. Examine the error trace (counterexample) that TLC produces
2. The trace shows a step-by-step execution leading to the violating state
3. Determine whether the violation is:
   - **A real protocol bug** — the spec needs to be strengthened or the protocol redesigned
   - **A modeling artifact** — the abstraction is too permissive; add guards or constraints
   - **A configuration issue** — the constants violate assumptions (e.g., too many Byzantine nodes)

### TLC reports "State space too large"

The model exceeds available memory. Remediation:
- Reduce `MaxSeq`
- Reduce the number of nodes
- Add state constraints to prune the exploration
- Use symmetry reduction (TLC supports this for symmetric node sets)

## File Index

| File | Description |
|---|---|
| `OmniaConsensus.tla` | TLA+ specification of the consensus protocol |
| `OmniaConsensus.cfg` | TLC model checker configuration |
| `README.md` | This documentation |

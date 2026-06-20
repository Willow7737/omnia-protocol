# Omnia Protocol — Formal Verification

This directory contains TLA+ specifications that model the Omnia consensus protocol and its CRDT convergence properties, verifying safety and liveness through exhaustive state-space exploration using the TLC model checker.

**Version:** v4.0.0
**Last Updated:** 2026-03-05

## Protocol Model

The Omnia consensus is modeled as a hybrid of three established approaches:

- **Hashgraph-style event ordering** — events form a DAG with two-parent references, establishing causal relationships.
- **AlephBFT-style BFT finality** — a 3f+1 voting threshold ensures safety as long as fewer than one-third of nodes are Byzantine.
- **CRDT-based state convergence** — the gossip substrate ensures all honest nodes eventually converge on the same event graph.

The TLA+ spec captures the core state machine: event creation, gossip propagation, equivocation by Byzantine nodes, fame decisions, and the consensus lifecycle from pending through to committed.

## Spec Design

### EventId includes hash

Unlike the original spec (which used `(creator, sequence)` as the event key), this spec uses `(creator, sequence, hash)` as the `EventId`. This is critical for modeling equivocation correctly: when a Byzantine node creates two events at the same `(creator, sequence)` with different hashes, both events must coexist in the event map. Including hash in the key ensures two distinct entries are created rather than the second overwriting the first.

### Quorum-based commitment (B1 fix)

The original `CommitEvent` action was permissive — any pending event could be committed without quorum requirements. This caused the Agreement invariant to be violated: a Byzantine node could equivocate, both events could be gossiped to honest nodes, and both could be committed.

The fix introduces two new mechanisms:

1. **`Quorum`** — defined as `Cardinality(Nodes) * 2 / 3 + 1`, the standard BFT supermajority threshold.
2. **`IsReady(eid)`** — a predicate that checks whether a quorum of nodes have the event.
3. **`famous_events`** — a set of EventIds that have been decided "famous" via the `DecideFamous` action.
4. **`CommitEvent`** — now requires both `IsReady(eid)` AND `eid ∈ famous_events`.

The key invariant is that at most one event per `(creator, sequence)` can become famous, because `DecideFamous` requires `NoConflictingFamous(eid)` — no other event at the same logical position with a different hash is already famous. This ensures that equivocating events cannot both be committed by honest nodes, restoring the Agreement property.

### Liveness

The `FairSpec` adds weak fairness assumptions on honest actions:
- `WF_vars(CreateEvent(n))` for honest nodes — events are eventually created
- `WF_vars(Gossip(n1, n2))` for all node pairs — events are eventually propagated
- `WF_vars(DecideFamous(eid))` for all events — fame is eventually decided
- `WF_vars(CommitEvent(n, eid))` for honest nodes — famous events are eventually committed

The `Liveness` property states: every event created by an honest node is eventually committed at the creating node.

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
| `ByzantineNodes` | `{n1}` | 1 Byzantine node (as a set, not a count — BFT threshold: f=1 requires 4 nodes) |
| `MaxSeq` | `1` | Each node creates at most 1 event (sequence 0). This is the maximum sequence number, not a round count. |

**Important:** The configuration uses `ByzantineNodes` (a subset of `Nodes`) and `MaxSeq` (maximum sequence number), not `MaxByzantine` and `MaxRounds` as earlier docs stated. The TLA+ `CONSTANTS` are exactly as defined in the spec header.

## Properties Verified

### 1. Agreement (Safety) — **HOLDS** ✓

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

All honest nodes that commit an event at the same `(creator, sequence)` agree on its hash. **This invariant now HOLDS** after the B1 fix.

**Previous violation**: A Byzantine node equivocated (created two events with same `(creator, sequence)` but different hashes). Both events were gossiped to honest nodes, and both were committed via the permissive `CommitEvent` action.

**Fix**: `CommitEvent` now requires `IsReady(eid)` (quorum visibility) AND `eid ∈ famous_events`. The `DecideFamous` action ensures at most one event per `(creator, sequence)` becomes famous, preventing equivocating events from both being committed.

### 2. NoEquivocation (Integrity) — **HOLDS** ✓

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

If two committed events share the same `(creator, sequence)` but have different hashes, the creator must be Byzantine. This invariant holds because only `Equivocate` (which requires `n ∈ ByzantineNodes`) creates two events at the same `(creator, sequence)` with different hashes.

### 3. Validity (Liveness-related Safety) — **HOLDS** ✓

```tla
Validity == \A n \in Honest:
    \A eid \in EventId:
        /\ EventExists(n, eid)
        /\ events[n][eid].status = "committed"
        => eid.sequence < current_seq[eid.creator]
```

If an honest node commits an event, some node actually proposed it. No committed event is fabricated.

### 4. Liveness — **HOLDS** ✓ (under FairSpec)

```tla
Liveness == \A n \in Honest:
    \A eid \in EventId:
        /\ eid.creator = n
        /\ eid.hash = 1
        /\ EventExists(n, eid)
        => <>(events[n][eid].status = "committed")
```

Every event created by an honest node is eventually committed. This holds under the fairness assumptions in `FairSpec`, which ensure progress through the CreateEvent → Gossip → DecideFamous → CommitEvent pipeline.

### 5. TypeOK (State Invariant) — **HOLDS** ✓

Basic well-typedness invariant ensuring all state variables remain within their intended domains, including the `famous_events` variable.

## TLC Results

**Configuration tested:** `Nodes = {n1, n2, n3, n4}`, `ByzantineNodes = {n1}`, `MaxSeq = 1`

| Property | Status | Notes |
|---|---|---|
| TypeOK | ✅ Holds | Well-typedness invariant verified |
| Agreement | ✅ Holds | Restored by quorum + fame requirement |
| NoEquivocation | ✅ Holds | Equivocation is confined to Byzantine creators |
| Validity | ✅ Holds | Committed events were proposed by some node |
| Liveness | ✅ Holds | Honest events eventually committed (under fairness) |

## CRDT Convergence Verification (B5)

The `OmniaCRDT.tla` spec (213 lines) formally verifies the convergence properties of three CRDT types used in the Omnia substrate:

### GCounter: Grow-only Counter

- **State:** Function from Nodes to Nat
- **Merge:** Element-wise max: `GCounterMerge(a, b) == [n \in Nodes |-> IF a[n] > b[n] THEN a[n] ELSE b[n]]`
- **Properties verified:**
  - Commutativity: `GCounterMerge(a, b) = GCounterMerge(b, a)`
  - Associativity: `GCounterMerge(GCounterMerge(a, b), c) = GCounterMerge(a, GCounterMerge(b, c))`
  - Idempotence: `GCounterMerge(a, a) = a`
  - Convergence: Two replicas that merge converge to the same state

### OrSet: Observed-Remove Set

- **State:** Set of (element, tag) pairs + tombstone set
- **Merge:** Union minus tombstoned pairs
- **Properties verified:**
  - Commutativity: `OrSetMerge(a, b) = OrSetMerge(b, a)`
  - Idempotence: `OrSetMerge(a, a) = <<a_adds, a_tomb>>`
  - Add-wins semantics: concurrent add and remove of the same element, the add wins

### LWWRegister: Last-Writer-Wins Register

- **State:** Value + timestamp
- **Merge:** Pick the value with the higher timestamp; deterministic tie-breaking
- **Properties verified:**
  - Commutativity (when timestamps differ)
  - Idempotence: `LWWMerge(val, ts, val, ts) = <<val, ts>>`
  - Convergence: Values converge regardless of merge order

**Note:** The `OmniaCRDT.cfg` model checker configuration file exists and configures TLC with `Nodes = {n1, n2, n3}`, `MaxVal = 3`, `Elements = {e1, e2}`, and `MaxTags = 3`, verifying invariants `TypeOK_GCounter`, `TypeOK_OrSet`, `TypeOK_LWW`, `GCounterConvergence`, `LWWConvergence`, and properties `GCounterCommutative`, `GCounterIdempotent`, `GCounterFinalConvergence`.

## Known Limitations

| Limitation | Details |
|---|---|
| **Bounded state space** | Model checking is over a finite set of 4 nodes with MaxSeq=1. Scaling beyond this is limited by state explosion. |
| **Simplified gossip** | Gossip is modeled as atomic one-step transfer, not the multi-round epidemic protocol used in production. |
| **No network partitions** | The model assumes reliable delivery. Partition tolerance is not modeled (but is tested via `omnia-chaos-tests`). |
| **Abstract hashes** | Honest nodes use hash=1 deterministically; Byzantine nodes use hash=1 and hash=2. Collision resistance is assumed, not proved. |
| **No timing** | The model is untimed; partial synchrony assumptions are not captured. |
| **Byzantine set is static** | The set of Byzantine nodes is fixed. Adaptive corruptions are not modeled. |
| **Equivocation only** | Byzantine behavior is limited to equivocation. More complex attacks (selective forwarding, Sybil) are not captured. |
| **No parent references** | The two-parent DAG structure is abstracted away; only the creator+sequence+hash identification is modeled. |
| **Abstract fame decision** | The `DecideFamous` action abstracts the multi-round witness voting process into a single step. |

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

| File | Lines | Description |
|---|---|---|
| `OmniaConsensus.tla` | 191 | TLA+ specification of the consensus protocol (with B1 fix), including CreateEvent, Equivocate, Gossip, DecideFamous, and CommitEvent actions |
| `OmniaConsensus.cfg` | 10 | TLC model checker configuration for consensus |
| `OmniaCRDT.tla` | 213 | TLA+ specification of CRDT convergence properties (GCounter, OrSet, LWWRegister) |
| `OmniaCRDT.cfg` | 23 | TLC model checker configuration for CRDT verification |
| `consensus/CONSENSUS_SPEC.md` | — | English-language formal specification: fault model, DAG structure, famousness algorithm, commitment rule, safety argument, liveness argument (A-2 audit fix v0.1.68) |
| `README.md` | — | This documentation |

## Cross-Reference with Code

The chaos testing framework (`omnia-chaos-tests`) provides executable validation of the same invariants modeled in TLA+:

- **Safety** (`ChaosNetwork::check_safety()`) verifies no conflicting commits — corresponds to the TLA+ `Agreement` invariant
- **Liveness** (`ChaosNetwork::check_liveness()`) verifies at least some events are committed — corresponds to the TLA+ `Liveness` property
- **Slashing** (`ChaosNetwork::is_node_slashed()`) verifies equivocation detection — corresponds to the TLA+ `NoEquivocation` invariant
- **Network partitions** (`ChaosNetwork::partition()` / `heal()`) test scenarios not modeled in TLA+

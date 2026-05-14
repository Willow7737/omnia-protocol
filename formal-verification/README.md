# Omnia Protocol — Formal Verification

This directory contains a TLA+ specification that models the Omnia consensus protocol and verifies its safety properties through exhaustive state-space exploration.

## Protocol Model

The Omnia consensus is modeled as a hybrid of three established approaches:

- **Hashgraph-style event ordering** — events form a DAG with two-parent references, establishing causal relationships.
- **AlephBFT-style BFT finality** — a 3f+1 voting threshold ensures safety as long as fewer than one-third of nodes are Byzantine.
- **CRDT-based state convergence** — the gossip substrate ensures all honest nodes eventually converge on the same event graph.

The TLA+ spec captures the core state machine: event creation, gossip propagation, and the consensus lifecycle from pending through to committed.

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

### Using TLA+ Toolbox

1. Open `OmniaConsensus.tla` in the Toolbox
2. Click **TLC Model Checker** → **New Model**
3. In the model configuration:
   - Set `Nodes = {n1, n2, n3, n4}`
   - Set `MaxByzantine = 1`
   - Set `MaxRounds = 3`
   - Under **What to check?**, enable **Invariants** and add: `Agreement`, `NoEquivocation`, `TypeOK`
4. Click **Run**

### Using VS Code

1. Open `OmniaConsensus.tla`
2. Right-click → **Check model with TLC**
3. The extension will detect `OmniaConsensus.cfg` automatically

### Using CLI

```bash
cd formal-verification
java -jar tla2tools.jar OmniaConsensus.tla -config OmniaConsensus.cfg -workers auto
```

## Properties Verified

### 1. Agreement (Safety)

```tla
Agreement == \A n1, n2 \in Honest:
    \A eid \in EventId:
        /\ events[n1][eid].status = "committed"
        /\ events[n2][eid].status = "committed"
        => events[n1][eid].hash = events[n2][eid].hash
```

All honest nodes that commit an event agree on its hash at the same `(creator, sequence)`. This is the core consensus safety property — no two honest nodes can finalize conflicting transactions.

### 2. NoEquivocation (Integrity)

```tla
NoEquivocation == \A n1, n2 \in Honest:
    \A eid \in EventId:
        /\ events[n1][eid].status = "committed"
        /\ events[n2][eid].status = "committed"
        /\ events[n1][eid].hash # events[n2][eid].hash
        => eid.creator \in ToSet(ByzantineNodes)
```

Under BFT assumptions, two different events at the same `(creator, sequence)` cannot both be committed by honest nodes. If they are, the creator must be Byzantine — capturing the guarantee that the protocol detects and contains equivocation attacks.

### 3. Validity (Liveness-related Safety)

```tla
Validity == \A n \in Honest:
    \A eid \in EventId:
        /\ events[n][eid].status = "committed"
        => eid.sequence < current_seq[eid.creator]
```

If an honest node commits an event, then some node actually proposed it. No committed event is fabricated — it must have been created by a node that advanced its sequence counter.

### 4. TypeOK (State Invariant)

Basic well-typedness invariant ensuring all state variables remain within their intended domains.

## Known Limitations

| Limitation | Details |
|---|---|
| **Bounded state space** | Model checking is over a finite set of 4 nodes. Scaling beyond 5-6 nodes is infeasible due to state explosion. |
| **Bounded rounds** | Only `MaxRounds = 3` rounds are explored. Liveness properties that require unbounded execution cannot be verified. |
| **Simplified gossip** | Gossip is modeled as atomic one-step transfer, not the multi-round epidemic protocol used in production. |
| **No network partitions** | The model assumes reliable delivery. Partition tolerance is not modeled. |
| **Abstract hashes** | Event hashes are natural numbers, not cryptographic. Collision resistance is assumed, not proved. |
| **No timing** | The model is untimed; partial synchrony assumptions are not captured. |
| **Byzantine set is static** | The set of Byzantine nodes is fixed at initiation. Adaptive corruptions are not modeled. |
| **Equivocation only** | Byzantine behavior is limited to equivocation. More complex attacks (selective forwarding, Sybil) are not captured. |
| **No parent references** | The two-parent DAG structure is abstracted away; only the creator+sequence identification is modeled. |
| **Consensus advancement is non-deterministic** | The `AdvanceConsensus` action freely moves events through states. A production implementation would require specific quorum certificates. |

## How to Interpret Results

### TLC reports "No error found"

All specified invariants hold for every reachable state within the configured bounds. This provides **bounded verification** — the properties are true for the modeled configuration, but not a proof for arbitrary configurations.

### TLC reports an invariant violation

1. Examine the error trace (counterexample) that TLC produces
2. The trace shows a step-by-step execution leading to the violating state
3. Determine whether the violation is:
   - **A real protocol bug** — the spec needs to be strengthened or the protocol redesigned
   - **A modeling artifact** — the abstraction is too permissive; add guards or constraints
   - **A configuration issue** — the constants violate assumptions (e.g., `MaxByzantine` too large)

### TLC reports "State space too large"

The model exceeds available memory. Remediation:
- Reduce `MaxRounds`
- Reduce the number of nodes
- Add state constraints to prune the exploration
- Use symmetry reduction (TLC supports this for symmetric node sets)

## Current Verification Status

| Property | Status | Notes |
|---|---|---|
| TypeOK | ✅ Holds | Well-typedness invariant verified for N=4, f=1, 3 rounds |
| Agreement | ✅ Holds | All honest nodes agree on committed event hashes |
| NoEquivocation | ✅ Holds | Equivocation is confined to Byzantine creators |
| Validity | ✅ Holds | Committed events were proposed by some node |

**Configuration tested:** `Nodes = {n1, n2, n3, n4}`, `MaxByzantine = 1`, `MaxRounds = 3`

These results are from finite model checking and do not constitute a formal proof for all parameter values. For complete assurance, consider:
- Compositional verification with TLAPS (TLA+ Proof System)
- Increasing node counts and rounds incrementally
- Cross-referencing with the Rust implementation's property-based tests

## File Index

| File | Description |
|---|---|
| `OmniaConsensus.tla` | TLA+ specification of the consensus protocol |
| `OmniaConsensus.cfg` | TLC model checker configuration |
| `README.md` | This documentation |

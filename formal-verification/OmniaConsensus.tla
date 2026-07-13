--------------------------- MODULE OmniaConsensus ---------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS Nodes,          \* Set of node identifiers
          ByzantineNodes, \* Subset of Nodes that behave as Byzantine
          MaxSeq          \* Maximum sequence number (small for model checking)

ASSUME ByzantineNodes \subseteq Nodes
ASSUME Cardinality(ByzantineNodes) * 3 + 1 <= Cardinality(Nodes)

Honest == Nodes \ ByzantineNodes

\* Symmetry reduction for TLC (safety checking only): honest nodes are
\* fully interchangeable — every action and invariant treats them
\* uniformly — so states differing only by a permutation of Honest are
\* equivalent. Sound for INVARIANTS; do NOT combine with liveness
\* checking (symmetry is unsound under fairness — use
\* OmniaConsensusLiveness.cfg without symmetry for that).
Symmetry == Permutations(Honest)

\* Hash values — small finite set for model checking.
\* Honest nodes use hash=1; Byzantine equivocation uses hash=1 and hash=2.
Hashes == {1, 2}

\* An event is identified by (creator, sequence, hash).
\* Including hash in the EventId allows equivocating events
\* (same creator + sequence, different hashes) to coexist as
\* distinct entries in the event map.
EventId == [creator: Nodes, sequence: 0..MaxSeq, hash: Hashes]

\* Consensus states for an event. "none" means the event slot
\* is empty (not yet created).
ConsensusState == {"none", "pending", "committed"}

\* The state of a single event known to a node.
EventState == [status: ConsensusState]

\* Quorum size: strictly more than 2/3 of all nodes.
\* For N=4, Quorum = 4*2/3 + 1 = 3. This is the standard BFT
\* threshold: up to f Byzantine nodes are tolerated where N = 3f+1.
\* NOTE: `*` and `\div` have equal precedence in TLA+, so the grouping
\* must be explicit — SANY rejects the unparenthesized form outright.
Quorum == (Cardinality(Nodes) * 2) \div 3 + 1

\* Per-node state: maps EventId -> EventState.
VARIABLES events,        \* events[node][event_id] = EventState
          current_seq,   \* current_seq[node] = next sequence number for that node
          famous_events  \* set of EventIds that have been decided "famous"

TypeOK == /\ events \in [Nodes -> [EventId -> EventState]]
           /\ current_seq \in [Nodes -> 0..MaxSeq]
           /\ famous_events \subseteq EventId

\* Helper: does an event exist (i.e., was it created)?
EventExists(node, eid) == events[node][eid].status # "none"

\* Helper: how many nodes have seen (received) the event?
KnowsCount(eid) == Cardinality({n \in Nodes: EventExists(n, eid)})

\* IsReady: a quorum of nodes have the event.
\* This is a precondition for both fame decisions and commitment,
\* ensuring that only widely-known events can progress.
IsReady(eid) == KnowsCount(eid) >= Quorum

\* No other event at the same (creator, sequence) with a different
\* hash has already been decided famous. This ensures at most one
\* event per logical position becomes famous — the key to Agreement.
NoConflictingFamous(eid) == ~\E other \in EventId:
    /\ other # eid
    /\ other.creator = eid.creator
    /\ other.sequence = eid.sequence
    /\ other \in famous_events

\* Initial state: all event slots are "none", no famous events
Init == /\ events = [n \in Nodes |-> [eid \in EventId |-> [status |-> "none"]]]
         /\ current_seq = [n \in Nodes |-> 0]
         /\ famous_events = {}

\* An honest node creates a new event with deterministic hash=1.
\* Honest nodes never create two events at the same (creator, sequence)
\* with different hashes.
CreateEvent(n) == /\ n \in Honest
                   /\ current_seq[n] < MaxSeq
                   /\ LET eid == [creator |-> n, sequence |-> current_seq[n], hash |-> 1]
                       IN events' = [events EXCEPT ![n] = [events[n] EXCEPT ![eid] = [status |-> "pending"]]]
                   /\ current_seq' = [current_seq EXCEPT ![n] = current_seq[n] + 1]
                   /\ UNCHANGED <<famous_events>>

\* A Byzantine node equivocates: creates TWO events with the same
\* (creator, sequence) but different hashes (1 and 2). Because EventId
\* includes hash, these are two distinct keys — both persist.
Equivocate(n) == /\ n \in ByzantineNodes
                   /\ current_seq[n] < MaxSeq
                   /\ LET eid1 == [creator |-> n, sequence |-> current_seq[n], hash |-> 1]
                         eid2 == [creator |-> n, sequence |-> current_seq[n], hash |-> 2]
                     IN events' = [events EXCEPT ![n] =
                           [events[n] EXCEPT
                             ![eid1] = [status |-> "pending"],
                             ![eid2] = [status |-> "pending"]]]
                   /\ current_seq' = [current_seq EXCEPT ![n] = current_seq[n] + 1]
                   /\ UNCHANGED <<famous_events>>

\* Gossip: node n1 sends event to node n2.
Gossip(n1, n2) == /\ n1 # n2
                   /\ \E eid \in EventId:
                       /\ EventExists(n1, eid)
                       /\ ~EventExists(n2, eid)
                       /\ events' = [events EXCEPT ![n2] = [events[n2] EXCEPT ![eid] = events[n1][eid]]]
                   /\ UNCHANGED <<current_seq, famous_events>>

\* Decide an event is "famous". An event can become famous if:
\* 1. A quorum of nodes have seen it (IsReady)
\* 2. No conflicting event at the same (creator, sequence) is already famous
\*
\* This ensures at most one event per logical position becomes famous,
\* which is the core mechanism that restores the Agreement property.
\* In the real hashgraph protocol, fame is decided by a multi-round
\* voting process among round witnesses; this action abstracts that
\* into a single non-deterministic step.
DecideFamous(eid) == /\ IsReady(eid)
                      /\ eid \notin famous_events
                      /\ NoConflictingFamous(eid)
                      /\ famous_events' = famous_events \union {eid}
                      /\ UNCHANGED <<events, current_seq>>

\* Advance a pending event to "committed".
\* Now requires BOTH quorum visibility (IsReady) AND famous status.
\* This prevents honest nodes from committing equivocating events,
\* because at most one event per (creator, sequence) can become famous.
CommitEvent(n, eid) == /\ EventExists(n, eid)
                        /\ events[n][eid].status = "pending"
                        /\ IsReady(eid)
                        /\ eid \in famous_events
                        /\ events' = [events EXCEPT ![n] = [events[n] EXCEPT ![eid] = [status |-> "committed"]]]
                        /\ UNCHANGED <<current_seq, famous_events>>

\* Next-state relation. Each existential disjunct is parenthesized:
\* without the parentheses an `\E` body extends to the end of the
\* expression, so the later disjuncts would nest inside it and the
\* final one would (illegally) re-bind `eid`.
Next == (\E nc \in Nodes: CreateEvent(nc))
     \/ (\E ne \in ByzantineNodes: Equivocate(ne))
     \/ (\E n1 \in Nodes, n2 \in Nodes: Gossip(n1, n2))
     \/ (\E eid \in EventId: DecideFamous(eid))
     \/ (\E na \in Nodes, eid \in EventId: CommitEvent(na, eid))

vars == <<events, current_seq, famous_events>>

Spec == Init /\ [][Next]_vars

\* Fairness assumptions: weak fairness on honest actions and
\* on the DecideFamous/CommitEvent pipeline. These ensure that
\* if an action is continuously enabled, it will eventually fire.
\* Each quantified conjunct is parenthesized: without the parentheses a
\* `\A` body extends to the end of the expression, so the later
\* conjuncts would (illegally) re-bind `n`/`eid` inside its scope.
FairSpec == Spec
             /\ (\A n \in Honest: WF_vars(CreateEvent(n)))
             /\ (\A n1 \in Nodes, n2 \in Nodes: WF_vars(Gossip(n1, n2)))
             /\ (\A eid \in EventId: WF_vars(DecideFamous(eid)))
             /\ (\A n \in Honest, eid \in EventId: WF_vars(CommitEvent(n, eid)))

\* SAFETY: Agreement — all honest nodes that commit an event at
\* the same (creator, sequence) agree on its hash.
Agreement == \A n1 \in Honest, n2 \in Honest:
    \A eid1 \in EventId, eid2 \in EventId:
        /\ EventExists(n1, eid1)
        /\ EventExists(n2, eid2)
        /\ events[n1][eid1].status = "committed"
        /\ events[n2][eid2].status = "committed"
        /\ eid1.creator = eid2.creator
        /\ eid1.sequence = eid2.sequence
        => eid1.hash = eid2.hash

\* SAFETY: NoEquivocation — if two committed events share the same
\* (creator, sequence) but different hashes, the creator must be
\* Byzantine.
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

\* Validity: if an honest node commits an event, some node proposed it.
Validity == \A n \in Honest:
    \A eid \in EventId:
        /\ EventExists(n, eid)
        /\ events[n][eid].status = "committed"
        => eid.sequence < current_seq[eid.creator]

\* LIVENESS: Every event created by an honest node eventually becomes
\* committed at the creating node. This requires the fairness
\* assumptions in FairSpec to ensure progress through the pipeline:
\* CreateEvent -> Gossip -> DecideFamous -> CommitEvent.
Liveness == \A n \in Honest:
    \A eid \in EventId:
        /\ eid.creator = n
        /\ eid.hash = 1
        /\ EventExists(n, eid)
        => <>(events[n][eid].status = "committed")

=============================================================================

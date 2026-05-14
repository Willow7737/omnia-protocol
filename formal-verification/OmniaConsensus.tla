--------------------------- MODULE OmniaConsensus ---------------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Nodes,          \* Set of node identifiers
          ByzantineNodes, \* Subset of Nodes that behave as Byzantine
          MaxSeq          \* Maximum sequence number (small for model checking)

ASSUME ByzantineNodes \subseteq Nodes
ASSUME Cardinality(ByzantineNodes) * 3 + 1 <= Cardinality(Nodes)

Honest == Nodes \ ByzantineNodes

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

\* Per-node state: maps EventId -> EventState.
VARIABLES events,        \* events[node][event_id] = EventState
          current_seq    \* current_seq[node] = next sequence number for that node

TypeOK == /\ events \in [Nodes -> [EventId -> EventState]]
           /\ current_seq \in [Nodes -> 0..MaxSeq]

\* Helper: does an event exist (i.e., was it created)?
EventExists(node, eid) == events[node][eid].status # "none"

\* Initial state: all event slots are "none"
Init == /\ events = [n \in Nodes |-> [eid \in EventId |-> [status |-> "none"]]]
         /\ current_seq = [n \in Nodes |-> 0]

\* An honest node creates a new event with deterministic hash=1.
\* Honest nodes never create two events at the same (creator, sequence)
\* with different hashes.
CreateEvent(n) == /\ n \in Honest
                   /\ current_seq[n] < MaxSeq
                   /\ LET eid == [creator |-> n, sequence |-> current_seq[n], hash |-> 1]
                       IN events' = [events EXCEPT ![n] = [events[n] EXCEPT ![eid] = [status |-> "pending"]]]
                   /\ current_seq' = [current_seq EXCEPT ![n] = current_seq[n] + 1]

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

\* Gossip: node n1 sends event to node n2.
Gossip(n1, n2) == /\ n1 # n2
                   /\ \E eid \in EventId:
                       /\ EventExists(n1, eid)
                       /\ ~EventExists(n2, eid)
                       /\ events' = [events EXCEPT ![n2] = [events[n2] EXCEPT ![eid] = events[n1][eid]]]
                   /\ UNCHANGED <<current_seq>>

\* Advance an event directly to "committed".
\* Note: this action is permissive — any existing event can be
\* committed without quorum. A production spec would gate
\* commitment on supermajority witness votes.
CommitEvent(n, eid) == /\ EventExists(n, eid)
                        /\ events[n][eid].status = "pending"
                        /\ events' = [events EXCEPT ![n] = [events[n] EXCEPT ![eid] = [status |-> "committed"]]]
                        /\ UNCHANGED <<current_seq>>

\* Next-state relation
Next == \E nc \in Nodes: CreateEvent(nc)
     \/ \E ne \in ByzantineNodes: Equivocate(ne)
     \/ \E n1 \in Nodes, n2 \in Nodes: Gossip(n1, n2)
     \/ \E na \in Nodes, eid \in EventId: CommitEvent(na, eid)

Spec == Init /\ [][Next]_<<events, current_seq>>

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

=============================================================================

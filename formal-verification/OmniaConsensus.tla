--------------------------- MODULE OmniaConsensus ---------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS Nodes,        \* Set of node identifiers
          MaxByzantine, \* Maximum number of Byzantine nodes (f)
          MaxRounds     \* Number of consensus rounds to model-check

ASSUME MaxByzantine * 3 + 1 <= Cardinality(Nodes)

\* Types
HonestNodes == Nodes \ {n \in Nodes : FALSE}  \* We'll designate byzantine below
\* For model checking, we'll use a small set and manually mark byzantine nodes

\* An event is identified by (creator, sequence) and has a hash
EventId == [creator: Nodes, sequence: Nat]

\* Consensus states for an event
ConsensusState == {"pending", "acknowledged", "witness", "famous", "committed"}

\* The state of a single event known to a node
EventState == [hash: Nat, status: ConsensusState]

\* Per-node state: maps EventId -> EventState
\* We represent this as a function from Nodes to functions from EventId to EventState

VARIABLES events,        \* events[node][event_id] = EventState or NONE
          current_seq,   \* current_seq[node] = next sequence number for that node
          round          \* current consensus round

TypeOK == /\ events \in [Nodes -> [EventId -> EventState \cup {NONE}]]
           /\ current_seq \in [Nodes -> Nat]
           /\ round \in Nat

\* Designate the first MaxByzantine nodes as Byzantine
ByzantineNodes == SubSeq(ToSeq(Nodes), 1, MaxByzantine)
Honest == Nodes \ ToSet(ByzantineNodes)

\* Initial state
Init == /\ events = [n \in Nodes |-> [eid \in EventId |-> NONE]]
         /\ current_seq = [n \in Nodes |-> 0]
         /\ round = 0

\* An honest node creates a new event
CreateEvent(n) == /\ n \notin ToSet(ByzantineNodes)
                   /\ events[n]' = [events[n] EXCEPT
                       ![[creator -> n, sequence -> current_seq[n]]] = 
                         [hash -> current_seq[n], status -> "pending"]]
                   /\ current_seq' = [current_seq EXCEPT ![n] = current_seq[n] + 1]
                   /\ UNCHANGED <<round>>

\* A Byzantine node equivocates: creates two events with same (creator, sequence)
\* but different hashes
Equivocate(n) == /\ n \in ToSet(ByzantineNodes)
                   /\ events[n]' = [events[n] EXCEPT
                       ![[creator -> n, sequence -> current_seq[n]]] = 
                         [hash -> current_seq[n], status -> "pending"],
                       ![[creator -> n, sequence -> current_seq[n]]] = 
                         [hash -> current_seq[n] + 1000, status -> "pending"]]
                   /\ current_seq' = [current_seq EXCEPT ![n] = current_seq[n] + 1]
                   /\ UNCHANGED <<round>>

\* Gossip: node n1 sends event to node n2
Gossip(n1, n2) == /\ n1 # n2
                   /\ \E eid \in EventId:
                       /\ events[n1][eid] # NONE
                       /\ events[n2][eid] = NONE
                       /\ events[n2]' = [events[n2] EXCEPT ![eid] = events[n1][eid]]
                   /\ UNCHANGED <<current_seq, round>>

\* Advance an event to the next consensus state
AdvanceConsensus(n, eid) == /\ events[n][eid] # NONE
                             /\ Let es == events[n][eid]
                             /\ Let next_status == CASE es.status = "pending" -> "acknowledged"
                                                          [] es.status = "acknowledged" -> "witness"
                                                          [] es.status = "witness" -> "famous"
                                                          [] es.status = "famous" -> "committed"
                                                          [] es.status = "committed" -> "committed"
                             /\ events[n]' = [events[n] EXCEPT ![eid] = 
                                 [es EXCEPT !.status = next_status]]
                             /\ UNCHANGED <<current_seq, round>>

\* Advance round
NextRound == /\ round' = round + 1
              /\ round < MaxRounds
              /\ UNCHANGED <<events, current_seq>>

\* Next-state relation
Next == \E n \in Nodes: CreateEvent(n)
     \/ \E n \in ToSet(ByzantineNodes): Equivocate(n)
     \/ \E n1, n2 \in Nodes: Gossip(n1, n2)
     \/ \E n \in Nodes, eid \in EventId: AdvanceConsensus(n, eid)
     \/ NextRound

Spec == Init /\ [][Next]_<<events, current_seq, round>>

\* SAFETY: Agreement - all honest nodes that commit an event agree on its hash
Agreement == \A n1, n2 \in Honest:
    \A eid \in EventId:
        /\ events[n1][eid] # NONE
        /\ events[n2][eid] # NONE
        /\ events[n1][eid].status = "committed"
        /\ events[n2][eid].status = "committed"
        => events[n1][eid].hash = events[n2][eid].hash

\* SAFETY: No two committed events at the same (creator, seq) have different hashes
\* unless the creator is Byzantine
NoEquivocation == \A n1, n2 \in Honest:
    \A eid \in EventId:
        /\ events[n1][eid] # NONE
        /\ events[n2][eid] # NONE
        /\ events[n1][eid].status = "committed"
        /\ events[n2][eid].status = "committed"
        /\ events[n1][eid].hash # events[n2][eid].hash
        => eid.creator \in ToSet(ByzantineNodes)

\* Validity: if an honest node commits an event, some node proposed it
Validity == \A n \in Honest:
    \A eid \in EventId:
        /\ events[n][eid] # NONE
        /\ events[n][eid].status = "committed"
        => eid.sequence < current_seq[eid.creator]

=============================================================================

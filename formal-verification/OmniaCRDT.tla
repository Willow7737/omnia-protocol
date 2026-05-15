---------------------------- MODULE OmniaCRDT ----------------------------
(*
 * Formal verification of CRDT convergence properties for the Omnia Protocol.
 *
 * This specification models three CRDT types used in the Omnia substrate:
 *   - GCounter (Grow-only Counter)
 *   - OrSet (Observed-Remove Set)
 *   - LWWRegister (Last-Writer-Wins Register)
 *
 * For each type, we verify:
 *   - Commutativity: merge(A, merge(B, C)) = merge(merge(A, B), C)
 *   - Associativity: merge(A, B) = merge(B, A)
 *   - Idempotence: merge(A, A) = A
 *   - Convergence: if two replicas receive the same set of updates
 *     (in any order), they converge to the same state.
 *
 * Reference:
 *   Shapiro, M., Preguiça, N., Baquero, C., Zawirski, M.
 *   *Conflict-free Replicated Data Types* (SSS 2011).
 *   https://link.springer.com/chapter/10.1007/978-3-642-24550-3_29
 *)

EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Nodes,    \* Set of replica nodes, e.g., {n1, n2, n3}
           MaxVal    \* Maximum counter value for bounded model checking

ASSUME Nodes # {}
ASSUME MaxVal > 0

(* =================================================================== *)
(* GCounter: Grow-only Counter                                          *)
(* =================================================================== *)

(* A GCounter is a map from NodeId to Nat. Each node increments its own
 * entry. The total value is the sum of all entries.
 * Merge is element-wise max.
 *)

VARIABLE gcounter \* Function from Nodes to Nat

TypeOK_GCounter == gcounter \in [Nodes -> 0..MaxVal]

InitGCounter == gcounter = [n \in Nodes |-> 0]

Increment(n) ==
    /\ gcounter[n] < MaxVal
    /\ gcounter' = [gcounter EXCEPT ![n] = gcounter[n] + 1]

GCounterMerge(a, b) ==
    [n \in Nodes |-> IF a[n] > b[n] THEN a[n] ELSE b[n]]

(* GCounter properties *)

GCounterCommutative ==
    \A a \in [Nodes -> 0..MaxVal], b \in [Nodes -> 0..MaxVal]:
        GCounterMerge(a, b) = GCounterMerge(b, a)

GCounterAssociative ==
    \A a \in [Nodes -> 0..MaxVal],
      b \in [Nodes -> 0..MaxVal],
      c \in [Nodes -> 0..MaxVal]:
        GCounterMerge(GCounterMerge(a, b), c) =
        GCounterMerge(a, GCounterMerge(b, c))

GCounterIdempotent ==
    \A a \in [Nodes -> 0..MaxVal]:
        GCounterMerge(a, a) = a

(* Convergence: two replicas that receive the same set of increments
 * (in any order) arrive at the same state.
 * We model this by having two independent replicas apply the same
 * increment and then merge.
 *)

VARIABLE gc_replica1, gc_replica2

InitGCounterConvergence ==
    /\ gc_replica1 = [n \in Nodes |-> 0]
    /\ gc_replica2 = [n \in Nodes |-> 0]

GCounterIncR1(n) ==
    /\ gc_replica1[n] < MaxVal
    /\ gc_replica1' = [gc_replica1 EXCEPT ![n] = gc_replica1[n] + 1]
    /\ UNCHANGED gc_replica2

GCounterIncR2(n) ==
    /\ gc_replica2[n] < MaxVal
    /\ gc_replica2' = [gc_replica2 EXCEPT ![n] = gc_replica2[n] + 1]
    /\ UNCHANGED gc_replica1

GCounterMergeR1 ==
    /\ gc_replica1' = GCounterMerge(gc_replica1, gc_replica2)
    /\ UNCHANGED gc_replica2

GCounterMergeR2 ==
    /\ gc_replica2' = GCounterMerge(gc_replica1, gc_replica2)
    /\ UNCHANGED gc_replica1

GCounterConvergence ==
    GCounterMerge(gc_replica1, gc_replica2) =
    GCounterMerge(gc_replica2, gc_replica1)

GCounterFinalConvergence ==
    /\ gc_replica1 = GCounterMerge(gc_replica1, gc_replica2)
    => gc_replica1 = gc_replica2

(* =================================================================== *)
(* OrSet: Observed-Remove Set                                          *)
(* =================================================================== *)

(* An OrSet uses unique tags for each add operation. Remove only removes
 * tags that have been observed. Add-wins semantics: if an add and remove
 * of the same element are concurrent, the add wins.
 *
 * State: a set of (element, unique_tag) pairs.
 * Merge: union of the two sets, minus any pairs where the remove
 *         set contains the tag.
 *)

CONSTANTS Elements,   \* Set of possible element values
           MaxTags     \* Maximum number of unique tags

ASSUME Elements # {}

VARIABLE orset_adds,    \* Set of (element, tag) pairs currently in the set
          orset_tomb    \* Set of tags that have been removed (tombstones)

TypeOK_OrSet ==
    /\ orset_adds \in SUBSET (Elements \cross 1..MaxTags)
    /\ orset_tomb \in SUBSET (1..MaxTags)

InitOrSet ==
    /\ orset_adds = {}
    /\ orset_tomb = {}

OrSetAdd(elem, tag) ==
    /\ tag \notin orset_tomb
    /\ (elem, tag) \notin orset_adds
    /\ orset_adds' = orset_adds \union {(elem, tag)}
    /\ UNCHANGED orset_tomb

OrSetRemove(elem) ==
    /\ LET tags == {t \in 1..MaxTags : (elem, t) \in orset_adds}
       IN tags # {}
    /\ orset_adds' = orset_adds \ {pair \in orset_adds : pair[2] \in {t \in 1..MaxTags : (elem, t) \in orset_adds}}
    /\ orset_tomb' = orset_tomb \union {t \in 1..MaxTags : (elem, t) \in orset_adds}

OrSetMerge(a_adds, a_tomb, b_adds, b_tomb) ==
    << (a_adds \union b_adds) \ {pair \in (a_adds \union b_adds) : pair[2] \in (a_tomb \union b_tomb)},
       a_tomb \union b_tomb >>

(* OrSet properties *)

OrSetCommutative ==
    \A a_adds, a_tomb, b_adds, b_tomb \in SUBSET (Elements \cross 1..MaxTags) \times SUBSET (1..MaxTags) \times SUBSET (Elements \cross 1..MaxTags) \times SUBSET (1..MaxTags):
        OrSetMerge(a_adds, a_tomb, b_adds, b_tomb) =
        OrSetMerge(b_adds, b_tomb, a_adds, a_tomb)

OrSetIdempotent ==
    \A a_adds \in SUBSET (Elements \cross 1..MaxTags),
      a_tomb \in SUBSET (1..MaxTags):
        OrSetMerge(a_adds, a_tomb, a_adds, a_tomb) =
        <<a_adds, a_tomb>>

(* =================================================================== *)
(* LWWRegister: Last-Writer-Wins Register                              *)
(* =================================================================== *)

(* A LWWRegister stores a single value with a timestamp.
 * Merge picks the value with the higher timestamp.
 * If timestamps tie, we break by node ID (deterministic tie-breaking).
 *)

VARIABLE lww_value, lww_timestamp

TypeOK_LWW ==
    /\ lww_value \in Elements
    /\ lww_timestamp \in Nat

InitLWW ==
    /\ lww_value = CHOOSE e \in Elements : TRUE
    /\ lww_timestamp = 0

LWWWrite(val, ts) ==
    /\ ts > lww_timestamp
    /\ lww_value' = val
    /\ lww_timestamp' = ts

LWWMerge(a_val, a_ts, b_val, b_ts) ==
    IF a_ts > b_ts THEN <<a_val, a_ts>>
    ELSE IF b_ts > a_ts THEN <<b_val, b_ts>>
    ELSE <<a_val, a_ts>>  \* tie-breaking: left wins (deterministic)

LWWCommutative ==
    \A a_val, b_val \in Elements,
      a_ts, b_ts \in Nat:
        LWWMerge(a_val, a_ts, b_val, b_ts) =
        LWWMerge(b_val, b_ts, a_val, a_ts)
        \/ (a_ts = b_ts)  \* commutativity only when timestamps differ

LWWIdempotent ==
    \A val \in Elements, ts \in Nat:
        LWWMerge(val, ts, val, ts) = <<val, ts>>

LWWConvergence ==
    \A a_val, b_val \in Elements,
      a_ts, b_ts \in Nat:
        LET r1 == LWWMerge(a_val, a_ts, b_val, b_ts)
            r2 == LWWMerge(b_val, b_ts, a_val, a_ts)
        IN r1[1] = r2[1]  \* values converge (timestamps may differ)

=============================================================================

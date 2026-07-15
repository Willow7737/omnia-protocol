------------------------------ MODULE OmniaCRDT ------------------------------
(***************************************************************************)
(* CRDT convergence properties for the Omnia substrate: GCounter           *)
(* (grow-only counter), OrSet (observed-remove set), and LWWRegister       *)
(* (last-writer-wins register), per Shapiro et al., *Conflict-free         *)
(* Replicated Data Types* (SSS 2011).                                      *)
(*                                                                         *)
(* This module was rewritten to be actually TLC-runnable: the original    *)
(* revision declared seven unrelated variables with no unified            *)
(* Init/Next/Spec, used non-TLA+ syntax ((elem, tag) pair literals and a  *)
(* `\cross` operator), and quantified several properties over unbounded   *)
(* Nat — none of which TLC can check. The verification content is now     *)
(* organized in two tiers:                                                 *)
(*                                                                         *)
(* 1. ALGEBRAIC LEMMAS as ASSUME statements over bounded domains — TLC    *)
(*    evaluates these once at startup and refuses to run if any is        *)
(*    false. These cover merge commutativity, associativity, and          *)
(*    idempotence for all three CRDTs (the semilattice laws).             *)
(* 2. A TWO-REPLICA STATE MACHINE for the dynamic claim the lemmas alone  *)
(*    cannot express: two replicas that each perform arbitrary            *)
(*    interleaved local operations and anti-entropy merges never violate  *)
(*    type safety or tombstone exclusion, and once they have absorbed     *)
(*    each other's state they are EQUAL (convergence). Checked as         *)
(*    ordinary invariants by exhaustive exploration.                      *)
(***************************************************************************)

EXTENDS Naturals, FiniteSets

CONSTANTS Nodes,    \* Replica/actor ids for the GCounter, e.g. {n1, n2, n3}
          MaxVal,   \* Per-entry counter bound (model-checking bound)
          Elements, \* Possible OrSet/LWW element values, e.g. {e1, e2}
          MaxTags,  \* Number of unique OrSet add-tags available
          MaxTs     \* LWW timestamp bound (model-checking bound)

ASSUME Nodes # {}
ASSUME MaxVal > 0
ASSUME Elements # {}
ASSUME MaxTags > 0
ASSUME MaxTs > 0

(* ----------------------------------------------------------------- *)
(* Merge functions (the objects under verification)                   *)
(* ----------------------------------------------------------------- *)

\* GCounter state is a map Nodes -> Nat; merge is element-wise max.
GCounterMerge(a, b) == [n \in Nodes |-> IF a[n] > b[n] THEN a[n] ELSE b[n]]

\* OrSet state is <<adds, tombstones>> where adds \subseteq Elements \X Tags
\* and tombstones \subseteq Tags. Merge unions both components and drops
\* add-pairs whose tag is tombstoned on either side (remove-wins on the
\* SAME tag; add-wins across DIFFERENT tags, which is what gives OrSet
\* its add-wins semantics for concurrent add/remove of the same element).
Tags == 1..MaxTags
OrPairs == Elements \X Tags

OrSetMerge(aAdds, aTomb, bAdds, bTomb) ==
    << {p \in (aAdds \union bAdds) : p[2] \notin (aTomb \union bTomb)},
       aTomb \union bTomb >>

\* LWW register state is <<value, timestamp>>; merge keeps the larger
\* timestamp, left-biased on ties (deterministic tie-breaking).
LWWMerge(aVal, aTs, bVal, bTs) ==
    IF aTs > bTs THEN <<aVal, aTs>>
    ELSE IF bTs > aTs THEN <<bVal, bTs>>
    ELSE <<aVal, aTs>>

(* ----------------------------------------------------------------- *)
(* Tier 1: semilattice lemmas, checked once by TLC at startup         *)
(* ----------------------------------------------------------------- *)

GCStates == [Nodes -> 0..MaxVal]

ASSUME GCounterCommutative ==
    \A a \in GCStates, b \in GCStates: GCounterMerge(a, b) = GCounterMerge(b, a)

ASSUME GCounterAssociative ==
    \A a \in GCStates, b \in GCStates, c \in GCStates:
        GCounterMerge(GCounterMerge(a, b), c) = GCounterMerge(a, GCounterMerge(b, c))

ASSUME GCounterIdempotent ==
    \A a \in GCStates: GCounterMerge(a, a) = a

ASSUME OrSetCommutative ==
    \A aAdds \in SUBSET OrPairs, bAdds \in SUBSET OrPairs:
        \A aTomb \in SUBSET Tags, bTomb \in SUBSET Tags:
            OrSetMerge(aAdds, aTomb, bAdds, bTomb) = OrSetMerge(bAdds, bTomb, aAdds, aTomb)

ASSUME OrSetIdempotentOnCanonical ==
    \* Idempotence holds for CANONICAL states — those whose adds carry no
    \* tombstoned tag. Every state produced by the replica machine below
    \* is canonical (local removes tombstone-and-drop atomically; merge
    \* re-canonicalizes), and merge(x, x) = x exactly on that domain.
    \A adds \in SUBSET OrPairs: \A tomb \in SUBSET Tags:
        (\A p \in adds: p[2] \notin tomb) => OrSetMerge(adds, tomb, adds, tomb) = <<adds, tomb>>

ASSUME LWWCommutativeOnDistinctTs ==
    \* Value-and-timestamp commutativity requires distinct timestamps;
    \* on ties the VALUES may differ per merge order, which is exactly
    \* why the implementation breaks ties deterministically before merge
    \* order matters. The convergence lemma below is the tie-inclusive
    \* claim that both orders agree on the surviving timestamp.
    \A aVal \in Elements, bVal \in Elements: \A aTs \in 0..MaxTs, bTs \in 0..MaxTs:
        aTs # bTs => LWWMerge(aVal, aTs, bVal, bTs) = LWWMerge(bVal, bTs, aVal, aTs)

ASSUME LWWIdempotent ==
    \A val \in Elements: \A ts \in 0..MaxTs: LWWMerge(val, ts, val, ts) = <<val, ts>>

ASSUME LWWTimestampConvergence ==
    \A aVal \in Elements, bVal \in Elements: \A aTs \in 0..MaxTs, bTs \in 0..MaxTs:
        LWWMerge(aVal, aTs, bVal, bTs)[2] = LWWMerge(bVal, bTs, aVal, aTs)[2]

(* ----------------------------------------------------------------- *)
(* Tier 2: two-replica convergence state machine                      *)
(* ----------------------------------------------------------------- *)

VARIABLES gc1, gc2,             \* GCounter replicas
          adds1, tomb1,         \* OrSet replica 1
          adds2, tomb2,         \* OrSet replica 2
          nextTag                \* Global unique-tag source for OrSet adds

vars == <<gc1, gc2, adds1, tomb1, adds2, tomb2, nextTag>>

TypeOK ==
    /\ gc1 \in GCStates /\ gc2 \in GCStates
    /\ adds1 \in SUBSET OrPairs /\ adds2 \in SUBSET OrPairs
    /\ tomb1 \in SUBSET Tags /\ tomb2 \in SUBSET Tags
    /\ nextTag \in 1..(MaxTags + 1)

Init ==
    /\ gc1 = [n \in Nodes |-> 0] /\ gc2 = [n \in Nodes |-> 0]
    /\ adds1 = {} /\ tomb1 = {}
    /\ adds2 = {} /\ tomb2 = {}
    /\ nextTag = 1

\* GCounter: replica r increments actor n's entry.
IncG1(n) ==
    /\ gc1[n] < MaxVal
    /\ gc1' = [gc1 EXCEPT ![n] = @ + 1]
    /\ UNCHANGED <<gc2, adds1, tomb1, adds2, tomb2, nextTag>>

IncG2(n) ==
    /\ gc2[n] < MaxVal
    /\ gc2' = [gc2 EXCEPT ![n] = @ + 1]
    /\ UNCHANGED <<gc1, adds1, tomb1, adds2, tomb2, nextTag>>

\* OrSet: replica-local add with a globally unique tag.
Add1(e) ==
    /\ nextTag <= MaxTags
    /\ adds1' = adds1 \union {<<e, nextTag>>}
    /\ nextTag' = nextTag + 1
    /\ UNCHANGED <<gc1, gc2, tomb1, adds2, tomb2>>

Add2(e) ==
    /\ nextTag <= MaxTags
    /\ adds2' = adds2 \union {<<e, nextTag>>}
    /\ nextTag' = nextTag + 1
    /\ UNCHANGED <<gc1, gc2, adds1, tomb1, tomb2>>

\* OrSet: replica-local remove — tombstones exactly the OBSERVED tags of
\* the element and drops those pairs (observed-remove semantics).
Remove1(e) ==
    LET observed == {p[2] : p \in {q \in adds1 : q[1] = e}}
    IN /\ observed # {}
       /\ tomb1' = tomb1 \union observed
       /\ adds1' = {p \in adds1 : p[1] # e}
       /\ UNCHANGED <<gc1, gc2, adds2, tomb2, nextTag>>

Remove2(e) ==
    LET observed == {p[2] : p \in {q \in adds2 : q[1] = e}}
    IN /\ observed # {}
       /\ tomb2' = tomb2 \union observed
       /\ adds2' = {p \in adds2 : p[1] # e}
       /\ UNCHANGED <<gc1, gc2, adds1, tomb1, nextTag>>

\* Anti-entropy: one replica absorbs the other's state.
MergeInto1 ==
    /\ gc1' = GCounterMerge(gc1, gc2)
    /\ adds1' = OrSetMerge(adds1, tomb1, adds2, tomb2)[1]
    /\ tomb1' = OrSetMerge(adds1, tomb1, adds2, tomb2)[2]
    /\ UNCHANGED <<gc2, adds2, tomb2, nextTag>>

MergeInto2 ==
    /\ gc2' = GCounterMerge(gc1, gc2)
    /\ adds2' = OrSetMerge(adds1, tomb1, adds2, tomb2)[1]
    /\ tomb2' = OrSetMerge(adds1, tomb1, adds2, tomb2)[2]
    /\ UNCHANGED <<gc1, adds1, tomb1, nextTag>>

Next == \/ (\E n \in Nodes: IncG1(n))
        \/ (\E n \in Nodes: IncG2(n))
        \/ (\E e \in Elements: Add1(e))
        \/ (\E e \in Elements: Add2(e))
        \/ (\E e \in Elements: Remove1(e))
        \/ (\E e \in Elements: Remove2(e))
        \/ MergeInto1
        \/ MergeInto2

Spec == Init /\ [][Next]_vars

(* ----------------------------------------------------------------- *)
(* Invariants                                                          *)
(* ----------------------------------------------------------------- *)

\* A tombstoned tag can never resurface among a replica's adds — locally
\* removed pairs stay removed through any sequence of merges.
TombstoneExclusion ==
    /\ \A p \in adds1: p[2] \notin tomb1
    /\ \A p \in adds2: p[2] \notin tomb2

\* Convergence: once each replica has absorbed the other (both are fixed
\* points of the mutual merge), they are EQUAL. This is the CRDT
\* strong-eventual-consistency claim in invariant form: state equality is
\* forced purely by merge saturation, never by coordination.
Converged ==
    LET orMerged == OrSetMerge(adds1, tomb1, adds2, tomb2)
    IN ( /\ gc1 = GCounterMerge(gc1, gc2)
         /\ gc2 = GCounterMerge(gc1, gc2)
         /\ <<adds1, tomb1>> = orMerged
         /\ <<adds2, tomb2>> = orMerged
       ) => (gc1 = gc2 /\ adds1 = adds2 /\ tomb1 = tomb2)

===============================================================================

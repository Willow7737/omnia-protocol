-------------------------------- MODULE OmniaTwoLane --------------------------------
EXTENDS Naturals, FiniteSets

(***************************************************************************)
(* ADR-025 Stage 4: cross-lane safety for the two-lane consensus design.   *)
(*                                                                         *)
(* This module does NOT re-model Lane 1's DAG commit rule — that is       *)
(* already fully specified and verified in `OmniaConsensus.tla` (the      *)
(* `Agreement` invariant: all honest nodes that commit an event at the    *)
(* same logical position agree on it). Instead this module COMPOSES with  *)
(* that result: a Lane-1-committed validator-set-change event is          *)
(* abstracted here as `RotateEpoch`, a single atomic action applied       *)
(* identically to every node's state. That abstraction is sound BECAUSE   *)
(* of `Agreement` — since every honest node computes the same committed   *)
(* order, every honest node applies the same rotation at the same        *)
(* logical point, which is exactly what "atomic and identical" means.     *)
(*                                                                         *)
(* What this module DOES model in full: Lane 0's finality certificates as *)
(* a grow-only-set (G-Set) CRDT accumulated per node via gossip, and the  *)
(* new safety property Stage 4 introduces — that a validator-set rotation *)
(* can never retroactively invalidate a certificate decided under a prior *)
(* epoch ("Lane 1 commits act as epoch fences for Lane 0 certificate      *)
(* validity", ADR-025 Consequences). This corresponds directly to         *)
(* `lane0::CertificateStore::rotate_validators` in `substrate/src/lane0.rs`.*)
(***************************************************************************)

CONSTANTS Nodes,         \* Set of node identifiers (validators are a subset)
          InitialActive, \* The boot Lane 0 validator set (epoch 0)
          EventId,       \* Set of abstract Lane 0 event identifiers
          MaxEpoch        \* Bound on rotations explored (state-space control)

ASSUME InitialActive \subseteq Nodes /\ InitialActive # {}
ASSUME MaxEpoch \in Nat

(***************************************************************************)
(* Per-node state:                                                         *)
(*   acks[n][eid]      — the set of validators whose ack for `eid` node n  *)
(*                        has locally recorded (G-Set: grows via AckEvent  *)
(*                        and GossipMerge; only shrinks via RotateEpoch,   *)
(*                        which drops acks from validators no longer in    *)
(*                        the active set — see `PendingAcksAreCurrentMembers`*)
(*                        below).                                          *)
(*   finalized[n]      — the set of event ids node n considers Lane        *)
(*                        0-final (G-Set: grows via FinalizeIfQuorum only, *)
(*                        NEVER shrinks — see `EpochFenceMonotone`).       *)
(*                                                                         *)
(* Global state (see the header comment for why these are global rather   *)
(* than per-node — they represent facts already agreed by Lane 1):        *)
(*   active             — the currently active Lane 0 validator set.       *)
(*   epoch              — number of rotations applied so far.              *)
(***************************************************************************)
VARIABLES acks, finalized, active, epoch

vars == <<acks, finalized, active, epoch>>

TypeOK == /\ acks \in [Nodes -> [EventId -> SUBSET Nodes]]
          /\ finalized \in [Nodes -> SUBSET EventId]
          /\ active \subseteq Nodes
          /\ active # {}
          /\ epoch \in Nat

\* BFT-style supermajority over the CURRENT active set: strictly more than
\* 2/3 of the active validator count. Mirrors `ValidatorSet::is_quorum` in
\* lane0.rs, specialized to equal-weight validators (see Known Limitations
\* in the formal-verification README).
Quorum(S) == Cardinality(S) * 3 > Cardinality(active) * 2

Init == /\ acks = [n \in Nodes |-> [eid \in EventId |-> {}]]
        /\ finalized = [n \in Nodes |-> {}]
        /\ active = InitialActive
        /\ epoch = 0

(***************************************************************************)
(* A validator `n` (honest or Byzantine — a Byzantine node still holds a   *)
(* real signing key, so it CAN produce a validly-signed ack; the protocol  *)
(* tolerates this by requiring a supermajority, not unanimity) acks event  *)
(* `eid`, recording its own ack in its own local certificate first — this  *)
(* mirrors `Substrate::lane0_ack_inserted`'s "fold locally first, then     *)
(* broadcast" order. Only current members of `active` can produce an ack  *)
(* that counts; this mirrors `CertificateStore::add_ack` rejecting acks   *)
(* from public keys outside the configured validator set.                 *)
(***************************************************************************)
AckEvent(n, eid) ==
    /\ n \in active
    /\ acks' = [acks EXCEPT ![n][eid] = @ \cup {n}]
    /\ UNCHANGED <<finalized, active, epoch>>

(***************************************************************************)
(* Gossip dissemination: node n2 merges node n1's locally known ack set    *)
(* for `eid` into its own. Union is the G-Set merge — commutative,         *)
(* associative, idempotent, so delivery order and duplication are          *)
(* harmless (mirrors `CertificateStore::add_ack`'s duplicate-is-a-no-op    *)
(* behavior and the CRDT convergence proofs in `OmniaCRDT.tla`).           *)
(*                                                                         *)
(* The `\cap active` filter models `add_ack`'s membership check: an ack    *)
(* from a public key outside the CURRENT validator set is rejected at      *)
(* receipt time (`Lane0Error::UnknownValidator` in lane0.rs), so gossip    *)
(* can never re-introduce a since-removed validator's stale ack into a     *)
(* pending certificate. TLC found the counterexample that mandates this    *)
(* filter on the first CI run of this spec: a node that finalized before   *)
(* a rotation keeps its (frozen) pre-rotation ack set, and an unfiltered   *)
(* merge would copy those stale acks into a peer's still-pending           *)
(* certificate — violating `PendingAcksAreCurrentMembers`. The             *)
(* implementation was never affected; the unfiltered model was simply      *)
(* more permissive than the code it models.                                *)
(***************************************************************************)
GossipMerge(n1, n2, eid) ==
    /\ n1 # n2
    /\ acks' = [acks EXCEPT ![n2][eid] = @ \cup (acks[n1][eid] \cap active)]
    /\ UNCHANGED <<finalized, active, epoch>>

(***************************************************************************)
(* Node `n` observes that its locally accumulated acks for `eid` — always  *)
(* a subset of the CURRENT active set, see `PendingAcksAreCurrentMembers`  *)
(* — now form a quorum, and finalizes. The `\cap active` is defensive      *)
(* (the invariant already guarantees it is a no-op) rather than load-      *)
(* bearing, matching the defense-in-depth style already used elsewhere in  *)
(* the codebase (e.g. gossip.rs's belt-and-suspenders payload-size check). *)
(***************************************************************************)
FinalizeIfQuorum(n, eid) ==
    /\ eid \notin finalized[n]
    /\ Quorum(acks[n][eid] \cap active)
    /\ finalized' = [finalized EXCEPT ![n] = @ \cup {eid}]
    /\ UNCHANGED <<acks, active, epoch>>

(***************************************************************************)
(* RotateEpoch — the epoch fence. Abstracts a Lane 1 (DAG-consensus)       *)
(* committed validator-set-change event, applied atomically and            *)
(* identically at every node (see the header comment for why that is a    *)
(* sound abstraction).                                                     *)
(*                                                                         *)
(* Mirrors `CertificateStore::rotate_validators` exactly:                 *)
(*   - Already-finalized event ids are UNTOUCHED at every node — finality *)
(*     survives the rotation regardless of who is in the new set. This is *)
(*     the monotonicity guarantee `EpochFenceMonotone` checks below.       *)
(*   - For every still-pending event id, acks from validators that are    *)
(*     not members of the new set are dropped; acks from validators that  *)
(*     remain members are kept. (A subsequent `FinalizeIfQuorum` step may *)
(*     then immediately become enabled if the retained acks already meet  *)
(*     quorum under the new set — this module models that as two separate*)
(*     actions rather than one atomic step, which is a strictly WEAKER    *)
(*     synchrony assumption than the real single-function-call Rust       *)
(*     implementation, so it cannot hide a bug the real code doesn't have.)*)
(***************************************************************************)
RotateEpoch(newActive) ==
    /\ newActive \subseteq Nodes
    /\ newActive # {}
    /\ newActive # active
    /\ epoch < MaxEpoch
    /\ active' = newActive
    /\ epoch' = epoch + 1
    /\ acks' = [n \in Nodes |-> [eid \in EventId |->
                  IF eid \in finalized[n]
                  THEN acks[n][eid]
                  ELSE acks[n][eid] \cap newActive]]
    /\ UNCHANGED finalized

Next == \/ \E n \in Nodes, eid \in EventId: AckEvent(n, eid)
        \/ \E n1 \in Nodes, n2 \in Nodes, eid \in EventId: GossipMerge(n1, n2, eid)
        \/ \E n \in Nodes, eid \in EventId: FinalizeIfQuorum(n, eid)
        \/ \E newActive \in SUBSET Nodes: RotateEpoch(newActive)

Spec == Init /\ [][Next]_vars

\* State-space bound for TLC (mirrors OmniaConsensus.tla's MaxSeq role).
StateConstraint == epoch <= MaxEpoch

\* Each quantified conjunct is parenthesized: without the parentheses a
\* `\A` body extends to the end of the expression, so the later
\* conjuncts would (illegally) re-bind `n`/`eid` inside its scope.
FairSpec == Spec
            /\ (\A n \in Nodes, eid \in EventId: WF_vars(AckEvent(n, eid)))
            /\ (\A n1 \in Nodes, n2 \in Nodes, eid \in EventId: WF_vars(GossipMerge(n1, n2, eid)))
            /\ (\A n \in Nodes, eid \in EventId: WF_vars(FinalizeIfQuorum(n, eid)))

(***************************************************************************)
(* SAFETY (headline property): every ack counted toward a PENDING          *)
(* certificate belongs to the CURRENTLY active validator set. This is the  *)
(* soundness half of epoch fencing — a rotation can never let a stale ack  *)
(* from a since-removed validator sneak a certificate over quorum.         *)
(* Finalized certificates are explicitly exempt (see RotateEpoch above):   *)
(* they are frozen, not re-evaluated, which is the other half of the       *)
(* fence (see `EpochFenceMonotone`).                                       *)
(***************************************************************************)
PendingAcksAreCurrentMembers ==
    \A n \in Nodes, eid \in EventId:
        eid \notin finalized[n] => acks[n][eid] \subseteq active

(***************************************************************************)
(* SAFETY (temporal property): `finalized[n]` never shrinks, for any node, *)
(* under any action — including RotateEpoch. This is the exact claim in    *)
(* ADR-025's Consequences: "Lane 1 commits act as epoch fences for Lane 0  *)
(* certificate validity" — once decided, a Lane 0 certificate's finality   *)
(* is permanent, independent of how many validator-set rotations follow.  *)
(***************************************************************************)
EpochFenceMonotone == [][\A n \in Nodes: finalized[n] \subseteq finalized'[n]]_vars

=======================================================================================

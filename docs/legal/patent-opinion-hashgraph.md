# Patent Risk Assessment: Hashgraph Patent Analysis

**Project**: Omnia Protocol  
**Date**: 2026-05-22  
**Status**: Mitigation Document — Legal Opinion Pending  
**Reference Issue**: #43

---

## 1. Overview

This document analyzes the potential patent risk arising from the similarity between Omnia Protocol's two-parent event DAG design and the Hashgraph consensus algorithm, which is covered by US Patent 10,496,525 (and related international patents) assigned to Swirlds, Inc. (now Hedera Hashgraph).

The purpose of this document is to:
1. Identify the specific claims that may be relevant
2. Document the technical differences between Omnia and Hashgraph
3. Propose mitigation strategies
4. Request a formal legal opinion from qualified patent counsel

---

## 2. Relevant Patent Claims

### US Patent 10,496,525 — "System and Method for Consensus"

Key claims relevant to Omnia's design:

- **Claim 1**: A method for achieving consensus comprising: creating events each comprising a hash of a first prior event and a hash of a second prior event from a different member; defining rounds based on witnessing events; determining fame of witnesses based on votes from later-round witnesses.

- **Claim 7**: A method where each event includes a hash of the event creator's last event (self-parent) and a hash of another member's event (other-parent).

- **Claim 12**: A distributed database system where members create events with multiple parent hashes and determine consensus through famous witness selection.

### Potentially Applicable Claims

| Claim Element | Hashgraph | Omnia | Similarity |
|---------------|-----------|-------|------------|
| Two-parent events (self + other) | Yes | Yes | High |
| Round assignment via witnessing | Yes | Yes | Medium |
| Famous witness determination | Yes | Modified | Medium |
| Supermajority threshold (2N/3+1) | Yes | Yes | High |
| Event hash as identifier | SHA-384 | SHA-256/BLAKE3 | Low |
| Gossip about gossip | Yes | No | None |
| Virtual voting | Yes | No | None |

---

## 3. Technical Differences

### 3.1 Fundamental Architecture

| Aspect | Hashgraph | Omnia |
|--------|-----------|-------|
| **Consensus mechanism** | Gossip-about-gossip + virtual voting | Causal graph + BFT finality gadget (AlephBFT-inspired) |
| **Event propagation** | Gossip about gossip (events contain full history) | Direct gossip (events contain only parent references) |
| **Voting** | Virtual voting (deterministic from graph structure) | Explicit acknowledgment + BFT commitment |
| **State representation** | Events as sole state carrier | Separate CausalGraph + ConsensusEngine + CRDT state |
| **Finality determination** | Famous witnesses → consensus timestamp | Witness → Fame → Commitment (BFT-style) |

### 3.2 Event Structure

| Field | Hashgraph | Omnia |
|-------|-----------|-------|
| Self-parent hash | Yes | Yes |
| Other-parent hash | Yes | Yes |
| Creator signature | Yes (Ed25519) | Yes (Ed25519 + optional BLS aggregation) |
| Timestamp | Yes (consensus median) | Yes (creator's local time) |
| Payload | Transactions | Arbitrary bytes (shard-specific) |
| Vector clock | No | Yes (explicit VectorClock field) |
| Event hash | SHA-384 | SHA-256 + BLAKE3 domain separation |

### 3.3 Consensus Protocol

**Hashgraph**:
1. Events are gossiped with full ancestry ("gossip about gossip")
2. Rounds are assigned when an event can "see" >2/3 of prior-round witnesses
3. Witnesses vote on fame of prior-round witnesses via virtual voting
4. Famous witnesses determine consensus order and timestamps
5. No separate state machine — consensus emerges from graph structure

**Omnia**:
1. Events are gossiped with only parent references (compact encoding + delta compression)
2. Rounds are assigned when an event can "strongly see" >2/3 of prior-round witnesses
3. Witnesses are determined by `(creator, round)` uniqueness
4. Fame is determined by explicit acknowledgment from later-round witnesses (BFT-style, not virtual voting)
5. Committed events trigger CRDT merges for state convergence
6. Separate VRF-based leader selection for round advancement
7. Explicit slashing for Byzantine behavior (equivocation detection)

### 3.4 Key Distinguishing Features

1. **No "Gossip About Gossip"**: Omnia does not propagate event history through gossip. Each event contains only parent hashes, not the full ancestry. This is fundamentally different from Hashgraph's "gossip about gossip" mechanism where each gossip round carries accumulated history.

2. **Explicit BFT Commitment**: Omnia uses explicit BFT-style commitment with acknowledgment tracking, not Hashgraph's virtual voting where votes are implicitly derived from graph structure.

3. **CRDT State Layer**: Omnia separates consensus from state management using CRDTs (GCounter, OrSet, LwwRegister) for deterministic state convergence. Hashgraph has no equivalent.

4. **VRF Leader Selection**: Omnia uses VRF-based leader selection with BLAKE3-derived round seeds, which is absent from Hashgraph.

5. **Slashing and Accountability**: Omnia implements explicit slashing for Byzantine behavior (equivocation, liveness violations) with graded penalties and jail registry. Hashgraph relies solely on its consensus mechanism for Byzantine tolerance.

6. **ZK Rollup Integration**: Omnia's consensus feeds into a ZK-rollup layer for L1 settlement, which is architecturally absent from Hashgraph.

7. **Sharded Consensus State**: Phase 0 introduces sharded consensus state for parallel processing, which has no equivalent in Hashgraph.

8. **Batch Processing**: Omnia processes events in batches with aggregated proofs for throughput optimization.

---

## 4. Mitigation Strategies

### 4.1 Design-Level Mitigations (Already Implemented)

- **Explicit BFT gadget** instead of virtual voting (fundamentally different consensus mechanism)
- **No gossip-about-gossip** (different propagation model)
- **CRDT state convergence** (separate from consensus)
- **VRF leader selection** (different round advancement)
- **Sharded + batch processing** (architectural innovation beyond Hashgraph)

### 4.2 Fallback Design (If Patent Opinion Is Adverse)

If legal counsel determines that the two-parent event structure infringes Hashgraph claims, the following fallback DAG design can be implemented:

**Single-Parent Linear Chain with Cross-Links**:
- Replace two-parent events with single-parent linear chains (one self-parent per event)
- Add cross-links as separate metadata records (not part of the core event structure)
- Round assignment based on chain height + cross-link density
- Consensus proceeds via the linear chain with cross-links as supplementary information

This design avoids the two-parent event structure entirely while maintaining equivalent functionality.

### 4.3 Licensing Options

If the fallback design is undesirable:
- Evaluate Swirlds/Hedera licensing terms
- Consider Apache 2.0 license compatibility
- Assess whether Omnia's use falls within fair use or experimental use exceptions

---

## 5. Legal Opinion Request

We request formal legal opinion on the following questions:

1. **Does Omnia's two-parent event DAG structure infringe any valid claim of US Patent 10,496,525?**
2. **Does the combination of Omnia's BFT finality gadget (as distinct from Hashgraph's virtual voting) avoid the patent claims?**
3. **Are the Hashgraph patent claims valid and enforceable, given the prior art in distributed systems (e.g., DAG-based consensus in earlier academic work)?**
4. **What is the risk assessment for operating Omnia in jurisdictions where Hashgraph patents are registered?**
5. **What specific design modifications would be sufficient to avoid infringement, if any?**

### Supporting Materials

- Omnia Protocol source code (repository access)
- Architecture documentation: `docs/arch/consensus-sharding.md`
- Event structure: `omnia-primitives/src/event.rs`
- Consensus engine: `omnia-consensus/src/consensus.rs`
- This document with detailed technical comparison

---

## 6. Timeline

| Milestone | Target |
|-----------|--------|
| Submit patent opinion request | Immediate |
| Counsel review begins | Week 1 |
| Interim opinion (preliminary risk assessment) | Week 4 |
| Final legal opinion delivered | Week 8 |
| Design modifications (if needed) | Weeks 9-12 |

---

## 7. Conclusion

While Omnia's two-parent event structure shares surface-level similarity with Hashgraph, the underlying consensus mechanism, state management, and architectural approach are fundamentally different. The most significant differentiators are:

1. Omnia does NOT use "gossip about gossip" — events carry only parent references
2. Omnia uses explicit BFT commitment, NOT virtual voting
3. Omnia has separate CRDT state convergence layer
4. Omnia includes VRF leader selection and slashing mechanisms absent from Hashgraph

These differences represent substantial technical innovation beyond the Hashgraph patent claims. However, a formal legal opinion is required to definitively assess infringement risk and determine appropriate mitigations.

# 🏗️ Omnia Protocol Architecture

> **⚠️ This document describes the full architecture vision. Only sections labeled ✅ Implemented reflect the actual codebase. Sections labeled 📋 Planned or 🔮 Aspirational describe future goals. For current implementation details, see the root [ARCHITECTURE.md](../../ARCHITECTURE.md).**

## Table of Contents

1. [System Overview](#system-overview)
2. [Layer 1: The Substrate](#layer-1-the-substrate)
3. [Layer 2: Domain Shards](#layer-2-domain-shards)
4. [Layer 3: The Binding Layer](#layer-3-the-binding-layer)
5. [Layer 4: Identity Layer](#layer-4-identity-layer)
6. [Layer 5: Economic Layer](#layer-5-economic-layer)
7. [Cross-Layer Interactions](#cross-layer-interactions)
8. [Consensus Mechanism](#consensus-mechanism)
9. [Scalability & Performance](#scalability--performance)
10. [Security Model](#security-model)

---

## System Overview

Omnia is a five-layer distributed system designed to enable trustless coordination at global and interplanetary scales.

```
┌─────────────────────────────────────────┐
│  LAYER 5: Economics (UBC, Governance)   │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 4: Identity (DIDs, Shamir, Bio) │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 3: Binding (Provenance, RF, QC) │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 2: Domain Shards (6 shards)     │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 1: Substrate (Causal Graph)     │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  PHASE 0: ZK-Rollup (Settlement Layer) │ ✅ IMPLEMENTED
└─────────────────────────────────────────┘
```

**Implementation status:** All five core layers are scaffolded and tested (200+ tests). Phase 0 (ZK-rollup settlement) has an Ethereum adapter. Some features within layers are ⚠️ stubs (RF fingerprinting, quantum commitments, ZK circuit).

---

## Layer 1: The Substrate — ✅ Implemented

### Purpose

The foundation that enables the network to agree on what happened without relying on global clock time or a single authority.

### Key Components

#### Causal Graph Consensus — ✅ Implemented

Instead of organizing events into sequential blocks, Omnia maintains a **directed acyclic graph (DAG)** where:

- Each event (transaction) is a node
- Edges represent causal relationships (event A must happen before event B)
- Unrelated events can be processed in parallel
- The graph naturally captures causality without artificial ordering

**Advantages:**
- Transactions that don't depend on each other can be finalized independently
- Network latency does not block unrelated transactions
- O(new_events) consensus processing via `unprocessed_events` queue

#### Vector Clocks — ✅ Implemented

Each node maintains a **vector clock** — a data structure that tracks what it has seen from every other node.

```
Node A's vector clock: [3, 2, 5, 1]
                        ↓  ↓  ↓  ↓
                    A's B's C's D's
                    events events events events
```

**Properties:**
- If `VC_A < VC_B` (component-wise), then event A causally precedes event B
- If neither `VC_A < VC_B` nor `VC_B < VC_A`, the events are concurrent
- Nodes can determine ordering without global synchronization

#### CRDTs — ✅ Implemented

For state that requires convergence, Omnia uses CRDTs:

- **GCounter**: Grow-only counter for monotonic values
- **OrSet**: Observed-remove set with add-wins semantics
- **LWWRegister**: Last-write-wins register for single values

⚠️ **Note:** The FinancialShard uses strict causal ordering, not CRDTs, for balance consistency.

#### Replay Protection — ✅ Implemented

Per-creator nonce tracking in both CausalGraph and ShardRouter prevents replay attacks.

#### State Commitments — ✅ Implemented

- `state_root()` — Merkle root of the entire graph state
- `merkle_proof()` — Inclusion proof for any event
- `prune_old_events()` — Event pruning for long-term sustainability

### Relativistic Boundaries — 🔮 Aspirational

For interplanetary operation, the protocol would need to acknowledge that communication has physical limits:

- Earth-to-Mars: 3-22 minutes one way
- Mars-to-Jupiter: 5-60 minutes one way

**Planned solution:** Each region maintains its own causal graph and periodically synchronizes with other regions. This is 🔮 not yet implemented or tested.

---

## Layer 2: Domain Shards — ✅ Implemented

### Purpose

Organize different types of activity into specialized lanes, each with optimized consensus and state management.

### Architecture

Each domain shard is a **projection of the unified state** that:

- Maintains its own state tree
- Processes transactions relevant to its domain (via `EventProcessor` trait)
- Can reference state from other shards atomically
- Contributes to the global state root

### Implemented Shards (6 total)

| Shard | Purpose | Status |
|-------|---------|--------|
| 💰 Financial | Balances, transfers, replay protection | ✅ Implemented |
| 🆔 Identity | DID management, credentials | ✅ Implemented |
| 📦 Physical | Object registration, provenance | ✅ Implemented |
| 🧮 Computational | AI training, proofs | ✅ Implemented |
| 🧬 Biological | Health records, bio-signals | ✅ Implemented |
| 📊 Economics | UBC, governance, useful work | ✅ Implemented |

⚠️ **Note:** The original spec included Energy and Temporal shards. The actual codebase implements 6 shards (Financial, Identity, Physical, Computational, Biological, Economics).

### Cross-Shard Transactions — ✅ Implemented

Cross-shard messaging with causality proofs is implemented. A single transaction can atomically touch multiple shards via the ShardRouter.

---

## Layer 3: The Binding Layer — 🏗️ Partially Implemented

### Purpose

Anchor the digital system to physical reality without requiring trusted intermediaries (oracles).

### Physical Anchoring Methods

#### Provenance Log — ✅ Implemented

The provenance log is fully implemented as an append-only CRDT. It provides:

- Create, transfer, verify, destroy lifecycle for tracked items
- Complete ownership history (cryptographic birth certificate)
- No intermediaries needed for verification

#### RF Fingerprinting — ⚠️ Stub

Every physical object emits unique electromagnetic noise due to manufacturing imperfections. The stub implementation uses Hamming distance comparison.

**What's real:** The data structure and comparison logic exist.
**What's not real:** 🌑 Requires SDR hardware (HackRF/USRP) for actual RF signal capture.

#### Quantum Commitments — ⚠️ Stub

The quantum commitment stub uses a hybrid classical + PQC placeholder.

**What's real:** The data structure and commitment flow exist.
**What's not real:** 🌑 Requires CRYSTALS-Dilithium integration for real post-quantum security.

#### Physical Time Anchors — 🌑 Not Implemented

Previously described as "Gravitational Timestamps" using atomic clocks to detect relativistic time dilation for location verification. This is **not implemented** and has no code. The protocol currently relies on logical time (vector clocks) rather than physical time anchors.

#### Biometric Binding — ✅ Implemented

Privacy-preserving biometric anchors using `BLAKE3(salt || template)`. The template is never stored in cleartext.

#### Satellite Mesh — 🌑 Not Implemented

GPS + Galileo + Starlink cross-validation for location verification is not implemented.

### Example: Ethical Diamonds — 🔮 Aspirational Use Case

The provenance log can theoretically support this use case, but real RF fingerprinting, quantum seals, and satellite mesh are 🌑 not implemented:

1. **Mining:** Diamond is extracted. RF fingerprint is recorded. Quantum seal is applied.
2. **Cutting:** Cutter signs the transaction with hardware attestation. RF fingerprint is updated.
3. **Polishing:** Polisher signs. RF fingerprint is verified.
4. **Shipping:** Container has quantum seal and GPS tracking via satellite mesh.
5. **Retail:** Jeweler verifies all signatures and physical properties.
6. **Consumer:** Scans ring. Full chain is visible: mine → cutter → polisher → shipper → jeweler → you.

---

## Layer 4: Identity Layer — ✅ Implemented

### Purpose

Enable self-sovereign identity where individuals, AI agents, and collectives own their identity forever.

### Components

#### Decentralized Identifiers (DIDs) — ✅ Implemented

The `did:omnia:` method is fully implemented with validation.

**Format:** `did:omnia:z6MkhaXgBZDvotDkL5257faWxcqACaGVJRPn92ND5CHXvP`

**Properties:**
- Created by the user, not issued by any authority
- Cryptographically verifiable
- Cannot be revoked or censored
- Portable across platforms

#### Social Recovery — ✅ Implemented

Social recovery uses **Shamir's Secret Sharing over GF(256)**:

1. Private key is split into N shares using Shamir's Secret Sharing
2. Shares are distributed to trusted guardians
3. Threshold number of shares (e.g., 3 of 5) can reconstruct the key
4. No single guardian has the full key
5. No company or government involved

#### Biometric Anchors — ✅ Implemented

Privacy-preserving biometric anchors: `BLAKE3(salt || template)`. Template never stored in cleartext.

#### AI Agent Identity — ✅ Implemented

AI agent identities with 5 capability types are implemented.

#### Reputation System — 🏗️ Partially Implemented

| Component | Status |
|-----------|--------|
| ✅ Exponential reputation decay | Implemented |
| 🌑 Full reputation scoring | Not yet implemented |
| 📋 Reputation thresholds | Planned |

---

## Layer 5: Economic Layer — 🏗️ Partially Implemented

### Purpose

Create a monetary system that serves people, not extracts from them.

### Universal Basic Compute (UBC) — ✅ Implemented

Every identity receives a soulbound (non-transferable) monthly quota. The UBC token and QuotaSystem with epoch advancement are implemented.

### Quadratic Voting — ✅ Implemented

Quadratic voting with exponential reputation decay is implemented.

### Conviction Voting — 📋 Planned

Locking tokens for longer periods to increase voting power is planned for Phase 1.

### Delegation — 📋 Planned

Delegating voting power to trusted representatives is planned for Phase 1.

### RPGF — 🔮 Aspirational

Retroactive Public Goods Funding is an aspirational concept. There is no treasury, no fee mechanism, and no RPGF distribution system implemented.

### Fee Structure — 🌑 Not Implemented

There is no fee mechanism. UBC quotas cover all transaction costs.

### Adaptive Monetary Policy — 🔮 Aspirational

The concept of algorithmic monetary policy responding to network state is aspirational.

---

## Cross-Layer Interactions

### Example: Supply Chain — 🔮 Aspirational

The provenance log is ✅ implemented, but real RF fingerprinting, quantum seals, and satellite mesh are 🌑 not.

| Layer | Feature | Status |
|-------|---------|--------|
| Layer 1 (Substrate) | Causal graph tracks event sequence | ✅ Implemented |
| Layer 2 (Shards) | Financial, Physical, Identity shards | ✅ Implemented |
| Layer 3 (Binding) | RF fingerprint, quantum seal, satellite mesh | ⚠️ Stubs / 🌑 Not Implemented |
| Layer 4 (Identity) | DID verification | ✅ Implemented |
| Layer 5 (Economic) | Fee structure, RPGF | 🌑 Not Implemented |

---

## Consensus Mechanism

### Causal+ Consistency — ✅ Implemented

Omnia implements causal consistency, which guarantees:

1. **Causality:** If event A causally precedes event B, all nodes see A before B
2. **Consistency:** All nodes eventually see the same state (via CRDTs)
3. **Liveness:** The system continues to make progress even if some nodes are offline

### Finality — ✅ Implemented

BFT finality via the ConsensusEngine with supermajority witness model (inspired by Hashgraph + AlephBFT).

⚠️ **Time to finality:** Not yet benchmarked at scale. The O(new_events) processing design targets low latency, but specific numbers have not been measured.

---

## Scalability & Performance

### Throughput

⚠️ **Not yet benchmarked.** The consensus engine processes O(new_events) per round via the `unprocessed_events` queue, which is designed for scalability. The 10,000+ TPS target has not been verified with benchmarks.

### Latency

⚠️ **Not yet benchmarked.** No real-world network testing has been performed.

### Storage

The `prune_old_events()` method provides a mechanism for sustainable state growth. Specific storage requirements have not been measured.

---

## 🛡️ Security Model

### Threat Model

**Adversaries:**
- Up to 1/3 of validator nodes are Byzantine (faulty or malicious) — designed, not tested in production
- Network may partition temporarily — designed via CRDT merge
- Cryptographic primitives: Ed25519 signatures, BLAKE3 hashing (post-quantum not yet integrated)

### Security Guarantees — Designed, Not Production-Tested

| Guarantee | Status |
|-----------|--------|
| ✅ Consistency (2/3 honest → system consistent) | Designed |
| ✅ Liveness (connected + 2/3 honest → progress) | Designed |
| ✅ Replay protection (nonce tracking) | Implemented |
| ✅ State commitments (Merkle root + proofs) | Implemented |
| ✅ Event pruning (sustainability) | Implemented |
| 🌑 Economic security (slashing, staking) | Not implemented |

### Cryptographic Primitives

| Primitive | Status |
|-----------|--------|
| ✅ Ed25519 signatures | Implemented |
| ✅ BLAKE3 hashing | Implemented |
| ✅ zk-SNARKs (arkworks R1CS + Groth16) | Implemented |
| ✅ CRYSTALS-Dilithium (PQC signatures) | Implemented |

### Economic Security — 🟡 Partially Implemented

Slashing is implemented (SlashingEngine with equivocation/liveness/invalid attestation detection, persistent via sled). Staking is partially modeled via the SlashingEngine's validator registration. There is no validator network yet.

---

## 🔮 Future Enhancements — Aspirational

### Quantum Resistance
- 🔄 CRYSTALS-Dilithium (signatures) — stub exists
- 🌑 SPHINCS+ (hash-based signatures) — not started
- 📋 Gradual migration, no hard fork — planned

### Homomorphic Encryption
- 🔮 Computing on encrypted data without decryption — aspirational

### Proof-of-Useful-Work
- ⚠️ Scientific computation, AI training, rendering — stubs exist (3 work types)
- 🌑 Real verification of useful work — not implemented

### Interplanetary Operation
- 🔮 Relativistic consensus — aspirational
- 🔮 Local autonomy with eventual consistency — aspirational

---

## 📚 References

- Lamport, L. (1978). "Time, Clocks, and the Ordering of Events in a Distributed System"
- Shapiro, M., & Preguiça, N. (2011). "Conflict-free Replicated Data Types"
- Ben-Sasson, E., et al. (2014). "Zerocash: Decentralized Anonymous Payments from Bitcoin"

---

**Status:** Architecture Specification — Partially Implemented
**Version:** 2.0
**Last Updated:** May 2026

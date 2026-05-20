# Architecture Full Specification
> 🎯 Audience: Architects
> 🔗 Context: Comprehensive architecture specification covering all layers, node binary, and cross-layer interactions
> 📅 Last Updated: 2026-05-20

**Version:** v4.0.0
**Last Updated:** 2026-03-05

> **This document describes the full architecture of the Omnia Protocol as implemented in v4.0.0. Sections are labeled with their implementation status: ✅ Implemented, ⚠️ Partially Implemented (has stubs), 🔮 Aspirational (no code).**

## Table of Contents

1. [System Overview](#system-overview)
2. [Layer 1: The Substrate](#layer-1-the-substrate)
3. [Layer 2: Domain Shards](#layer-2-domain-shards)
4. [Layer 3: The Binding Layer](#layer-3-the-binding-layer)
5. [Layer 4: Identity Layer](#layer-4-identity-layer)
6. [Layer 5: Economic Layer](#layer-5-economic-layer)
7. [Node Binary](#node-binary)
8. [Cross-Layer Interactions](#cross-layer-interactions)
9. [Consensus Mechanism](#consensus-mechanism)
10. [Scalability & Performance](#scalability--performance)
11. [Security Model](#security-model)

---

## System Overview

Omnia is a five-layer distributed system designed to enable trustless coordination at global and interplanetary scales.

```
┌─────────────────────────────────────────┐
│  LAYER 5: Economics (UBC, Governance)   │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 4: Identity (DIDs, Shamir, Bio) │ ✅ IMPLEMENTED (in shards)
├─────────────────────────────────────────┤
│  LAYER 3: Binding (Provenance, RF, QC) │ ✅ IMPLEMENTED (QC real, RF stub)
├─────────────────────────────────────────┤
│  LAYER 2: Domain Shards (6 shards)     │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  LAYER 1: Substrate (Causal Graph)     │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  PHASE 0: ZK-Rollup (Settlement Layer) │ ✅ IMPLEMENTED
├─────────────────────────────────────────┤
│  NODE BINARY (CLI + REST API)          │ ✅ IMPLEMENTED
└─────────────────────────────────────────┘
```

**Implementation status:** All five core layers are implemented and tested (295+ tests). The node binary provides a CLI, REST API with Swagger UI, and Prometheus metrics. Phase 0 (ZK-rollup settlement) has an Ethereum adapter plus Solana, Celestia, and Bitcoin adapters. Some features within layers are ⚠️ stubs (RF fingerprinting, ZK circuit hash round constants).

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

For state that requires convergence, Omnia uses CRDTs (formally verified in `OmniaCRDT.tla`):

- **GCounter**: Grow-only counter for monotonic values
- **OrSet**: Observed-remove set with add-wins semantics
- **LWWRegister**: Last-write-wins register for single values

⚠️ **Note:** The FinancialShard uses strict causal ordering, not CRDTs, for balance consistency.

#### Replay Protection — ✅ Implemented

Per-creator nonce tracking in both CausalGraph and ShardRouter prevents replay attacks. Nonce state is persisted via `RedbNonceStore` across restarts (configured automatically in `omnia-node`).

#### State Commitments — ✅ Implemented

- `state_root()` — Merkle root of the entire graph state
- `merkle_proof()` — Inclusion proof for any event
- `prune_old_events()` — Event pruning for long-term sustainability
- `StateSnapshot` — Serialized snapshot with integrity verification (`snapshot.rs`)

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

| Shard | Purpose | Status | API Endpoint |
|-------|---------|--------|-------------|
| 💰 Financial | Balances, transfers, replay protection | ✅ Implemented | `POST /api/v1/shards/financial/operations` |
| 🆔 Identity | DID management, credentials | ✅ Implemented | `POST /api/v1/shards/identity/operations` |
| 📦 Physical | Object registration, provenance | ✅ Implemented | `POST /api/v1/shards/physical/operations` |
| 🧮 Computational | AI training, proofs | ✅ Implemented | `POST /api/v1/shards/computational/operations` |
| 🧬 Biological | Health records, bio-signals | ✅ Implemented | `POST /api/v1/shards/biological/operations` |
| 📊 Economics | UBC, governance, useful work | ✅ Implemented | `POST /api/v1/shards/economics/operations` |

⚠️ **Note:** Only the Economics shard has full API-level operation support (mint, spend, register, advance_epoch). Other shards return `{"status": "accepted", "note": "..."}` via the API.

### Cross-Shard Transactions — ✅ Implemented

Cross-shard messaging with causality proofs is implemented in `shards/src/cross_shard.rs`. A single transaction can atomically touch multiple shards via the ShardRouter.

### Fee Enforcement — ✅ Implemented

The `FeeSchedule` maps each `ShardOp` variant to a fixed `u64` fee (2-15 UBC). The `QuotaSystem` deducts fees atomically before shard dispatch. The `ShardRouter::route_event()` processing order is: nonce check → fee deduction → route.

---

## Layer 3: The Binding Layer — ✅ Implemented (with stubs)

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

#### Quantum Commitments — ✅ Implemented

The quantum commitment system uses a hybrid Ed25519 + CRYSTALS-Dilithium approach with phase transitions (`ClassicalOnly` → `Hybrid` → `PostQuantum`).

**What's real:** Both `verify_ed25519()` and `verify_dilithium()` perform real cryptographic verification. The `verify_dilithium()` method calls `pqc_dilithium::verify()` (no longer a stub returning `true`).

**Remaining gap:** No constant-time guarantee documented for the `pqc_dilithium` crate.

#### Physical Time Anchors — 🌑 Not Implemented

Previously described as "Gravitational Timestamps" using atomic clocks. Not implemented; protocol relies on logical time (vector clocks).

#### Biometric Binding — ✅ Implemented

Privacy-preserving biometric anchors using `BLAKE3(salt || template)`. The template is never stored in cleartext.

#### Satellite Mesh — 🌑 Not Implemented

GPS + Galileo + Starlink cross-validation for location verification is not implemented.

---

## Layer 4: Identity Layer — ✅ Implemented (within shards)

### Purpose

Enable self-sovereign identity where individuals, AI agents, and collectives own their identity forever.

**Note:** Identity is implemented within the `omnia-shards` crate (`IdentityShard`), not as a separate crate/layer. This differs from the original architecture vision which envisioned Identity as a standalone layer.

### Components

#### Decentralized Identifiers (DIDs) — ✅ Implemented

The `did:omnia:` method is fully implemented with validation.

**Format:** `did:omnia:z6MkhaXgBZDvotDkL5257faWxcqACaGVJRPn92ND5CHXvP`

#### Social Recovery — ✅ Implemented

Social recovery uses **Shamir's Secret Sharing over GF(256)** with N shares and threshold reconstruction.

#### Biometric Anchors — ✅ Implemented

Privacy-preserving biometric anchors: `BLAKE3(salt || template)`.

#### AI Agent Identity — ✅ Implemented

AI agent identities with 5 capability types are implemented.

#### Reputation System — 🏗️ Partially Implemented

| Component | Status |
|-----------|--------|
| ✅ Exponential reputation decay | Implemented |
| 🌑 Full reputation scoring | Not yet implemented |
| 📋 Reputation thresholds | Planned |

---

## Layer 5: Economic Layer — ✅ Implemented

### Purpose

Create a monetary system that serves people, not extracts from them.

### Universal Basic Compute (UBC) — ✅ Implemented

Every identity receives a soulbound (non-transferable) monthly quota. The UBC token and QuotaSystem with epoch advancement are implemented. Monthly quota: 1000 UBC with 10% decay.

### Quadratic Voting — ✅ Implemented

Quadratic voting with exponential reputation decay is implemented. Voting power = √stake via `isqrt()`. Voters must have registered stake in `voting_weights`.

### Fee Structure — ✅ Implemented

The `FeeSchedule` maps operations to fixed UBC fees:

| Category | Fee Range |
|----------|-----------|
| Identity operations | 2 UBC |
| Financial operations | 5 UBC |
| Cross-shard operations | 15 UBC |

Fees are deducted atomically before shard dispatch. No fee refund on operation failure.

### Slashing — ✅ Implemented

The `SlashingEngine` tracks three offense types with configurable thresholds:

| Offense | Points | Slash Threshold | Ejection Threshold |
|---------|--------|----------------|-------------------|
| Equivocation | 500 | 500 | 2000 |
| LivenessViolation | 100 | 500 | 2000 |
| InvalidAttestation | 300 | 500 | 2000 |

Persistent storage via `RedbSlashingStore`. The `omnia-node` binary configures redb persistence automatically.

### Conviction Voting — 📋 Planned

Locking tokens for longer periods to increase voting power.

### Delegation — 📋 Planned

Delegating voting power to trusted representatives.

### RPGF — 🔮 Aspirational

Retroactive Public Goods Funding is an aspirational concept with no implementation.

### Adaptive Monetary Policy — 🔮 Aspirational

Algorithmic monetary policy responding to network state is aspirational.

---

## Node Binary — ✅ Implemented

The `omnia-node` binary provides the operational interface to the protocol.

### CLI Interface

```sh
omnia-node [OPTIONS] [COMMAND]

Options:
  --node-id <ID>              Node identifier (u64, default: 1)
  --listen-addr <ADDR>        P2P listen address (default: "0.0.0.0:4001")
  --bootstrap-nodes <ADDRS>   Comma-separated bootstrap peers
  --http-port <PORT>          HTTP API port (default: 8080)
  --data-dir <DIR>            Data directory (default: "./data")
  --log-level <LEVEL>         Log level (default: "info")
  --config <PATH>             TOML config file path
  --protocol-version <VER>    Protocol version (default: "4.0.0")

Commands:
  run                  Run the node (default)
  keygen               Generate validator keypair
  setup-contribute     Contribute to Powers of Tau ceremony
  setup-verify         Verify Powers of Tau ceremony
  snapshot             Take a state snapshot
  restore              Restore from a snapshot
```

All CLI flags support `OMNIA_` prefix environment variable overrides (e.g., `OMNIA_NODE_ID=1`).

### REST API

9 endpoints under `/api/v1/` with Swagger UI at `/swagger-ui`:

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Liveness probe |
| GET | `/metrics` | Prometheus metrics |
| GET | `/api/v1/node/info` | Node identity and status |
| GET | `/api/v1/node/peers` | Connected peer list |
| POST | `/api/v1/events` | Submit a new event |
| GET | `/api/v1/events/{id}` | Retrieve event by ID |
| POST | `/api/v1/shards/{shard_id}/operations` | Submit shard operation |
| POST | `/api/v1/governance/proposals` | Create governance proposal |
| POST | `/api/v1/governance/vote` | Cast quadratic-weighted vote |
| GET | `/api/v1/economics/balance/{did}` | Check UBC balance |
| POST | `/api/v1/economics/transfer` | Spend UBC tokens |

**Security (Phase 0, FIND-001):** JWT authentication, AuthorizedCallers ACL, rate limiting, and CORS are now implemented. Endpoints require valid JWT tokens. Privileged operations (mint UBC, advance epoch) require admin JWT. Configured via `OMNIA_JWT_SECRET`, `OMNIA_AUTHORIZED_CALLERS`, `OMNIA_RATE_LIMIT_RPS`.

### Prometheus Metrics

6 node-level metrics registered in `NodeMetrics` (`node/src/state.rs`):

| Metric | Type | Description |
|--------|------|-------------|
| `omnia_node_events_submitted_total` | Counter | Events submitted via API |
| `omnia_node_events_finalized_total` | Counter | Events finalized by consensus |
| `omnia_node_peers_connected` | Gauge | Connected peers |
| `omnia_node_consensus_round` | Gauge | Current consensus round |
| `omnia_node_shard_operations_total` | Counter | Shard operations processed |
| `omnia_node_http_requests_total` | Counter | HTTP requests served |

---

## Cross-Layer Interactions

### Example: Supply Chain

| Layer | Feature | Status |
|-------|---------|--------|
| Layer 1 (Substrate) | Causal graph tracks event sequence | ✅ Implemented |
| Layer 2 (Shards) | Financial, Physical, Identity shards | ✅ Implemented |
| Layer 3 (Binding) | RF fingerprint, quantum seal, satellite mesh | ⚠️ Stubs / 🌑 Not Implemented |
| Layer 4 (Identity) | DID verification | ✅ Implemented |
| Layer 5 (Economic) | Fee structure, slashing | ✅ Implemented |
| Node Binary | REST API for all operations | ✅ Implemented |

---

## Consensus Mechanism

### Causal+ Consistency — ✅ Implemented

Omnia implements causal consistency, which guarantees:

1. **Causality:** If event A causally precedes event B, all nodes see A before B
2. **Consistency:** All nodes eventually see the same state (via CRDTs)
3. **Liveness:** The system continues to make progress even if some nodes are offline

### Finality — ✅ Implemented

BFT finality via the ConsensusEngine with supermajority witness model (inspired by Hashgraph + AlephBFT).

The TLA+ model (`formal-verification/OmniaConsensus.tla`, 191 lines) verifies:
- **Agreement** — All honest nodes that commit an event at the same `(creator, sequence)` agree on its hash
- **NoEquivocation** — Equivocation is confined to Byzantine creators
- **Validity** — Committed events were proposed by some node
- **Liveness** — Honest events are eventually committed (under fairness)
- **TypeOK** — State well-typedness

⚠️ **Time to finality:** Not yet benchmarked at scale.

---

## Scalability & Performance

### Throughput

⚠️ **Not yet benchmarked.** The consensus engine processes O(new_events) per round via the `unprocessed_events` queue, which is designed for scalability.

### Latency

⚠️ **Not yet benchmarked.** No real-world network testing has been performed.

### Storage

The `prune_old_events()` method provides a mechanism for sustainable state growth. The `snapshot_interval` config (default: 10,000 events) controls automatic snapshot creation. Specific storage requirements have not been measured.

---

## Security Model

### Threat Model

**Adversaries:**
- Up to 1/3 of validator nodes are Byzantine (faulty or malicious) — designed, not tested in production
- Network may partition temporarily — designed via CRDT merge, tested via chaos tests
- Cryptographic primitives: Ed25519 signatures, BLAKE3 hashing, CRYSTALS-Dilithium PQC signatures

### Security Guarantees

| Guarantee | Status |
|-----------|--------|
| ✅ Consistency (2/3 honest → system consistent) | Designed |
| ✅ Liveness (connected + 2/3 honest → progress) | Designed |
| ✅ Replay protection (nonce tracking with redb persistence) | Implemented |
| ✅ State commitments (Merkle root + proofs) | Implemented |
| ✅ Event pruning (sustainability) | Implemented |
| ✅ Economic security (slashing with persistence) | Implemented |
| ✅ Fee enforcement (FeeSchedule + QuotaSystem) | Implemented |
| ✅ API security (JWT auth + ACL + rate limiting + CORS) | Implemented (FIND-001) |

### Cryptographic Primitives

| Primitive | Status |
|-----------|--------|
| ✅ Ed25519 signatures | Implemented |
| ✅ BLAKE3 hashing | Implemented |
| ✅ zk-SNARKs (arkworks R1CS + Groth16 + Poseidon) | Implemented (BLAKE3-derived round constants) |
| ✅ CRYSTALS-Dilithium (PQC signatures) | Implemented (real verification) |
| ✅ Shamir's Secret Sharing (GF(256)) | Implemented |

### Known Security Gaps

1. **ZK circuit hash round constants** — Poseidon hash uses BLAKE3-derived round constants instead of Filecoin/Neptune reference; needs audit
2. **No Sybil resistance** — no staking requirement for validators
3. **Groth16 trusted setup** — no multi-party ceremony coordination
4. **Single primary developer** — bus factor of 1
5. **No formal verification beyond bounded TLA+** — unbounded proofs are Phase 2+

---

## Future Enhancements — 🔮 Aspirational

### Quantum Resistance
- ✅ CRYSTALS-Dilithium (signatures) — implemented
- ✅ Creator-pubkey binding — constant-time validation (FIND-003)
- ✅ Encrypted key storage — AES-256-GCM + HKDF-SHA256 (FIND-010)
- ✅ BLAKE3 domain separation — context-specific hashing (FIND-022)
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

## References

- Lamport, L. (1978). "Time, Clocks, and the Ordering of Events in a Distributed System"
- Shapiro, M., & Preguiça, N. (2011). "Conflict-free Replicated Data Types"
- Ben-Sasson, E., et al. (2014). "Zerocash: Decentralized Anonymous Payments from Bitcoin"

**Status:** Architecture Specification — Partially Implemented
**Version:** 4.0.0

---
🔙 **Back**: [Architecture Index](./) | 🔄 **Related**: [Trait Boundaries](./trait-boundaries.md)
🚀 **Next**: [Pipeline Design](./pipeline-design.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

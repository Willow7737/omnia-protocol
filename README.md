<p align="center">
  <img src="./assets/banner.png" alt="Omnia Protocol Banner" width="100%">
</p>

<p align="center">
  <a href="https://github.com/Willow7737/omnia-protocol/actions/workflows/ci.yml">
    <img src="https://github.com/Willow7737/omnia-protocol/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI Status">
  </a>
  <img src="https://img.shields.io/badge/Status-Live_Testnet-00ff88?style=for-the-badge&logo=github" alt="Status">
  <img src="https://img.shields.io/badge/Tests-Run_cargo_test-00ff88?style=for-the-badge&logo=rust" alt="Tests">
  <img src="https://img.shields.io/badge/Lines-85,000+-ff6b6b?style=for-the-badge&logo=rust" alt="Lines">
  <img src="https://img.shields.io/badge/License-CC0_Public_Domain-ff6b6b?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/Rust-1.91-orange?style=for-the-badge&logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/Phases_0--4-Complete-brightgreen?style=for-the-badge" alt="Phases 0-4">
  <img src="https://img.shields.io/badge/Phase_5-Validated-yellow?style=for-the-badge" alt="Phase 5">
  <img src="https://img.shields.io/github/stars/Willow7737/omnia-protocol?style=for-the-badge&color=gold" alt="GitHub Stars">
</p>

<p align="center">
  <a href="https://www.producthunt.com/products/omnia-protocol-2?embed=true&utm_source=badge-featured&utm_medium=badge&utm_campaign=badge-omnia-protocol-2" target="_blank" rel="noopener noreferrer"><img alt="Omnia Protocol - Parallel consensus for the next generation of Blockchains | Product Hunt" width="250" height="54" src="https://api.producthunt.com/widgets/embed-image/v1/featured.svg?post_id=1215154&theme=light&t=1786376330702"></a>
</p>

<h1 align="center">Omnia Protocol</h1>

<p align="center">
  <strong>The Universal Coordination Layer for Reality</strong><br>
  <em>A settlement-agnostic protocol that replaces trust with mathematics, using causal graph consensus for parallel transaction processing.</em>
</p>

[![Discord](https://img.shields.io/badge/Discord-5865F2?style=flat&logo=discord&logoColor=white&color=black)](https://discord.gg/qYkpAeSYR)

---

## 🚪 Choose Your Path 

| If you are...           | Start Here                                                                     | Next Step                                                                      |
| ----------------------- | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| 🌱 New to Omnia         | [docs/use-cases/](docs/use-cases/)                                             | [Quick Start](#-quick-start)                                                   |
| 💻 Contributor          | [CONTRIBUTING.md](CONTRIBUTING.md)                                             | [docs/architecture/](docs/architecture/)                                       |
| 🏗️ Systems Architect    | [docs/reference/blueprint-reference.md](docs/reference/blueprint-reference.md) | [docs/architecture/trait-boundaries.md](docs/architecture/trait-boundaries.md) |
| 📦 Validator Operator   | [docs/building/feature-matrix.md](docs/building/feature-matrix.md)             | [External validator onboarding](docs/operations/validator-setup.md#external-validator-onboarding) |
| 📱 Wallet user          | [Omnia Wallet](https://github.com/Willow7737/Omnia-Wallet)                     | [Live node](#-live-right-now)                                                  |
| 📊 Performance Engineer | [docs/reference/benchmark-gates.md](docs/reference/benchmark-gates.md)         | [docs/architecture/pipeline-design.md](docs/architecture/pipeline-design.md)   |


---

## 🟢 Live Right Now

The Omnia stack is no longer only a codebase — a public testnet node is
serving traffic, with a full client ecosystem built against it:

| Piece | Where | Status |
| :---- | :---- | :----- |
| **Public testnet** | `https://78.47.43.136.sslip.io` (REST `/api/v1/*`, Swagger UI) | 🟢 Live — **5-node geo-distributed mesh**, 4 peers each, 3 regions (v0.1.76+, protocol `/omnia/4.0.0`) |
| **Mobile wallet** | [`Willow7737/Omnia-Wallet`](https://github.com/Willow7737/Omnia-Wallet) (Flutter, Android/iOS) | ✅ v1 shipped |
| **Web dashboard** | [`Willow7737/omnia-protocol-interface`](https://github.com/Willow7737/omnia-protocol-interface) (Next.js + Supabase) | ✅ Deployed |
| **Website** | [`Willow7737/omnia-web`](https://github.com/Willow7737/omnia-web) | ✅ Deployed |

The wallet talks to this node over three auth endpoints added in July 2026 (`node/src/api/wallet_auth.rs`):

- `POST /api/v1/auth/challenge` + `POST /api/v1/auth/login` — self-custody login: an on-device Ed25519 key signs a single-use nonce (`"omnia-auth:" + nonce`, verified with `verify_strict`); the node derives `did:omnia:` + `sha256(pubkey)[..32]`, registers it in the UBC quota system, and issues a JWT.
- `POST /api/v1/auth/register` — JWT-authenticated, idempotent DID registration for externally-minted JWTs (the wallet's Supabase sign-in mints node JWTs via an edge function).

Try it:

```bash
curl https://78.47.43.136.sslip.io/api/v1/node/info
curl https://78.47.43.136.sslip.io/readyz
```

The full loop — create wallet → challenge/login → registered DID with a 1,000 UBC monthly quota → send → history — is verified end-to-end against this node (see the wallet repo's `tool/e2e_wallet_auth.dart`).

### 🌍 The standing network (as of 2026-08-10)

A **5-node geo-distributed validator mesh** is running continuously —
this is a standing network, not a mesh brought up for a benchmark:

| Node | Region | Role | Peers |
| :--- | :----- | :--- | :---- |
| **A** `78.47.43.136` | eu-central (Nuremberg, nbg1) | bootstrap + validator + public ingress | 4 |
| **B** `178.156.163.211` | us-east (Ashburn, ash) | validator | 4 |
| **C** `5.223.85.30` | ap-southeast (Singapore, sin) | validator | 4 |
| **D** `46.62.218.24` | eu-central (Helsinki) | validator | 4 |
| **E** `46.224.103.217` | eu-central (Falkenstein) | validator | 4 |

All five on v0.1.76+, stake 1 each. Three regions (eu-central, us-east,
ap-southeast) with two continents. Only node A exposes HTTP publicly; B–E
are validators reachable over the P2P mesh but not the REST API.

The mesh is fully peered and currently idle — nothing has been submitted to
it since the v0.1.95 rollout, so the Lane 0 counters read zero:

```jsonc
// GET /api/v1/node/info  (node A)
{ "peers": 4, "version": "0.1.95", "protocol_version": "4.0.0",
  "lane0": { "acks_accepted": 0, "acks_rejected": 0, "events_finalized": 0 } }
```

Zero counters here mean no traffic, not a stalled lane. What Lane 0 does
under load is recorded in
[benchmark-gates.md](docs/reference/benchmark-gates.md): 10k-event bursts
reaching 100% propagation and full quorum finality across all five nodes.

**Readiness contract.** `/readyz` reports the node as ready when it is
operational for traffic: it has the configured minimum peer count and is
not in fast-sync. It does **not** require recent traffic, Lane 1 canonical
commits, or Lane 0 preconfirmations, so quiet networks stay ready:

```jsonc
{ "status": "ready", "peers": 4, "finalized_height": 0, "lane0_enabled": true, "lane0_finalized_events": 0 }
```

Use `/api/v1/node/info` and Prometheus finality metrics to monitor Lane 0
fast-path progress and Lane 1 canonical commits. A zero `finalized_height`
on `/readyz` can be normal on an idle network; `/readyz` should fail only
for reachability/sync blockers such as `no_peers` or `syncing`.

**Utilization:** the node process uses ~30 MB RSS on a 16 GB host, with
load average flat at 0.00 over 31 days and 4.7 MB of chain data. Even at
the documented 10k-burst peak (145 MB RSS) that is ~1% of available
memory. The fleet is heavily over-provisioned; the smallest instances
available would carry this network comfortably.

**July 2026 throughput milestones** — measured on a real 5-node mesh, not
simulated, during stress runs on the dates given. These are load results
from a mesh sized for the benchmark; the standing network above is the production 5-node one (see [benchmark-gates.md](docs/reference/benchmark-gates.md)):

- **5-node gossipsub mesh over QUIC** — 1,000-event bursts propagate to
  100% of nodes in ~10 s; 5,000-event bursts in ~40–45 s, zero loss.
- **Real BFT finality (ADR-025 Lane 0)** — every event collects a
  quorum of signed validator acks: **10,000/10,000 events finalized
  across all 5 validators** in the latest stress run (2026-07-19).
- **Self-healing anti-entropy with fast drain** — nodes exchange
  frontier digests and repair any events lost to bounded-queue drops,
  chaining repair batches while behind. Proven at 10k scale on the full
  5-node mesh: a 10,000-event burst overwhelms live gossip, then repair
  recovers every event — **100% propagation + full quorum finality on
  all five nodes, zero loss, median convergence under a minute.**
- **Proven on the real internet (2026-07-20)** — a 3-region WAN
  validator network (Nuremberg / Ashburn / Singapore, RTTs up to
  ~218 ms) sustained the same test: **100% propagation + full quorum
  finality at 1k/5k/10k bursts, zero loss.** No more single-host
  asterisk.
- Formally specified: TLA+ models (`OmniaTwoLane`, `OmniaConsensus`,
  `OmniaCRDT`) model-checked in CI on every PR.

---

## 🏗️ The Omnia Architecture

```
┌──────────────────────────────────────────────────┐
│  LAYER 5: Economics (UBC, Governance)           │ ✅ IMPLEMENTED
├──────────────────────────────────────────────────┤
│  LAYER 4: Identity (DIDs, Shamir, Bio)          │ ✅ IMPLEMENTED
├──────────────────────────────────────────────────┤
│  LAYER 3: Binding (Provenance, RF, QC)          │ ✅ IMPLEMENTED
├──────────────────────────────────────────────────┤
│  LAYER 2: Domain Shards (6 shards)              │ ✅ IMPLEMENTED
├──────────────────────────────────────────────────┤
│  LAYER 1: Substrate (Causal Graph)              │ ✅ IMPLEMENTED
├──────────────────────────────────────────────────┤
│  PHASE 0: ZK-Rollup (Settlement Layer)          │ ✅ IMPLEMENTED
├──────────────────────────────────────────────────┤
│  THROUGHPUT OPT: Sharded State + Batch + Pool   │ ✅ SPRINTS 0-5
│  + Compact Encoding + Bloom + Priority Gossip   │
├──────────────────────────────────────────────────┤
│  PHASE 0 REMEDIATION: Critical Security Fixes   │ ✅ COMPLETE
│  + Feldman VSS DKG + SRS Binding + Auth Fixes   │
├──────────────────────────────────────────────────┤
│  NODE INTEGRATION: Background Consensus Loop    │ ✅ COMPLETE
│  + Pipeline Router Workers + P2P Network Task   │
├──────────────────────────────────────────────────┤
│  MERKLE TYPE SAFETY: Generic MerkleProof<H>     │ ✅ COMPLETE
│  + Blake3/Poseidon markers prevent mismatch     │
├──────────────────────────────────────────────────┤
│  CEREMONY API: Wired to CeremonyServer          │ ✅ COMPLETE
├──────────────────────────────────────────────────┤
│  PLACEHOLDER FIXES: Reject invalid proofs        │ ✅ COMPLETE
├──────────────────────────────────────────────────┘
```

---

## 🔬 What Is Omnia?

Omnia is not a company, a coin, or an app. It is a **protocol** — a fundamental set of rules that any computer can follow to participate in a shared, unchangeable record of truth. It uses **causal graph consensus** (DAG + vector clocks + CRDTs) instead of sequential blockchains to achieve parallel transaction processing. The protocol is **settlement-agnostic** by design — the `SettlementAdapter` trait admits any L1 with data availability and proof verification. **Ethereum is the only adapter with a real, working implementation** (Alloy, `ethereum-live` feature); Bitcoin, Solana, and Cosmos are trait-conforming stubs and Celestia is unverified plumbing.

### The Problem We Solve

| Challenge                   | Impact                               | Omnia's Solution                                    |
| :-------------------------- | :----------------------------------- | :-------------------------------------------------- |
| **Inefficient Blockchains** | High fees and energy waste           | Parallel causal graph consensus                     |
| **Broken Governance**       | Opaque decisions and ignored votes   | Quadratic voting + reputation decay                 |
| **Data Exploitation**       | Corporate profit from personal info  | User-controlled data via Zero-Knowledge Proofs      |
| **Opaque Supply Chains**    | Hidden child labor and fake medicine | Cryptographic birth certificates for physical items |
| **Centralized AI**          | Corporate control of models and data | Distributed training with shared rewards            |
| **Speculative Crypto**      | Wealth concentration and volatility  | Universal Basic Compute for all participants        |

---

## 🧪 Workspace

| Crate               | Purpose                                                                            | Status |
| ------------------- | ---------------------------------------------------------------------------------- | ------ |
| `omnia-primitives/` | Shared types: Event, VectorClock, wire format                                      | ✅     |
| `omnia-crypto/`     | Ed25519, BLS, deterministic hash selection, AES-GCM, keystore, PQC, DKG            | ✅     |
| `omnia-consensus/`  | Causal graph, consensus engine, mempool, CRDTs, slashing                           | ✅     |
| `omnia-network/`    | P2P networking: gossipsub, fast-sync, snapshots                                    | ✅     |
| `omnia-adapters/`   | ZK-rollup (arkworks R1CS + Groth16), settlement adapters                           | ✅     |
| `substrate/`        | Integration crate: causal graph, consensus, gossip, crypto, CRDTs, slashing (redb) | ✅     |
| `shards/`           | 6 domain shards + cross-shard messaging                                            | ✅     |
| `binding/`          | Provenance log, RF stub, hybrid PQC signatures                                     | ✅     |
| `economics/`        | UBC token, quota, governance, useful work                                          | ✅     |
| `node/`             | Binary entrypoint, REST API, health/metrics, consensus loop                        | ✅     |
| `chaos-tests/`      | Network partitions, crash recovery, byzantine, message loss                        | ✅     |
| `fuzz/`             | 12 fuzz harnesses (libfuzzer)                                                      | ✅     |
| `benches/`          | Throughput, ZK, IAI/Callgrind hot-path benchmarks                                  | ✅     |
| `tests/`            | Integration tests                                                                  | ✅     |

**Total: 224+ Rust source files, 83,000+ lines.** Run `cargo test --workspace` for current test counts.

---

## 🚀 Quick Start

```bash
git clone https://github.com/Willow7737/omnia-protocol.git
cd omnia-protocol
cargo test --workspace
cargo bench --no-run
```

---

## ✅ What's Implemented

### Layer 1: Substrate ✅

- Causal graph (DAG) with vector clock ordering
- Hashgraph-like two-parent events
- AlephBFT-inspired BFT finality
- CRDT state convergence (GCounter, OrSet, LWWRegister)
- libp2p gossip protocol (QUIC + GossipSub + mDNS)
- Ed25519 signatures with replay protection
- Performance: O(new_events) consensus processing (not O(n) graph walk)
- SlashingEngine with equivocation/liveness/invalid attestation detection
- Persistent slashing state via `redb` embedded database (`RedbSlashingStore`)
- Security: `state_root()`, `merkle_proof()`, `prune_old_events()`

### Layer 2: Domain Shards ✅

- 6 shards: Financial, Identity, Physical, Computational, Biological, Economics
- Shard router with automatic dispatch (`EventProcessor` trait)
- Cross-shard messaging with causality proofs
- Fee enforcement via FeeSchedule + QuotaSystem integration
- Security: Per-creator nonce replay protection (`last_nonces` in ShardRouter)
- FinancialShard uses strict causal ordering (not CRDTs) for balance consistency
- **Two distinct economies, both reachable over the API:**
  - **UBC** (`/api/v1/economics/*`) — soulbound monthly compute rights.
    Non-transferable by design: a "send" spends the sender's quota and
    credits nobody. This is what stops participation rights from being
    accumulated and concentrated.
  - **Financial ledger** (`/api/v1/financial/*`) — the transferable
    asset. `POST /financial/transfer` debits the sender and credits the
    recipient, conserving total supply. Authorized by the account
    holder's own Ed25519 signature over a domain-tagged message
    (`FinancialOp::SignedTransfer`), re-verified by every node that
    applies the event — so a relaying node can decline to forward a
    transfer but cannot forge or alter one. Accounts are addressed by
    public key, since a `did:omnia:` is a one-way hash of it.

### Layer 3: Binding Layer ✅

- Append-only provenance log (CRDT) with BLAKE3 hash-chain integrity
- Physical anchor (RF + quantum + provenance)
- ProvenanceTracker with create/transfer/verify/destroy lifecycle
- Hybrid PQC signatures (Ed25519 + CRYSTALS-Dilithium)
- PqcKeyRotationManager for post-quantum key rotation (3-phase migration)
- ⚠️ **STUB**: RF fingerprinting (needs SDR hardware; see [stub inventory](docs/stub-inventory.md))

### Layer 4: Identity Hardening ✅

- `did:omnia:` method with validation
- Shamir's Secret Sharing over GF(256)
- Privacy-preserving biometric anchors (BLAKE3(salt || template))
- AI agent identity with 5 capability types
- Social recovery with guardian threshold

### Layer 5: Economics ✅

- Universal Basic Compute (UBC) — soulbound monthly quota
- Quota system with epoch advancement
- Quadratic voting with exponential reputation decay
- Fixed-point governance decay (PPM arithmetic, no f64 in consensus)
- ⚠️ **STUB**: Proof-of-useful-work (3 work types defined, not production; see [stub inventory](docs/stub-inventory.md))

### Phase 0: ZK-Rollup ✅

- Settlement-agnostic architecture (`SettlementAdapter` + `SettlementLayer` traits)
- Ethereum adapter with Solidity contract (OmniaRollup.sol) — live mode via `ethereum-live` feature
- FFI settlement adapter for production C-library integration (`settlement-ffi` feature)
- ⚠️ **PARTIAL**: Celestia adapter — HTTP plumbing behind the `celestia` feature, never
  exercised against a real Celestia node and not instantiated anywhere
  (see [stub inventory](docs/stub-inventory.md))
- ⚠️ **STUB**: Bitcoin, Solana, Cosmos settlement adapters (see [stub inventory](docs/stub-inventory.md))
- L2 operator with batch builder (TOCTOU race condition fixed)
- ZK circuit (arkworks R1CS + Groth16 on BN254)
- Expanded circuit with Merkle path verification + per-event state transition constraints
- SRS-to-key derivation with cryptographic binding (`derive_keys_deterministic_from_srs`)
- Sparse Merkle tree proofs (BLAKE3 off-circuit)
- Event pruning for sustainability
- ⚠️ Placeholder: ExpandedRollupCircuit uses Poseidon hash (production-ready, but parameters use Cauchy MDS + BLAKE3 round constants, not Grain LFSR from paper)

### Phase 0 Remediation ✅

- **Coin round** integrated into fame determination (breaks split-vote deadlocks)
- **Feldman VSS DKG** replaces deprecated key aggregation (`FeldmanVssSession`)
- **SRS binding** in key derivation (`derive_keys_deterministic_from_srs`)
- **Multi-node integration test** with in-memory consensus simulation
- **Financial shard** burn authorization (creator must match `from`)
- **Physical shard** transfer authorization (caller must be current owner)
- **Bounded sequence tracking** (`max_sequence_entries` in `ConsensusConfig`)
- **RollupOperator** race condition fix (single atomic read lock)
- **BLS duplicate signer detection** (`aggregate_signatures_dedup`)
- **SLIP-0010 key derivation** (HMAC-SHA512, BIP-44 path for Ed25519)
- **Domain shard verification** (`real_verification` feature gate)
- **Chaos suite** fixes (byzantine equivocation actually generates conflicts)

---

## 🌑 What's Not Yet Implemented

| Feature                    | Status                  | Notes                                                |
| -------------------------- | ----------------------- | ---------------------------------------------------- |
| Real RF fingerprinting     | ⚠️ **STUB**             | Needs HackRF/USRP hardware                           |
| Bitcoin settlement adapter | ⚠️ **STUB**             | Implements trait, returns hardcoded values           |
| Solana settlement adapter  | ⚠️ **STUB**             | Implements trait, no-op methods                      |
| Cosmos settlement adapter  | ⚠️ **STUB**             | Implements trait, no-op methods                      |
| Proof-of-useful-work       | ⚠️ **STUB**             | 3 types defined, no real verification                |
| Mobile wallet              | ✅ **Shipped (v1)**     | [`Omnia-Wallet`](https://github.com/Willow7737/Omnia-Wallet) — dual-mode auth, live against the testnet node |
| Validator network          | ✅ **Running** (5 nodes) | Geo-distributed EU/US/Asia mesh — but all five run by the same operator, so not yet trust-distributed |
| Conviction voting          | 🌑 Not started          | Planned for post-testnet                             |
| Delegation                 | 🌑 Not started          | Planned for post-testnet                             |
| Production ZK hash gadget  | ✅ Poseidon implemented | Cauchy MDS + BLAKE3 round constants (not Grain LFSR) |

> **Full stub inventory**: See [docs/stub-inventory.md](docs/stub-inventory.md) for detailed documentation of all stubs and partial implementations.

---

## 📊 Transparency Dashboard

To uphold our commitment to radical transparency, we maintain a live dashboard of our progress, requirements, and team health.

- [**Project Dashboard**](docs/reference/project-dashboard.md) - High-level overview, team status, and risk assessment.
- [**Requirements & Status**](docs/reference/status.md) - Granular tracking of technical requirements and completion.

---

## 🗺️ Implementation Roadmap

### Phase 0: The Seed ✅ Complete

_Goal: Proof of Concept_

- ✅ Causal graph consensus (Rust, 68 primitives tests + 294 consensus tests)
- ✅ Self-sovereign identity system (DIDs, Shamir, biometrics)
- ✅ Universal Basic Compute (UBC)
- ✅ 6 domain shards with cross-shard messaging
- ✅ Settlement-agnostic ZK-rollup architecture
- ✅ Full ZK circuit (arkworks R1CS + Groth16 + Poseidon)
- ✅ Real PQC signatures (ML-KEM-768 / FIPS-203 algorithm; NIST certification of this Rust implementation is **not** claimed. PQC features require `--features pqc` and are not production-ready.)
- ✅ REST API with JWT auth, rate limiting, CORS
- ✅ Encrypted keystore (AES-256-GCM)
- ✅ Gradual slashing (3-tier: Warning → Jail → Ejection)
- ✅ BFT consensus with deterministic hash-based leader selection (ECVRF migration planned — see ADR-012 and `omnia-crypto/src/vrf.rs`)
- ✅ Docker 5-node testnet + monitoring stack

### Phase 1: Hardening ✅ Complete

_Goal: Code Quality_

- ✅ Typed error migration — 34 `thiserror` enums
- ✅ `unwrap()` replacement — `#![deny(clippy::unwrap_used)]` on all crates
- ✅ E2E REST API Integration Tests — 19 test functions
- ✅ Code coverage integration — `cargo llvm-cov` in CI
- ✅ RUSTSEC advisory review — stale ignores removed
- ✅ Documentation sprint — 50+ discrepancy fixes
- ✅ Solidity Groth16 Verifier — production-quality with BN254 precompiles
- ✅ Rustdoc coverage — 35 documentation items

### Phase 2: Cryptographic Key Management & ZK Hardening ✅ Complete

_Goal: Security Closure_

- ✅ SSS recovery flow with encrypted shares and key derivation
- ✅ Trusted setup ceremony with real EC scalar multiplication
- ✅ ZK circuit dummy fields populated with event semantics constraints
- ✅ ZK-SNARK benchmark suite
- ✅ Groth16 batch verification
- ✅ PQC key rotation with encrypted keystore
- ✅ Gradual slashing with jail/suspension and events
- ✅ BIP-39 mnemonic support
- ✅ DKG for threshold signatures (Feldman VSS)
- ✅ Complete sled removal (migrated to redb)
- ✅ ADRs 010–014

### Phase 3: Network Optimization & Security Closure ✅ Complete

_Goal: Production Network Readiness_

- ✅ SSS/DKG share encryption — XOR to AES-256-GCM
- ✅ ZK circuit trusted setup dummy values + transcript hash initialization
- ✅ Leader selection wired into consensus block production
- ✅ Kademlia DHT + NAT Traversal
- ✅ GossipSub peer scoring configuration
- ✅ Consensus state persistence across restarts
- ✅ Real Ethereum settlement adapter (Alloy, `ethereum-live` feature flag)
- ✅ ML-KEM-768 key encapsulation (FIPS-203 algorithm; implementation not NIST-certified. Requires `--features pqc`.)
- ✅ Fast-sync protocol with BLAKE3 checkpoints
- ✅ Message compression (Snappy for >256 bytes)
- ✅ Load testing infrastructure
- ✅ RUSTSEC advisory cleanup

### Phase 4: Mainnet Readiness ✅ Complete

_Goal: Production Hardening_

- ✅ Real Ethereum settlement with Alloy
- ✅ Gradual slashing implementation — ADR-011
- ✅ Migrate pqc_kyber → ml-kem (fix KyberSlash, FIPS-203)
- ✅ Fast-sync P2P automation
- ✅ Separate liveness and readiness probes
- ✅ Multi-party trusted setup ceremony automation
- ✅ Documentation sprint — ADRs 015–021
- ✅ Load testing baseline capture
- ✅ Supply chain hardening (cargo-vet, cargo-deny, SBOM)

### Phase 5: Testnet Launch & Validation ✅ Validated

_Goal: Performance Validation_

- ✅ Real performance benchmarking (12,000 ops/s (v0.1.68); ~7,190 ops/s (v0.1.48 historical) synchronous single-node; ~13.6× improvement over initial tokio-based measurements)
- ✅ Multi-node BFT testnet validation (3-node E2E via real libp2p)
- ✅ VRF migration to ECVRF per RFC 9381
- ✅ Genesis tooling — network bootstrap procedure
- ✅ Ethereum testnet integration test
- ✅ Poseidon dual-hash transition foundation
- ✅ Bug bounty program ($100–$50,000, 90-day embargo)
- ✅ External audit preparation package
- ✅ Side-channel audit for ZK and binding crates

### Phase 0 Throughput Optimization (Sprints 0–5) ✅ Complete

- ✅ Sprint 0: Baseline benchmarks, 3-node testnet Docker Compose, monitoring stack
- ✅ Sprint 1: `ShardedConsensusState` — 256-shard RwLock for parallel event processing
- ✅ Sprint 2: `BatchIngestor` + `ConsensusEventBatch` — amortized validation & proof generation
- ✅ Sprint 3: `EventPool` + `PruningAwarePool` — pre-allocated arena allocator with slot reuse
- ✅ Sprint 4: `CompactEncoder` + `GossipBloomFilter` + `PriorityGossipQueue` — optimized gossip
- ✅ Sprint 5: Integration test suite, 168h stability framework, full chaos suite, completion report
- ✅ Phase 0 Completion Report: [`docs/reports/phase0-completion-report.md`](docs/reports/phase0-completion-report.md)

**Optimization Results**: ~40% wire size reduction, O(1) duplicate detection, priority-based finality propagation, pre-allocated graph insertion with slot reuse.

### Post-Phase 5: Audit & Hardening 🔄 In Progress

- ✅ 7 high-priority audit findings remediated (BatchCrdtMerger rollback, GCounter overflow, constant-time VRF/BLS, DkgSession fixes, gossip topic mismatch, KeyStoreBridge persistence)
- ✅ 1 medium-priority finding remediated (signature dedup)
- ✅ Event submission now uses node's persistent keypair (not ephemeral)
- 🔄 14 medium-priority findings tracked (see [status.md](docs/reference/status.md))
- 📋 External security audit
- ✅ Public testnet endpoint **live** (single node at `78.47.43.136.sslip.io`, 0 peers)
- ✅ **Standing validator network — live.** A 5-node geo-distributed mesh
  (Nuremberg / Ashburn / Singapore / Helsinki / Falkenstein) runs
  continuously with 4 peers each. Measured RTTs match the benchmark baseline
  exactly. Five equal-stake validators put Lane 0 finality at 4-of-5 acks, so
  the network now tolerates one node down — at three nodes, quorum was all
  three and fault tolerance was zero. The mesh is fully peered but **idle**:
  no events have been submitted since the v0.1.95 rollout, so the Lane 0
  counters read zero. That is an absence of traffic, not an absence of
  liveness — the 10k-burst runs in
  [benchmark-gates.md](docs/reference/benchmark-gates.md) are what Lane 0
  does under load.
- 🔄 **Independent operators — not yet.** All five nodes are run by the
  same operator, so the network is geo-distributed but not yet
  trust-distributed. The external operator onboarding path is documented,
  but third-party validators are still the remaining step; see
  [validator-setup.md](docs/operations/validator-setup.md#external-validator-onboarding).

#### v0.1.69 Critical Security Hardening

16 critical security vulnerabilities identified and fixed in v0.1.69. See [SECURITY.md](./SECURITY.md) for the full list. Key fixes:

- Identity recovery `secret_commitment` — fixed in `shards/src/identity/recovery.rs`
- Biological ZK non-empty `public_inputs` — fixed in `shards/src/biological/validator.rs`
- Cross-shard causal proof verification — fixed in `shards/src/cross_shard.rs`
- Nonce store fail-closed — fixed in `shards/src/nonce_store.rs`
- Economics `verifier_pubkey` required — fixed in `economics/src/useful_work.rs`
- Ethereum `verify_proof_with_root` — fixed in `omnia-adapters/src/settlement/ethereum/live.rs`
- Per-client rate limiting — fixed in `substrate/src/rate_limiter.rs`
- `/readyz` peer tracking — fixed in `node/src/http.rs`
- Validator registration — fixed in `economics/src/economics_shard.rs`
- Dual `EconomicsState` — fixed in `economics/src/lib.rs`
- Shard ops bypass — fixed in `shards/src/shard.rs`
- Helm chart — fixed in `helm/omnia-node/`
- Substrate fallback — fixed in `substrate/src/lib.rs`
- Persistent node keypair — fixed in `node/src/main.rs`
- Genesis hex — fixed in `substrate/src/genesis.rs`
- Phase 2 ceremony — fixed in `omnia-adapters/src/setup/ceremony_server.rs`

### Phase 5.5: Live Node & Wallet Ecosystem (July 2026) ✅ Shipped

_Goal: Real users on a real node_

- ✅ Public testnet node deployed (`https://78.47.43.136.sslip.io`, Docker); Lane 0
  multi-node finality validated in stress runs, not continuously running
- ✅ Wallet challenge/signature auth — `POST /api/v1/auth/challenge` + `/auth/login` (Ed25519, single-use TTL nonces, domain-separated messages, `verify_strict`, auto DID registration)
- ✅ `POST /api/v1/auth/register` — idempotent DID registration for externally-minted JWTs (DID taken from the verified JWT `sub`, never the request body)
- ✅ SHA-256 DID derivation shared with clients (`did:omnia:` + `sha256(pubkey)[..32]`, cross-repo pinned test vector)
- ✅ **Omnia Wallet v1** ([repo](https://github.com/Willow7737/Omnia-Wallet)): Flutter, dual-mode auth (self-custody keys or Google/GitHub/email via Supabase + `mint-node-jwt` edge function), balance/send/history with per-transaction detail, governance voting, QR send/receive, address book, team news feed with threaded replies and images, in-app notifications, biometric app lock — verified E2E against the live node
- ✅ Web dashboard + website deployed (`omnia-protocol-interface`, `omnia-web`)

### Future Phases

| Phase                     | Goal                                            | Status       |
| ------------------------- | ----------------------------------------------- | ------------ |
| Phase 6: Public Testnet   | Multi-node testnet, external audit              | 🔄 In progress (multi-node live; audit pending) |
| Phase 7: Mainnet          | Sybil resistance, GC, formal verification       | 📋 Planned   |
| Phase 8: Decentralization | Hardware mesh, production PoUW                  | 📋 Long-term |
| Phase 9: Universality     | Relativistic consensus, physical-digital fusion | 📋 Long-term |

---

## 📈 Honest Performance Numbers

> **L-19 fix (audit v0.1.68):** Hardware specifications were missing from
> the original benchmark table, making the numbers irreproducible. The
> "Hardware" column below is now mandatory. Numbers measured on the
> reference benchmark machine (AMD Ryzen 9 7950X, 64 GB DDR5-6000, Linux
> 6.8, `rustc 1.91.0`). Re-run `cargo bench --bench baseline_bench` on
> your own hardware for comparable figures.

| Metric                       | Measured                                    | Conditions                                                                                        | Hardware                                      |
| ---------------------------- | ------------------------------------------- | ------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| **Synchronous pipeline**     | ~12,000 ops/s (v0.1.68 baseline, `benches/baselines.json`) | Release build, single-node, no async                                                              | AMD Ryzen 9 7950X, 64 GB DDR5-6000, Linux 6.8 |
| **Async (tokio)**            | Obsoleted by sync benchmark                 | Previous tokio-based measurement was an artifact of async runtime overhead, not a consensus limit | AMD Ryzen 9 7950X, 64 GB DDR5-6000, Linux 6.8 |
| **Finality latency p50**     | 24.5 µs (v0.1.68 baseline, `benches/baselines.json`) | Synchronous, single-node                                                                          | AMD Ryzen 9 7950X, 64 GB DDR5-6000, Linux 6.8 |
| **Graph insert p50**         | 18 µs (Criterion benchmark, insertion only) | O(1) amortized, 0→1000 events                                                                     | AMD Ryzen 9 7950X, 64 GB DDR5-6000, Linux 6.8 |
| **Ed25519 verify**           | ~27,000 sig/s (est., test timing)           | Standalone                                                                                        | AMD Ryzen 9 7950X, 64 GB DDR5-6000, Linux 6.8 |
| **Groth16 prove (expanded)** | ~88 ms/event (Criterion benchmark)          | BN254, R1CS                                                                                       | AMD Ryzen 9 7950X, 64 GB DDR5-6000, Linux 6.8 |
| **Groth16 verify**           | ~2.7 ms (Criterion benchmark)               | Single proof                                                                                      | AMD Ryzen 9 7950X, 64 GB DDR5-6000, Linux 6.8 |
| **VRF compute**              | ~19 µs (Criterion benchmark)                | Ed25519 + BLAKE3                                                                                  | AMD Ryzen 9 7950X, 64 GB DDR5-6000, Linux 6.8 |
| **CRDT batch merge**         | ~100K ops/s (est., no dedicated benchmark)  | 1K ops/batch                                                                                      |

> Numbers marked (est.) are approximate and environment-dependent, not from rigorous Criterion benchmarks. Numbers marked (Criterion benchmark) or (v0.1.48 micro-benchmark) come from reproducible benchmark suites. Real-world throughput will be lower due to network latency, BFT supermajority requirements, and ZK proof generation overhead. For reproduction, run: `cargo bench --bench baseline_bench`, `cargo bench --bench throughput`, `cargo bench --bench zk_benchmarks --features full`

### ZK Throughput Bottleneck

The consensus pipeline can process ~12,000 ops/s (v0.1.68 baseline), but Groth16 proof generation for the expanded circuit runs at ~88 ms/event (~11.4 events/sec). This creates a **~560× throughput gap** between consensus and ZK settlement. In practice, ZK rollups will batch events and generate proofs asynchronously — the pipeline design decouples consensus from proof generation so that slow proving does not block transaction finality. This is the expected trade-off for Groth16 on BN254 and will be addressed with hardware acceleration, proof aggregation, or alternative SNARK constructions in future phases.

> **Sub-linear ZK scaling (2026-06-23):** The previously-rumoured "27× superlinear scaling" was investigated and debunked. Actual ZK proving scales **sub-linearly**: per-event cost decreases from ~125 ms (1 event) to ~79 ms (100 events) as batching amortizes fixed costs. See [`docs/benchmarks/zk-scaling-analysis.md`](docs/benchmarks/zk-scaling-analysis.md) for the full analysis.

---

## 📚 Documentation

Full documentation is organized in [docs/](docs/):

- [**docs/architecture/**](docs/architecture/) — Layer specifications, trait boundaries, pipeline design, CRDT proofs, consensus queue
- [**docs/building/**](docs/building/) — Feature flags, cross-compilation, binary optimization
- [**docs/operations/**](docs/operations/) — Validator setup, monitoring, deployment, runbook, CLI & API
- [**docs/reference/**](docs/reference/) — Roadmap, benchmarks, ADR index, security audit, dependency policy, economic analysis, phase reports
- [**docs/use-cases/**](docs/use-cases/) — Real-world scenarios, FAQ, governance

Quick links:

- [**Architecture**](docs/architecture/) — Links to canonical architecture docs
- [**Implementation Spec**](docs/reference/implementation-spec.md) — Protocol specifications
- [**Research**](./substrate/RESEARCH.md) — Consensus research and implementation results

---

## 🤝 Contributing

Omnia thrives through open collaboration. This is a **Rust-only codebase**. See [**CONTRIBUTING.md**](./CONTRIBUTING.md) for guidelines.

```bash
cargo test --workspace    # Run all tests
cargo clippy -- -D warnings  # Lint
cargo fmt --check         # Check formatting
```

---

## 💬 Community & Support

Omnia is a public-interest protocol. Join the conversation:

- **[GitHub Discussions](discussions)** - Questions, ideas, and general community interaction.
- **[GitHub Issues](issues)** - Bug reports, feature requests, and technical research proposals.
- **[Project Dashboard](docs/reference/project-dashboard.md)** - Real-time project health and status updates.
- **[Requirements & Status](docs/reference/status.md)** - Granular tracking of technical requirements.
- **[Discord](https://discord.gg/qYkpAeSYR)** - Real-time chat and community.
- **Email:** `GitHub Security Advisory (https://github.com/Willow7737/omnia-protocol/security/advisories/new)` (for conduct-related issues)

---

## 📜 License

**Public Domain (CC0)** — No entity owns this protocol. Use it freely. Build on it. Improve it.

---

<p align="center">
  <strong>Omnia is the infrastructure for a future where trust is mathematically guaranteed.</strong>
</p>

<p align="center">
  <a href="https://ko-fi.com/U4C324U81N"><img src="https://ko-fi.com/img/githubbutton_sm.svg" alt="ko-fi"></a>
</p>

<p align="center">
  <a href="https://www.buymeacoffee.com/willow7737"><img src="https://img.buymeacoffee.com/button-api/?text=Support us&emoji=&slug=willow7737&button_colour=FFDD00&font_colour=000000&font_family=Lato&outline_colour=000000&coffee_colour=ffffff" /></a>
</p>


## Ghana-first OMNIA financial path

The dev branch now contains the first end-to-end financial path for the Ghana pilot. **UBC remains a free, epoch-reset, non-transferable participation allowance. OMNIA is the transferable native asset**, with nine decimal places and a floating value. The pilot distribution rail is Ghana mobile money; it allocates existing treasury inventory and does not auto-mint or promise a fixed GHS redemption rate.

### Quote-backed acquisition

Clients cannot submit an exchange rate, fee, quantity, caller role, or payment-success assertion. The node computes the economic terms, signs the quote with Ed25519, stores the quote context, binds initiation to the authenticated caller, and rejects expired or already-consumed quotes.

| Endpoint | Auth boundary | Purpose |
| :--- | :--- | :--- |
| `POST /api/v1/payment-orders/quote` | Wallet JWT | Generate a signed quote from GHS pesewas and `Mtn`, `Telecel`, or `At`. |
| `POST /api/v1/payment-orders/initiate` | Wallet JWT | Initiate an order from `quote_id`; all economics come from server-side quote storage. |
| `POST /api/v1/payment-orders/callback` | Provider HMAC + service role | Verify provider signature, replay protection, order binding, and amount binding. |
| `GET /api/v1/payment-orders/:id` | Wallet JWT | Read the authenticated caller's authoritative order snapshot. |
| `POST /api/v1/payment-orders/:id/advance` | Registered internal service role | Perform only an authorization-matrix-approved state transition. |

The payment engine persists event-sourced snapshots and side-effect records. Treasury reservations, consumption, release, provider references, and callback outcomes are represented explicitly so recovery is idempotent. The state machine has twenty-five states, and only the delivery states are economically successful; failures and refunds never masquerade as delivery.

### Merchant settlement interface

Merchant onboarding and QR/invoice creation are available through `/api/v1/merchants/register` and `/api/v1/merchants/:id/payment-request`. Merchant owners can read `/api/v1/merchants/:id/payments` and `/api/v1/merchants/:id/receipt/:payment_id`. A delivery service can confirm a payment only with the registered `delivery-service` service role. Merchant QR payloads carry a GHS price, quote expiry, OMNIA amount, payment ID, and optional Ed25519 settlement public key; the wallet signs the resulting transferable OMNIA payment locally.

### Runtime services

`AppState` now shares the quote service, event-sourced payment store, service-role registry, Ghana sandbox provider, and in-memory merchant registry across production and HTTP test fixtures. The Ghana provider uses HMAC-SHA256 callback authentication and constant-time signature comparison. The treasury bucket path consumes approved pre-minted, unassigned inventory before minting any shortfall, and the asset-registry suite includes a no-double-mint invariant test.

The protocol remains explicit about its launch boundary: the Ghana provider is a sandbox adapter until a regulated production mobile-money integration, operational secret management, reconciliation process, refund policy, and legal review are completed. The code path is therefore suitable for controlled testnet/pilot validation, not a claim that OMNIA is already legal tender or redeemable at a fixed GHS rate.


## Financial runtime hardening

The Ghana-first financial path now supports a durable `RedbPaymentStore` when `OMNIA_PAYMENT_STORE_PATH` is configured. It persists payment events, snapshots, and side-effect markers atomically so provider callbacks, refunds, treasury reservations, and delivery operations can be retried idempotently after restart. The live node also runs a conservative recovery sweep that reconstructs active orders and reports replay failures without claiming provider success or chain delivery.

Set `OMNIA_RUNTIME_MODE=production` to enable fail-closed startup validation. Production mode requires a durable payment-store path plus non-placeholder quote-signing and Ghana-provider secrets. Development and test nodes may retain the in-memory store and sandbox adapter. Deployment, backup, reconciliation, refund, chain-delivery, and Ghana compliance requirements are documented in [`docs/financial/production-readiness.md`](docs/financial/production-readiness.md).

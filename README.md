<p align="center">
  <img src="./assets/banner.png" alt="Omnia Protocol Banner" width="100%">
</p>

<p align="center">
  <a href="https://github.com/Willow7737/omnia-protocol/actions/workflows/ci.yml">
    <img src="https://github.com/Willow7737/omnia-protocol/actions/workflows/ci.yml/badge.svg" alt="CI Status">
  </a>
  <img src="https://img.shields.io/badge/Status-Active_Development-00ff88?style=for-the-badge&logo=github" alt="Status">
  <img src="https://img.shields.io/badge/Tests-200%2B_Passing-00ff88?style=for-the-badge&logo=rust" alt="Tests">
  <img src="https://img.shields.io/badge/License-CC0_Public_Domain-ff6b6b?style=for-the-badge" alt="License">
  <img src="https://img.shields.io/badge/Rust-1.75%2B-orange?style=for-the-badge&logo=rust" alt="Rust">
  <img src="https://img.shields.io/github/stars/Willow7737/omnia-protocol?style=for-the-badge&color=gold" alt="GitHub Stars">
</p>

<h1 align="center">Omnia Protocol</h1>

<p align="center">
  <strong>The Universal Coordination Layer for Reality</strong><br>
  <em>A settlement-agnostic protocol that replaces trust with mathematics, using causal graph consensus for parallel transaction processing.</em>
</p>

[![Discord](https://img.shields.io/badge/Discord-5865F2?style=flat&logo=discord&logoColor=white&color=black)](https://discord.gg/qYkpAeSYR)

---

## 🏗️ The Omnia Architecture

```
┌─────────────────────────────────────────┐
│  LAYER 5: Economics (UBC, Governance)  │ ✅ IMPLEMENTED
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

---

## 🔬 What Is Omnia?

Omnia is not a company, a coin, or an app. It is a **protocol** — a fundamental set of rules that any computer can follow to participate in a shared, unchangeable record of truth. It uses **causal graph consensus** (DAG + vector clocks + CRDTs) instead of sequential blockchains to achieve parallel transaction processing. The protocol is **settlement-agnostic** — it can settle on Ethereum, Bitcoin, Solana, or any L1 with data availability and proof verification.

### The Problem We Solve

| Challenge | Impact | Omnia's Solution |
| :--- | :--- | :--- |
| **Inefficient Blockchains** | High fees and energy waste | Parallel causal graph consensus |
| **Broken Governance** | Opaque decisions and ignored votes | Quadratic voting + reputation decay |
| **Data Exploitation** | Corporate profit from personal info | User-controlled data via Zero-Knowledge Proofs |
| **Opaque Supply Chains** | Hidden child labor and fake medicine | Cryptographic birth certificates for physical items |
| **Centralized AI** | Corporate control of models and data | Distributed training with shared rewards |
| **Speculative Crypto** | Wealth concentration and volatility | Universal Basic Compute for all participants |

---

## 🧪 Workspace

| Crate | Purpose | Tests | Status |
|-------|---------|-------|--------|
| `substrate/` | Causal graph, consensus, gossip, crypto, CRDTs | 75+ | ✅ |
| `shards/` | 6 domain shards + cross-shard messaging | 33+ | ✅ |
| `binding/` | Provenance log, RF stub, quantum commitment stub | 41+ | ✅ |
| `economics/` | UBC token, quota, governance, useful work | 22+ | ✅ |
| `zk/` | Settlement-agnostic ZK-rollup, Ethereum adapter | 8+ | ✅ |

**Total: 200+ tests, all passing.**

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
- Security: `state_root()`, `merkle_proof()`, `prune_old_events()`

### Layer 2: Domain Shards ✅
- 6 shards: Financial, Identity, Physical, Computational, Biological, Economics
- Shard router with automatic dispatch (`EventProcessor` trait)
- Cross-shard messaging with causality proofs
- Security: Per-creator nonce replay protection (`last_nonces` in ShardRouter)
- FinancialShard uses strict causal ordering (not CRDTs) for balance consistency

### Layer 3: Binding Layer ✅
- Append-only provenance log (CRDT)
- Physical anchor (RF + quantum + provenance)
- ProvenanceTracker with create/transfer/verify/destroy lifecycle
- ⚠️ Stubs: RF fingerprinting (needs SDR hardware), quantum commitments (needs pqc_dilithium)

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
- ⚠️ Stub: Proof-of-useful-work (3 work types defined, not production)

### Phase 0: ZK-Rollup ✅
- Settlement-agnostic architecture (`SettlementLayer` trait)
- Ethereum adapter with Solidity contract (OmniaRollup.sol)
- Bitcoin, Solana, Celestia stubs
- L2 operator with batch builder
- ⚠️ Stub: ZK circuit (hash chain, not full R1CS — arkworks integration is production target)
- Merkle state root + inclusion proofs
- Event pruning for sustainability

---

## 🌑 What's Not Yet Implemented

| Feature | Status | Notes |
|---------|--------|-------|
| Real ZK proofs | ⚠️ Stub | Full arkworks R1CS circuit is production target |
| Real PQC signatures | ⚠️ Stub | CRYSTALS-Dilithium integration pending |
| Real RF fingerprinting | ⚠️ Stub | Needs HackRF/USRP hardware |
| Fee mechanism | 🌑 Not started | UBC covers quotas, no transaction fees yet |
| Mobile wallet | 🌑 Not started | Planned for Phase 1 |
| REST API | 🌑 Not started | All interaction is via Rust library |
| Validator network | 🌑 Not started | Single-node operator for Phase 0 |
| Slashing | 🌑 Not started | Economic security not yet implemented |

---

## 📊 Transparency Dashboard

To uphold our commitment to radical transparency, we maintain a live dashboard of our progress, requirements, and team health.

- [**Project Dashboard**](./PROJECT_DASHBOARD.md) - High-level overview, team status, and risk assessment.
- [**Requirements & Status**](./STATUS.md) - Granular tracking of technical requirements and completion.

---

## 🗺️ Implementation Roadmap

### Phase 0: The Seed ✅ In Progress
*Goal: Proof of Concept*
- ✅ Causal graph consensus (Rust, 75+ tests)
- ✅ Self-sovereign identity system (DIDs, Shamir, biometrics)
- ✅ Universal Basic Compute (UBC)
- ✅ 6 domain shards with cross-shard messaging
- ✅ Settlement-agnostic ZK-rollup architecture
- 🔄 Full ZK circuit (arkworks R1CS)
- 🔄 Real PQC signatures (Dilithium)
- 🌑 Local testnet (5 nodes)

### Phase 1: The Root 📋 Planned
*Goal: Independence*
- 📋 Standalone validator network
- 📋 Real RF fingerprinting (SDR hardware)
- 📋 Fee mechanism
- 📋 Mobile wallet
- 📋 Conviction voting & delegation

### Phase 2: The Trunk 🔮 Long-term Vision
*Goal: Decentralization*
- 🔮 Quantum-resistant cryptography
- 🔮 Hardware mesh networks
- 🔮 Proof-of-useful-work (production)

### Phase 3: The Canopy 🔮 Long-term Vision
*Goal: Universality*
- 🔮 Relativistic consensus for interplanetary operation
- 🔮 Full physical-digital fusion
- 🔮 Post-human governance

---

## 📚 Documentation

- [**Architecture**](./ARCHITECTURE.md) - Technical deep-dives and layer specifications.
- [**Implementation**](./docs/specifications/IMPLEMENTATION.md) - Protocol specifications.
- [**Governance**](./docs/GOVERNANCE.md) - Community and decision-making.
- [**Use Cases**](./docs/USE_CASES.md) - Real-world applications.
- [**FAQ**](./docs/FAQ.md) - Common questions.
- [**Research**](./substrate/RESEARCH.md) - Consensus research and implementation results.

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

- **[GitHub Discussions](https://github.com/Willow7737/omnia-protocol/discussions)** - Questions, ideas, and general community interaction.
- **[GitHub Issues](https://github.com/Willow7737/omnia-protocol/issues)** - Bug reports, feature requests, and technical research proposals.
- **[Project Dashboard](./PROJECT_DASHBOARD.md)** - Real-time project health and status updates.
- **[Requirements & Status](./STATUS.md)** - Granular tracking of technical requirements.
- **[Discord](https://discord.gg/qYkpAeSYR)** - Real-time chat and community.
- **Email:** `conduct@omnia.protocol` (for conduct-related issues)

---

## 📜 License

**Public Domain (CC0)** — No entity owns this protocol. Use it freely. Build on it. Improve it.

---

<p align="center">
  <strong>Omnia is the infrastructure for a future where trust is mathematically guaranteed.</strong>
</p>
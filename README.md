# Omnia Protocol

**A settlement-agnostic universal coordination layer built on causal graph consensus.**

[![License](https://img.shields.io/badge/license-CC0-blue)](LICENSE)

## What Is Omnia?

Omnia is a decentralized protocol that replaces trust with mathematics. It uses **causal graph consensus** (DAG + vector clocks + CRDTs) instead of sequential blockchains to achieve parallel transaction processing. The protocol is **settlement-agnostic** — it can settle on Ethereum, Bitcoin, Solana, or any L1 with data availability and proof verification.

## Architecture

```
┌─────────────────────────────────────────┐
│  Layer 5: Economics (UBC, Governance)   │
├─────────────────────────────────────────┤
│  Layer 4: Identity (DIDs, Shamir, Bio) │
├─────────────────────────────────────────┤
│  Layer 3: Binding (Provenance, RF, QC) │
├─────────────────────────────────────────┤
│  Layer 2: Domain Shards (6 shards)     │
├─────────────────────────────────────────┤
│  Layer 1: Substrate (Causal Graph)     │
├─────────────────────────────────────────┤
│  Phase 0: ZK-Rollup (Settlement Layer) │
└─────────────────────────────────────────┘
```

## Workspace

| Crate | Purpose | Tests |
|-------|---------|-------|
| `substrate/` | Causal graph, consensus, gossip, crypto, CRDTs | 75+ |
| `shards/` | 6 domain shards + cross-shard messaging | 33+ |
| `binding/` | Provenance log, RF stub, quantum commitment stub | 41+ |
| `economics/` | UBC token, quota, governance, useful work | 22+ |
| `zk/` | Settlement-agnostic ZK-rollup, Ethereum adapter | 8+ |

**Total: 200+ tests, all passing.**

## Quick Start

```bash
git clone https://github.com/Willow7737/omnia-protocol.git
cd omnia-protocol
cargo test --workspace
cargo bench --no-run
```

## What's Implemented

### Layer 1: Substrate ✅
- Causal graph (DAG) with vector clock ordering
- Hashgraph-like two-parent events
- AlephBFT-inspired BFT finality
- CRDT state convergence (GCounter, OrSet, LWWRegister)
- libp2p gossip protocol (QUIC + GossipSub + mDNS)
- Ed25519 signatures
- Performance: O(new_events) consensus processing (not O(n) graph walk)
- Security: Replay protection via nonce tracking

### Layer 2: Domain Shards ✅
- 6 shards: Financial, Identity, Physical, Computational, Biological, Economics
- Shard router with automatic dispatch
- Cross-shard messaging with causality proofs
- Security: Per-creator nonce replay protection

### Layer 3: Binding Layer ✅
- Append-only provenance log (CRDT)
- Physical anchor (RF + quantum + provenance)
- ProvenanceTracker with create/transfer/verify/destroy lifecycle
- Stubs: RF fingerprinting (needs SDR hardware), quantum commitments (needs pqc_dilithium)

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
- Proof-of-useful-work stubs (3 work types)

### Phase 0: ZK-Rollup ✅
- Settlement-agnostic architecture (`SettlementLayer` trait)
- Ethereum adapter with Solidity contract (OmniaRollup.sol)
- Bitcoin, Solana, Celestia stubs
- L2 operator with batch builder
- ZK circuit stub (hash chain, not full R1CS)
- Merkle state root + inclusion proofs
- Event pruning for sustainability

## What's Not Yet Implemented

| Feature | Status | Notes |
|---------|--------|-------|
| Real ZK proofs | Stub | Full arkworks R1CS circuit is production target |
| Real PQC signatures | Stub | CRYSTALS-Dilithium integration pending |
| Real RF fingerprinting | Stub | Needs HackRF/USRP hardware |
| Fee mechanism | Not started | UBC covers quotas, no transaction fees yet |
| Mobile wallet | Not started | Planned for Phase 1 |
| REST API | Not started | All interaction is via Rust library |
| Validator network | Not started | Single-node operator for Phase 0 |
| Slashing | Not started | Economic security not yet implemented |

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — Technical deep-dive (updated)
- [CONTRIBUTING.md](CONTRIBUTING.md) — Development guidelines (updated)
- [substrate/RESEARCH.md](substrate/RESEARCH.md) — Consensus research
- [CHANGELOG.md](CHANGELOG.md) — Version history

## Community

- [GitHub Discussions](https://github.com/Willow7737/omnia-protocol/discussions) — Questions, ideas, community interaction
- [GitHub Issues](https://github.com/Willow7737/omnia-protocol/issues) — Bug reports, feature requests
- [Discord](https://discord.gg/qYkpAeSYR) — Real-time chat

## License

CC0 — Public Domain

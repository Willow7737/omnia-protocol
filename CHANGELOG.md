# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Layer 1: Substrate
- CausalGraph with DAG storage, vector clock ordering, topological sort
- ConsensusEngine with BFT finality (Hashgraph + AlephBFT hybrid)
- GossipProtocol with libp2p (QUIC, GossipSub, mDNS)
- Event with Ed25519 signatures, bincode serialization
- CRDTs: GCounter, OrSet, LWWRegister
- Performance: O(new_events) consensus via unprocessed queue
- Security: state_root(), merkle_proof(), prune_old_events()
- Replay protection via nonce tracking

### Layer 2: Domain Shards
- 6 shards: Financial, Identity, Physical, Computational, Biological, Economics
- ShardRouter with automatic dispatch
- Cross-shard messaging with causality verification
- Replay protection via per-creator nonce tracking

### Layer 3: Binding Layer
- ProvenanceLog (append-only CRDT)
- PhysicalAnchor (RF + quantum + provenance)
- ProvenanceTracker with full lifecycle
- RF fingerprinting stub (Hamming distance)
- Quantum commitment stub (hybrid classical + PQC)

### Layer 4: Identity Hardening
- did:omnia: method with validation
- Shamir's Secret Sharing over GF(256)
- BiometricAnchor (BLAKE3(salt || template))
- AgentIdentity with 5 capability types
- Social recovery with guardian threshold

### Layer 5: Economics
- UbcToken (soulbound, monthly quota)
- QuotaSystem with epoch advancement
- GovernanceState (quadratic voting + decay)
- UsefulWorkProof stubs (3 work types)

### Phase 0: ZK-Rollup
- Settlement-agnostic architecture (SettlementLayer trait)
- Ethereum adapter with Solidity contract
- Bitcoin, Solana, Celestia stubs
- L2 operator with batch builder
- ZK circuit stub (hash chain)

### Documentation
- Complete overhaul of all markdown files to match actual codebase
- Removed aspirational claims without labels
- Labeled all stubs and planned features honestly
- Updated build instructions, workspace structure, and test counts

## [1.0.0] - 2026-05-10

### Added

- Initial release of Omnia Protocol specification.
- Five-layer architecture definition.
- Causal graph consensus mechanism outline.
- Zero-Knowledge Proofs integration concept.
- Implementation roadmap (Phase 0 to Phase 3).
- Basic `README.md`, `CONTRIBUTING.md`, `LICENSE`, `SECURITY.md`.
- Initial diagrams for architecture, governance, supply chain, consensus comparison, and identity system.

### Changed

- Updated `README.md` with new banner, logo, and architecture visual.
- Enhanced `README.md` structure with Community & Support section.
- Added `CODE_OF_CONDUCT.md`.

### Removed

- Old banner image reference in `README.md`.

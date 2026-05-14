# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Sprint 3] - 2026-05-15

### Added

- zk: ExpandedRollupCircuit with Merkle path verification + per-event state transition constraints
- zk: Sparse Merkle tree module (BLAKE3 off-circuit, Fr↔hash conversions, MerkleProof struct)
- zk: Expanded prover/verifier for ExpandedRollupCircuit (Groth16 on BN254)
- substrate: SlashingStore trait + SledSlashingStore (sled-backed persistence) + InMemorySlashingStore
- substrate: SlashingEngine::with_store() for persistent slashing state across restarts
- node: omnia-node binary crate with CLI (clap), health/metrics HTTP (axum), graceful shutdown
- node: REST API with events/shards/governance/economics/node endpoints + utoipa Swagger UI
- chaos-tests: ChaosNetwork framework with partitions, crash recovery, byzantine, message loss
- chaos-tests: 4 integration test suites (partition.rs, crash_recovery.rs, byzantine.rs, message_loss.rs)
- formal-verification: TLA+ specification of consensus (Agreement, NoEquivocation, Validity invariants)
- docs/audit: AUDIT_SCOPE.md, ATTACK_SURFACE.md, SELF_ASSESSMENT.md, AUDIT_README.md

### Changed

- README: removed stale "hash chain stub" and "REST API not started" references
- README: added node/ and chaos-tests/ crates to workspace table
- README: updated "What's Not Yet Implemented" table with Sprint 3 completions
- CHANGELOG: added Sprint 3 section
- STATUS: updated completion tracking with Sprint 3 requirements

### ⚠️ Known Limitations

- ExpandedRollupCircuit uses simplified field-addition hash as placeholder for Pedersen/Poseidon
- TLA+ model uses finite state spaces (4 nodes, 3 rounds) — not exhaustive
- Chaos tests simulate at the library level, not real network I/O

## [Sprint 2] - 2026-05-14

### Added

- governance: replaced f64 decay with fixed-point PPM arithmetic (BasisPpmExt + DecayRate newtype)
- binding: replaced PQC verify() stub with real ed25519-dalek + pqc_dilithium hybrid verification
- shards: added FeeSchedule + QuotaSystem enforcement in ShardRouter
- substrate: added SlashingEngine with equivocation/liveness/invalid attestation detection
- substrate: added real libp2p QUIC multi-node integration test (#[ignore] + CI cron)
- zk: replaced hash-chain stub with arkworks R1CS + Groth16 proof system on BN254

### Changed

- economics: all consensus-critical arithmetic now uses integer fixed-point (no f64/f32)
- binding: PqPublicKey now carries separate ed25519 + dilithium key components
- shards: ShardRouter now deducts UBC fees before routing operations

## [Unreleased]

### ✅ Layer 1: Substrate
- CausalGraph with DAG storage, vector clock ordering, topological sort
- ConsensusEngine with BFT finality (Hashgraph + AlephBFT hybrid)
- GossipProtocol with libp2p (QUIC, GossipSub, mDNS)
- Event with Ed25519 signatures, bincode serialization
- CRDTs: GCounter, OrSet, LWWRegister
- 🚀 Performance: O(new_events) consensus via unprocessed queue
- 🛡️ Security: `state_root()`, `merkle_proof()`, `prune_old_events()`
- 🛡️ Replay protection via nonce tracking

### ✅ Layer 2: Domain Shards
- 6 shards: Financial, Identity, Physical, Computational, Biological, Economics
- ShardRouter with automatic dispatch (`EventProcessor` trait)
- Cross-shard messaging with causality verification
- Replay protection via per-creator nonce tracking (`last_nonces`)

### ✅ Layer 3: Binding Layer
- ProvenanceLog (append-only CRDT)
- PhysicalAnchor (RF + quantum + provenance)
- ProvenanceTracker with full lifecycle (create/transfer/verify/destroy)
- ⚠️ RF fingerprinting stub (Hamming distance)
- ✅ Hybrid PQC signatures (replaced quantum commitment stub — moved to Sprint 2)

### ✅ Layer 4: Identity Hardening
- `did:omnia:` method with validation
- Shamir's Secret Sharing over GF(256)
- BiometricAnchor (BLAKE3(salt || template))
- AgentIdentity with 5 capability types
- Social recovery with guardian threshold

### ✅ Layer 5: Economics
- UbcToken (soulbound, monthly quota)
- QuotaSystem with epoch advancement
- GovernanceState (quadratic voting + exponential decay)
- ⚠️ UsefulWorkProof stubs (3 work types)

### ✅ Phase 0: ZK-Rollup
- Settlement-agnostic architecture (`SettlementLayer` trait)
- Ethereum adapter with Solidity contract (OmniaRollup.sol)
- Bitcoin, Solana, Celestia stubs
- L2 operator with batch builder
- ✅ ZK circuit (replaced hash-chain stub — moved to Sprint 2)
- Merkle state root + inclusion proofs
- Event pruning for sustainability

### 📝 Documentation
- Complete overhaul of all markdown files to match actual codebase
- Honest labeling: stubs labeled ⚠️, planned features labeled 📋, aspirational content labeled 🔮
- Updated build instructions, workspace structure, and test counts
- Preserved visual design (banners, badges, ASCII art, emoji indicators)

## [1.0.0] - 2026-05-10

### Added 🎉

- Initial release of Omnia Protocol specification.
- Five-layer architecture definition.
- Causal graph consensus mechanism outline.
- Zero-Knowledge Proofs integration concept.
- Implementation roadmap (Phase 0 to Phase 3).
- Basic `README.md`, `CONTRIBUTING.md`, `LICENSE`, `SECURITY.md`.
- Initial diagrams for architecture, governance, supply chain, consensus comparison, and identity system.

### Changed 🔄

- Updated `README.md` with new banner, logo, and architecture visual.
- Enhanced `README.md` structure with Community & Support section.
- Added `CODE_OF_CONDUCT.md`.

### Removed ❌

- Old banner image reference in `README.md`.

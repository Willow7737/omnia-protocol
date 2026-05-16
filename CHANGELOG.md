# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-05-15

### Added

- substrate: CausalGraph with DAG storage, vector clock ordering, topological sort
- substrate: ConsensusEngine with BFT finality (Hashgraph + AlephBFT hybrid), VRF leader selection, round-based commit
- substrate: GossipProtocol with libp2p (QUIC, GossipSub, mDNS, request-response)
- substrate: Event with Ed25519 signatures, bincode serialization
- substrate: CRDTs: GCounter, OrSet, LWWRegister — all implement CvRDT trait
- substrate: VectorClock (BTreeMap<NodeId, u64>) with CvRDT merge for partition reconciliation
- substrate: SlashingEngine with equivocation (500pts), liveness (100pts), and invalid attestation (300pts) detection
- substrate: SledSlashingStore + InMemorySlashingStore for persistent slashing state
- substrate: SlashingUndoManager for governance-based reversal of slash decisions
- substrate: BLS12-381 signature aggregation (blst crate) for N-to-1 verification
- substrate: ThresholdKeyManager for t-of-n key sharing
- substrate: EncryptedKeyStore with rotation proofs
- substrate: Protocol version negotiation in P2P layer (VersionHandshake)
- substrate: State snapshot system (StateSnapshot::take/verify/serialize/deserialize)
- substrate: Event pruning by finalized round (CausalGraph::prune_finalized())
- substrate: Token-bucket rate limiter for event submission
- substrate: Crypto schemes abstraction (CryptoProfile with Hash/Signature/VRF/ZK schemes)
- shards: 6 domain shards — Financial, Computational, Physical, Biological, Identity, Economics
- shards: ShardRouter with automatic dispatch (implements EventProcessor trait)
- shards: Cross-shard messaging with causality verification (CrossShardMessage)
- shards: Replay protection via per-creator nonce tracking with sled persistence
- shards: Fee enforcement via FeeSchedule + QuotaSystem integration
- shards: Identity shard with DID (did:omnia:<hex>), Shamir recovery, biometric anchor, AI agent identity
- shards: Financial shard with causal account balances, transfer/mint/burn operations
- shards: Computational shard with task queue and proof registry
- shards: Physical shard with append-only provenance log
- shards: Biological shard with consent registry and ZK queries
- shards: Economics shard with UBC balances, epoch advancement, governance
- binding: RF Fingerprinting with PUF/RF-DNA spectral signatures (stub — needs SDR hardware)
- binding: Quantum-Resistant Commitments with Ed25519 + CRYSTALS-Dilithium hybrid signatures
- binding: Three-phase PQC migration: ClassicalOnly -> Hybrid -> PostQuantum
- binding: ProvenanceLog (append-only CRDT) with full lifecycle (Created -> Transferred -> Verified -> Destroyed)
- binding: PhysicalAnchor — unified verification of RF fingerprint + quantum commitment + provenance chain
- binding: PqcKeyRotationManager for post-quantum key rotation
- zk: Settlement-agnostic architecture (SettlementLayer trait)
- zk: Ethereum adapter with OmniaRollup.sol contract (deposit, withdrawal with 7-day challenge, batch submission)
- zk: Bitcoin, Solana, Celestia settlement adapters (stubs — return NotImplemented)
- zk: Groth16/Bn254 proof system with arkworks R1CS + Groth16
- zk: Poseidon SNARK-friendly hash function (BN254, t=3, R_F=8, R_P=57)
- zk: ExpandedRollupCircuit with Merkle path verification + per-event state transition constraints
- zk: PowersOfTau trusted setup ceremony (Phase 1) with multi-participant contributions
- zk: ProofBundle — chain-agnostic format with version, state roots, transition proof, L1 anchor
- zk: RollupOperator — collects finalized events, builds batch, generates Groth16 proof, settles on L1
- economics: UBC (Universal Basic Compute) — soulbound token, 1000 UBC/month default, non-transferable
- economics: QuotaSystem with 30-day epochs, register/spend/reward/balance_of/advance_epoch
- economics: Quadratic voting with reputation decay via PPM fixed-point arithmetic
- economics: Proof-of-Useful-Work with 3 types: AI Training, Scientific Simulation, Distributed Storage
- economics: TimeLockVoting for long-duration stake commitments
- economics: Fixed-point arithmetic (PPM) for cross-platform deterministic governance
- node: omnia-node binary with CLI (clap), health/metrics HTTP (axum), graceful shutdown
- node: REST API with events/shards/governance/economics/node endpoints + utoipa Swagger UI
- node: CLI subcommands: keygen, setup-contribute, setup-verify, snapshot, restore, run
- node: TOML configuration file support (NodeConfigFile, --config flag)
- node: --protocol-version CLI flag for advertising protocol version on the network
- node: Docker setup with multi-stage build, docker-compose for 5-node testnet
- chaos-tests: ChaosNetwork framework with partitions, crash recovery, byzantine, message loss
- chaos-tests: 4 integration test suites (partition.rs, crash_recovery.rs, byzantine.rs, message_loss.rs)
- fuzz: 11 libFuzzer targets with seed corpora and OSS-Fuzz Dockerfile
- formal-verification: TLA+ specification of consensus (Agreement, NoEquivocation, Validity invariants)
- formal-verification: TLA+ specification of CRDT convergence (GCounter, OrSet, LWWRegister)
- monitoring: Grafana dashboard with 9 panels + alert rules
- monitoring: Prometheus configuration + docker-compose integration
- CI: 5 GitHub Actions workflows (ci, benchmarks, chaos-tests, network-tests, nightly-fuzz)
- CI: Cross-platform testing (Ubuntu/macOS/Windows), cargo audit, cargo vet, SBOM generation, reproducible builds
- CD: release-please for automated versioning + changelog, release workflow for binary + contract + Docker publishing
- Supply chain: cargo-vet audits, CycloneDX SBOM, dependency policy

### Known Limitations

- Poseidon parameters use a Cauchy MDS matrix and BLAKE3-derived round constants (not the Grain LFSR from the paper)
- TLA+ CRDT model uses finite state spaces (3 nodes, MaxVal=3) — not exhaustive
- BLS key generation uses zero seed by default in tests — production must provide entropy
- EncryptedKeyStore uses XOR-based encryption (not AES-256-GCM) — production must upgrade
- RF fingerprinting is a stub (Hamming distance comparison) — needs SDR hardware
- Bitcoin/Solana/Celestia settlement adapters are stubs — return NotImplemented
- UsefulWorkProof verification is a stub (checks non-zero hash + positive compute units only)
- OmniaRollup.sol verifyProof is a Phase 0 stub (checks non-empty only)
- BiometricAnchor is a stub (BLAKE3 hash of salted template)
- sled 0.34 is alpha-quality — production deployments should migrate to rocksdb or redb

## [1.0.0-spec] - 2026-05-10

### Added

- Initial release of Omnia Protocol specification.
- Five-layer architecture definition.
- Causal graph consensus mechanism outline.
- Zero-Knowledge Proofs integration concept.
- Implementation roadmap (Phase 0 to Phase 3).
- Basic README.md, CONTRIBUTING.md, LICENSE, SECURITY.md.
- Initial diagrams for architecture, governance, supply chain, consensus comparison, and identity system.

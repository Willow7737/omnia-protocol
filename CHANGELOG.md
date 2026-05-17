# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.3](https://github.com/Willow7737/omnia-protocol/compare/v0.1.2...v0.1.3) (2026-05-17)


### Bug Fixes

* resolve all CI workflow failures ([d1b2d2b](https://github.com/Willow7737/omnia-protocol/commit/d1b2d2be751b4ce9ff4be0d4ec8d3e6b0ea79173))

## [0.1.2](https://github.com/Willow7737/omnia-protocol/compare/v0.1.1...v0.1.2) (2026-05-16)


### Features

* A-grade quality improvement plan — all 23 tasks across 4 sprints ([d231c10](https://github.com/Willow7737/omnia-protocol/commit/d231c1023be94e6b07e796cf4707c8ac9f3ced6d))

## [0.1.1](https://github.com/Willow7737/omnia-protocol/compare/v0.1.0...v0.1.1) (2026-05-16)


### Features

* **ci:** add CD pipeline with release-please and automated binary/contract/Docker publishing ([8f9d108](https://github.com/Willow7737/omnia-protocol/commit/8f9d10872d0f408af99a509aebfd4ca07193731d))
* Initial commit - Omnia Protocol universal coordination layer ([f5d035a](https://github.com/Willow7737/omnia-protocol/commit/f5d035a35d1579a970d0b0c5022076c02461c930))
* integrate Layer 1 Substrate implementation and enhance documentation structure ([e1212c4](https://github.com/Willow7737/omnia-protocol/commit/e1212c43dff244565616db3882a85e60201bb357))
* **integration:** wire Layer 2 shards into Layer 1 substrate — EventProcessor trait + committed event routing ([f49b72f](https://github.com/Willow7737/omnia-protocol/commit/f49b72f358a63696dbc069f83930625769d6577e))
* **layer3:** binding layer — provenance log + RF stub + quantum commitment stub ([62ca8d8](https://github.com/Willow7737/omnia-protocol/commit/62ca8d8b95432ddbb9a3b7fd67398ac4466f212f))
* **layer4:** identity hardening — Shamir recovery + biometric anchors + AI agent identity ([b0ea4df](https://github.com/Willow7737/omnia-protocol/commit/b0ea4dfecc928e26ce3a2c69ea69b59ffdf69879))
* **layer5:** economics — UBC token + proof-of-useful-work + quadratic voting with decay ([d33de8f](https://github.com/Willow7737/omnia-protocol/commit/d33de8f3b3733d4f01924e4d2c135f55fa7a14b7))
* **phase0:** settlement-agnostic ZK-rollup — Ethereum adapter + Bitcoin/Solana/Celestia stubs ([c703dd4](https://github.com/Willow7737/omnia-protocol/commit/c703dd43debb5598dfbe99ada48fdc3913011d12))
* Sprint 3 — Testnet Readiness ([7800d26](https://github.com/Willow7737/omnia-protocol/commit/7800d26b090e5f064c14f46886548d28d56d9eba))
* Sprint 4 — security hardening, formal verification, cryptographic maturity, and operational readiness ([aac338c](https://github.com/Willow7737/omnia-protocol/commit/aac338c67df2df7350da235e0b428e835681b601))
* Sprint 4 final push — nonce wiring, VRF stake weighting, Grafana alerts, ceremony PoK ([4ff1deb](https://github.com/Willow7737/omnia-protocol/commit/4ff1deb416c63c96b27d671094fda201a5dfd68e))
* Sprint 5 — fuzzing, proptests, supply chain hardening, reproducible builds ([0c1cb52](https://github.com/Willow7737/omnia-protocol/commit/0c1cb528bac5b536dcf6cd7aa84ac1a28fad8fc0))
* Sprint 6 — Complete the Foundation ([0f6ceff](https://github.com/Willow7737/omnia-protocol/commit/0f6ceffc2ab08cf24eae28c9070ae2ae61dd68c4))
* **sprint-1:** foundation hardening — consensus docs, event validati… ([df14119](https://github.com/Willow7737/omnia-protocol/commit/df141199c574aee35c989afa7120c18d4565855e))
* **sprint-1:** foundation hardening — consensus docs, event validation, adversarial tests, ProofBundle, CI/CD, ADRs ([0d51070](https://github.com/Willow7737/omnia-protocol/commit/0d5107006628714faf329ef2131e6aefd8af0192))
* **sprint-6:** Complete the Foundation — all 8 phases ([9a79f63](https://github.com/Willow7737/omnia-protocol/commit/9a79f6394a404b89af640b323b28cebf1211ba2f))
* **sprint-7:** Remediate ARGUS-PANOPTES audit findings ([51241d7](https://github.com/Willow7737/omnia-protocol/commit/51241d7af57640ed80b31596cb5090a792018e83))
* **sprint-7:** remediate ARGUS-PANOPTES audit findings — 2 critical, 3 high, 5 medium, 2 low/info ([56dbcfa](https://github.com/Willow7737/omnia-protocol/commit/56dbcfad09baefae1e2e8da851600de09a17d356))


### Bug Fixes

* add missing nonce_data_dir field in integration test NodeConfig ([0da370e](https://github.com/Willow7737/omnia-protocol/commit/0da370e8cae726be17ce600bfe8926665b6be946))
* add required toolchain input to dtolnay/rust-toolchain@v1 ([2cf1285](https://github.com/Willow7737/omnia-protocol/commit/2cf128550996a470443b71edef933ddf57269248))
* **ci:** cargo fmt + ignore RUSTSEC-2024-0384 (instant via sled) ([33e3542](https://github.com/Willow7737/omnia-protocol/commit/33e3542eb73ed0838ffc898f886bd19ee01d038b))
* **ci:** exclude fuzz crate from workspace test/clippy/doc/coverage, fix cargo-vet command ([5ad0b3d](https://github.com/Willow7737/omnia-protocol/commit/5ad0b3d495ba1c723f54dcc84fa65602156025be))
* **ci:** fix cargo-vet config format, make supply-chain job non-blocking ([cc571a9](https://github.com/Willow7737/omnia-protocol/commit/cc571a939df6f511f62d6d11d1ac9e0670fa8a03))
* **ci:** fresh rustup install on macOS to bypass broken Homebrew rustup ([03ea642](https://github.com/Willow7737/omnia-protocol/commit/03ea642f6abdf5c67b20b8c2e26bf630f8c9f38d))
* **ci:** ignore RUSTSEC-2025-0055 audit, fix macOS cargo PATH ([fc28e66](https://github.com/Willow7737/omnia-protocol/commit/fc28e66474dfaff73c46957606126ca72d86e6fb))
* **ci:** move with_random_seed out of impl Default, fix remaining fmt issues ([e76a3d6](https://github.com/Willow7737/omnia-protocol/commit/e76a3d6c762485d11c663ef1f412ebbd8f8a5400))
* **ci:** properly initialize cargo-vet supply-chain with exemptions and imports ([8766ebf](https://github.com/Willow7737/omnia-protocol/commit/8766ebf0c2fa93a83ca2440e052dcb91c083a465))
* **ci:** provide non-zero round_seed in chaos-tests ([04da88c](https://github.com/Willow7737/omnia-protocol/commit/04da88c4405bdfcdefa3c0b81018d79f3ed959e9))
* **ci:** provide non-zero round_seed in SubstrateConfig constructors ([b8fa3ef](https://github.com/Willow7737/omnia-protocol/commit/b8fa3ef504b0fb7158df6c0de01d513d29a45d54))
* **ci:** re-add cargo to PATH after rust-cache on macOS ([2547b98](https://github.com/Willow7737/omnia-protocol/commit/2547b98a1d27788b23cf1644bbdabcdd4740e793))
* **ci:** resolve all CD pipeline failures — release-please, cross-compile, Windows, publish ([73e4353](https://github.com/Willow7737/omnia-protocol/commit/73e43533c44261859e369608ec7fe4f2cc8c69e2))
* **ci:** resolve clippy warnings and fuzz install resilience ([9e9b095](https://github.com/Willow7737/omnia-protocol/commit/9e9b095814fec7eae9cbeb1e82e53e29352d252e))
* **ci:** resolve compilation, formatting, and CI workflow errors ([f82aa21](https://github.com/Willow7737/omnia-protocol/commit/f82aa2167f7b0810200ce75a65dfe7daa86f841e))
* **ci:** resolve fmt, clippy, and ZK circuit test failures ([3a0d0b0](https://github.com/Willow7737/omnia-protocol/commit/3a0d0b09b68aa9c4a299c45ba095f048145cafc9))
* **ci:** resolve fmt, compilation, and Cargo.lock issues ([43768a6](https://github.com/Willow7737/omnia-protocol/commit/43768a64c8cb17c3f32874184987508b99cfbfd2))
* **ci:** resolve macOS cargo PATH loss after rust-cache restore ([e7994c1](https://github.com/Willow7737/omnia-protocol/commit/e7994c117a3fbebebe9ae5c57d9b2495c67efcec))
* **ci:** resolve macOS cargo=rustup-init Homebrew conflict ([99b1daf](https://github.com/Willow7737/omnia-protocol/commit/99b1daf47a1a57a7378c75242964f4f0a41ea3d4))
* **ci:** resolve Python toml import error and cargo-fuzz manifest issue ([ad328b5](https://github.com/Willow7737/omnia-protocol/commit/ad328b54ef5dda81e40769cefa38de288b401451))
* **ci:** resolve remaining fmt, SBOM, and fuzz CI issues ([99d13ea](https://github.com/Willow7737/omnia-protocol/commit/99d13ea9ca06c557c15e156728cc0abf3b328b39))
* **ci:** resolve rustdoc broken intra-doc links ([df6ad3f](https://github.com/Willow7737/omnia-protocol/commit/df6ad3fbc548640ab354c311f25f4fa9f4c1927f))
* **ci:** resolve rustdoc redundant explicit link target in zk/src/lib.rs ([e480478](https://github.com/Willow7737/omnia-protocol/commit/e4804788fb5b33e2fed14f47fb12c4c6be4236be))
* **ci:** resolve rustdoc warnings treated as errors ([3429b14](https://github.com/Willow7737/omnia-protocol/commit/3429b14802e62389e83e1f6bb75b592d738e17af))
* **ci:** update remaining protocol identifier test assertion to 4.0.0 ([dd2c417](https://github.com/Willow7737/omnia-protocol/commit/dd2c4179d2bb72161867dcf4e65ec9917473740c))
* **ci:** update test expectations for protocol version and undo rate limit ([1ac950f](https://github.com/Willow7737/omnia-protocol/commit/1ac950fc854599c9b577141338918da092b2b5f6))
* **ci:** use dtolnay/rust-toolchain for all platforms including macOS ([98c1976](https://github.com/Willow7737/omnia-protocol/commit/98c19762070b43505d90733083aedd7cfc9ef5d4))
* **ci:** wrap governance error return in Err(), remove dead seen_sequences field ([364d114](https://github.com/Willow7737/omnia-protocol/commit/364d114bb9074469fc1d9b1273f6c7add761d4cd))
* clippy bench warnings, drop MSRV matrix (deps require 1.86+) ([fd8498c](https://github.com/Willow7737/omnia-protocol/commit/fd8498c53bd69595ac299234ffdad9562829714d))
* commit Cargo.lock for security auditing and reproducible builds ([d68ef8d](https://github.com/Willow7737/omnia-protocol/commit/d68ef8da5338e711c089e0e150852ad972217b95))
* commit Cargo.lock for security auditing and reproducible builds ([a600515](https://github.com/Willow7737/omnia-protocol/commit/a600515b21cf984e5f0ee44cffc9caae30b6893a))
* configure cargo audit to ignore known transitive dep vulnerabilities ([e90549f](https://github.com/Willow7737/omnia-protocol/commit/e90549fb0fede5f37c88582614fe7b4abc9373e6))
* configure cargo audit to ignore known transitive dep vulnerabilities ([53434d8](https://github.com/Willow7737/omnia-protocol/commit/53434d89118d3b62fd4c475184f5dbe611e3679f))
* **docs:** resolve rustdoc warnings that break CI with -D warnings ([504aa08](https://github.com/Willow7737/omnia-protocol/commit/504aa081d7f3b9d41f451585d0f76892fc3b0642))
* **layer3:** strengthen links_to, add destroy_item, explicit blake3 dep ([14d807e](https://github.com/Willow7737/omnia-protocol/commit/14d807e37040cb494752d8bc016839f88df2ed65))
* make PROOF_BUNDLE_VERSION public to fix rustdoc private-intra-doc-links ([cc11864](https://github.com/Willow7737/omnia-protocol/commit/cc11864104dcc5243361b1c18454aa1e6d553360))
* **pre-rollup:** 4 critical gaps — replay protection, state root, event pruning, economics wiring ([facd051](https://github.com/Willow7737/omnia-protocol/commit/facd0515af3821d984c567a89287efdfcf4297e9))
* resolve all CI failures — fmt, clippy, and dependabot config ([a1cc009](https://github.com/Willow7737/omnia-protocol/commit/a1cc00900e2b84b609ffaab007dcecd9633e4bee))
* resolve all CI failures — fmt, clippy, and dependabot config ([6a1f00d](https://github.com/Willow7737/omnia-protocol/commit/6a1f00d2b8e216b0093dc7e6e82ce4c01fcb38c3))
* resolve all remaining CI failures ([1ef8eda](https://github.com/Willow7737/omnia-protocol/commit/1ef8edae4178c634444b4bfdf832427c18228511))
* resolve all remaining CI failures (audit action + toolchain pinning) ([4937f16](https://github.com/Willow7737/omnia-protocol/commit/4937f16d3f0159dd1909bc5da3eacab147feab64))
* single persistent SlashingEngine shared between consensus and API ([e4b0b40](https://github.com/Willow7737/omnia-protocol/commit/e4b0b40e360136f5077aca3b2f5ccfcb02c39412))
* **sprint-1:** CI overhaul, multi-OS testing, Docker testnet, fuzz targets ([8467c93](https://github.com/Willow7737/omnia-protocol/commit/8467c9395a5ddd009a2bacf3b5e9dd743fecace2))
* **sprint-1:** CI overhaul, multi-OS testing, Docker testnet, fuzz targets ([cfd9113](https://github.com/Willow7737/omnia-protocol/commit/cfd911341d4ba90578a415afdd041c4783b1e80c))
* **sprint-2:** critical hardening — 6 security fixes ([b31d148](https://github.com/Willow7737/omnia-protocol/commit/b31d148899b4ff620ce0a78de819385db801cda3))
* **sprint-2:** Critical Hardening — 6 Security Fixes ([4e398fc](https://github.com/Willow7737/omnia-protocol/commit/4e398fce0cf4dfc9f819efbf9edb0dce3cf05e91))
* three hotfix sprint blocking issues ([4cfd97d](https://github.com/Willow7737/omnia-protocol/commit/4cfd97d0dcf628f1bd1542fec54e2f18ba795810))
* upgrade CI toolchain to Rust 1.85 ([9e45fd1](https://github.com/Willow7737/omnia-protocol/commit/9e45fd10c427d23eea730dd7a72e27a6cf91adfa))
* upgrade CI toolchain to Rust 1.85 ([5286bbe](https://github.com/Willow7737/omnia-protocol/commit/5286bbed2bc2b1bda65f7a54e5d0d340cf9b95f9))
* use stable Rust toolchain in CI ([bcab7ef](https://github.com/Willow7737/omnia-protocol/commit/bcab7ef33ee38d56b9636f2d56f932b6a869ef9a))
* use stable Rust toolchain in CI to resolve transitive dep MSRV issues ([0fa9700](https://github.com/Willow7737/omnia-protocol/commit/0fa9700e31898ed5894f145b22b82a6554267755))


### Performance

* **substrate:** O(n) → O(new_events) — replace graph walk with unprocessed event queue ([2f45765](https://github.com/Willow7737/omnia-protocol/commit/2f457655324259aa183c94668dd6bd4fe83bb750))


### Documentation

* comprehensive documentation audit — align all markdown with v4.0.0 codebase ([e37d6ef](https://github.com/Willow7737/omnia-protocol/commit/e37d6ef91c9c2b271ba99d6f970617dad7e8d591))
* comprehensive repository beautification (excluding workflows due to permissions) ([5d8088d](https://github.com/Willow7737/omnia-protocol/commit/5d8088de797abff9f926086e8fa4695b809d4b21))
* comprehensive repository beautification and enhancement ([52bca8e](https://github.com/Willow7737/omnia-protocol/commit/52bca8e7f3d6871509dded8034c093a9a719cf1d))
* configure community channels and issue templates ([9b038df](https://github.com/Willow7737/omnia-protocol/commit/9b038dfc503b3c9f7b8068ec2066f7630b89371f))
* implement radical transparency dashboard and status tracking ([d1bf6d7](https://github.com/Willow7737/omnia-protocol/commit/d1bf6d7690a6878fcceaca02134d458ef1aa3bac))
* overhaul all markdown — match docs to actual codebase ([2381c77](https://github.com/Willow7737/omnia-protocol/commit/2381c77c6b22684d2592394417b9b2e7706bb0c3))
* update all markdown — honest content + preserved visual design ([c2f8034](https://github.com/Willow7737/omnia-protocol/commit/c2f803415fcdaeddc9e2ede2caed2e6761544643))
* update Discord server link to https://discord.gg/qYkpAeSYR ([d4face4](https://github.com/Willow7737/omnia-protocol/commit/d4face4940ea600dab6c74c0af9f6589aedc48a6))
* update README with direct community and tracking links ([144b670](https://github.com/Willow7737/omnia-protocol/commit/144b67089ea069fc78be5b4bd33d16e36a11ddf3))


### Build

* **deps:** bump actions/checkout from 4 to 6 ([94120e8](https://github.com/Willow7737/omnia-protocol/commit/94120e8a63d9cd9c6125cc0a3fb8ae30f7b7e2c8))
* **deps:** bump actions/checkout from 4 to 6 ([fd771f8](https://github.com/Willow7737/omnia-protocol/commit/fd771f816fd62a3817bc85ca1cb5d5db6770ab68))
* **deps:** update bincode requirement from 1.3 to 3.0 ([ebc42a5](https://github.com/Willow7737/omnia-protocol/commit/ebc42a56c6abbaaefd29c7853d1c4e30f3af5899))
* **deps:** update bincode requirement from 1.3 to 3.0 ([ee4118d](https://github.com/Willow7737/omnia-protocol/commit/ee4118d371e456bfb9ce327b1867dc72df2bc6a4))
* **deps:** update criterion requirement from 0.5 to 0.8 ([bff04f3](https://github.com/Willow7737/omnia-protocol/commit/bff04f32609872c2746eae84f14936eee2e8410b))
* **deps:** update criterion requirement from 0.5 to 0.8 ([4652f14](https://github.com/Willow7737/omnia-protocol/commit/4652f148ecc266ed2d6ad62e245d4b8f644fdae8))
* **deps:** update libp2p requirement from 0.53 to 0.56 ([734156b](https://github.com/Willow7737/omnia-protocol/commit/734156b780c85244f97ee01814b9cf5c39aa4224))
* **deps:** update libp2p requirement from 0.53 to 0.56 ([bc7165a](https://github.com/Willow7737/omnia-protocol/commit/bc7165a6f6f7e8eb210a25470b5dfb43430b58fb))
* **deps:** update rand requirement from 0.8 to 0.10 ([259659f](https://github.com/Willow7737/omnia-protocol/commit/259659fabace680f308f875023c80902eb1cbe11))
* **deps:** update rand requirement from 0.8 to 0.10 ([709914a](https://github.com/Willow7737/omnia-protocol/commit/709914a79ec2cc0185c5038aa6af54f01b99f53e))
* **deps:** update sha2 requirement from 0.10 to 0.11 ([0646dcd](https://github.com/Willow7737/omnia-protocol/commit/0646dcd70951ad63f4603e6f8a5fe00efeafbc48))
* **deps:** update sha2 requirement from 0.10 to 0.11 ([a53aa93](https://github.com/Willow7737/omnia-protocol/commit/a53aa935e01aa4455414017bfc2287fdd178a4ef))
* **deps:** update thiserror requirement from 1.0 to 2.0 ([2073b7b](https://github.com/Willow7737/omnia-protocol/commit/2073b7b9be610b7e7efef38b8da1c16829d835fd))
* **deps:** update thiserror requirement from 1.0 to 2.0 ([12a762e](https://github.com/Willow7737/omnia-protocol/commit/12a762eafa50ef94221d3334b258e96d6c559497))


### CI

* **release-please:** add workflow_dispatch trigger for manual re-runs ([fa80aef](https://github.com/Willow7737/omnia-protocol/commit/fa80aef19c14bcb82bcd43fe43f08c2606af8498))

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

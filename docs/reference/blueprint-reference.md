# Blueprint Reference

> 🎯 Audience: Architects
> 🔗 Context: Implementation specification reference — status of all protocol components and milestones
> 📅 Last Updated: 2026-05-20

## Implementation Status

```
[███████████████████████████░] 96% Complete
```

| Layer                         | Status         | Tests         |
| ----------------------------- | -------------- | ------------- |
| Layer 1: Substrate            | ✅ Implemented | 454+          |
| Layer 2: Domain Shards        | ✅ Implemented | 62+           |
| Layer 3: Binding              | ✅ Implemented | 61+           |
| Layer 4: Identity (in shards) | ✅ Implemented | —             |
| Layer 5: Economics            | ✅ Implemented | 58+           |
| Phase 0: ZK-Rollup            | ✅ Implemented | 129+          |
| Node Binary                   | ✅ Implemented | 30+           |
| Chaos Tests                   | ✅ Implemented | ~15 scenarios |

**Total: 800+ lib tests + chaos/integration tests, all passing.**

## Technology Stack

### Core Crates (14 crates)

| Crate             | Purpose                                                                 | Key Dependencies                              |
| ----------------- | ----------------------------------------------------------------------- | --------------------------------------------- |
| `substrate/`      | Causal graph, consensus, gossip, crypto, CRDTs, slashing, snapshots     | ed25519-dalek, blake3, redb, libp2p, postcard |
| `shards/`         | 6 domain shards + cross-shard messaging + fee enforcement + nonce store | redb (nonce persistence)                      |
| `binding/`        | Provenance log, RF stub, quantum commitments, key rotation              | ed25519-dalek, ml-kem, aes-gcm, hkdf          |
| `economics/`      | UBC token, quota, governance, useful work, fixed-point                  | thiserror                                     |
| `omnia-adapters/` | ZK-rollup (arkworks R1CS + Groth16), settlement adapters                | ark-bn254, ark-groth16, alloy (feature-gated) |
| `node/`           | CLI binary, HTTP server, REST API, Swagger UI, Prometheus metrics       | axum, utoipa, clap, jsonwebtoken              |
| `chaos-tests/`    | Network partition, crash, drop-rate, equivocation simulation            | tokio                                         |

### Cryptographic Primitives

| Primitive          | Implementation                              | File Reference                    |
| ------------------ | ------------------------------------------- | --------------------------------- |
| Groth16 SNARK      | `ark-groth16` on BN254                      | `omnia-adapters/src/prover.rs`    |
| Poseidon hash      | Custom (Cauchy MDS + BLAKE3 RC) + Reference | `omnia-adapters/src/poseidon.rs`  |
| BLAKE3             | `blake3` 1.5 (domain-separated)             | Multiple files                    |
| Ed25519            | `ed25519-dalek` 2.1                         | `binding/src/quantum_commit.rs`   |
| CRYSTALS-Dilithium | `pqc_dilithium` 0.2                         | `binding/src/quantum_commit.rs`   |
| ML-KEM-768         | `ml-kem` 0.2 (FIPS-203)                     | `binding/src/quantum_commit.rs`   |
| BLS12-381          | `blst`                                      | `substrate/src/bls.rs`            |
| Shamir's SSS       | GF(256) with AES irreducible polynomial     | `shards/src/identity/recovery.rs` |
| VRF V2 (ECVRF)     | ECVRF construction (spec-compliant V2)      | `omnia-crypto/src/vrf.rs`         |
| ML-KEM-768         | `ml-kem` 0.2 (FIPS-203)                     | `binding/src/quantum_commit.rs`   |

### State Management

| Component         | Implementation                          | Persistence                 |
| ----------------- | --------------------------------------- | --------------------------- |
| CRDTs             | GCounter, OrSet, LWWRegister            | In-memory                   |
| Merkle state root | BLAKE3 off-circuit, Poseidon on-circuit | Snapshots                   |
| Slashing state    | `RedbSlashingStore`                     | redb (ACID)                 |
| Nonce tracking    | `RedbNonceStore`                        | redb (ACID)                 |
| Consensus state   | `RedbConsensusStore`                    | redb (ACID)                 |
| Keystore          | `EncryptedKeyStore`                     | AES-256-GCM encrypted files |

## What's Fully Implemented ✅

- Causal graph consensus with vector clock ordering
- 6 domain shards with cross-shard messaging
- Fee enforcement via FeeSchedule + QuotaSystem
- Replay protection with persistent nonce store
- Slashing engine with persistent redb storage (3-tier gradual model)
- Provenance tracking (full lifecycle)
- DID method (`did:omnia:`) with validation
- Shamir's Secret Sharing for social recovery (AES-256-GCM encrypted shares)
- Biometric anchors (BLAKE3-based)
- AI agent identity with 5 capability types
- UBC token (soulbound quota with reputation decay)
- Quadratic voting with exponential decay and time-locked voting
- Real ML-KEM-768 / FIPS-203 post-quantum key encapsulation
- Real Dilithium signature verification
- Real Groth16 ZK proving/verification with Poseidon hash
- Settlement-agnostic ZK-rollup with real Ethereum settlement (Alloy)
- Full node binary with CLI, REST API, Swagger UI, Prometheus metrics
- JWT authentication, ACL authorization, rate limiting, CORS
- Chaos testing framework
- Docker deployment with 5-node testnet + monitoring
- BIP-39 mnemonic key generation
- DKG for threshold signatures
- ECVRF (V2) leader selection
- Fast-sync protocol
- Message compression (Snappy)
- Genesis tooling

## What's a Stub ⚠️

| Feature              | Status                  | What's Needed                                                       |
| -------------------- | ----------------------- | ------------------------------------------------------------------- |
| ZK circuit hash      | ⚠️ Poseidon implemented | Round constants use BLAKE3 derivation; dual-hash transition started |
| RF fingerprinting    | ⚠️ Stub                 | SDR hardware (HackRF/USRP)                                          |
| Proof-of-useful-work | ⚠️ Stub                 | Production verification                                             |

> **Note:** 3-layer benchmark gate operational (IAI + multi-sample + criterion). Network-simulated multi-node benchmarks added.

## What Doesn't Exist Yet 🌑

- Mobile wallet
- JavaScript/Python client libraries
- Validator network (single-node operator for Phase 0)
- Sybil resistance / staking requirement
- Conviction voting and delegation
- Constant-time guarantee for Dilithium verify() (pending formal audit)

---

🔙 **Back**: [reference/](./) | 🔄 **Related**: [roadmap.md](./roadmap.md)
🚀 **Next**: [metrics-glossary.md](./metrics-glossary.md) | 📜 **Source of Truth**: [Restructuring Blueprint](./blueprint-reference.md)

# ADR-008: Cryptographic Dependency Audit

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-06-24

**Status**: Accepted
**Date**: 2026-03-04
**Updated**: 2026-05-16
**Decider**: Cipher (Agent 02 — ZK/Crypto Layer)
**Sprint**: Sprint 1

## Context

The Omnia Protocol relies on several cryptographic crates for core security guarantees: event signing, Merkle tree computation, state root hashing, ZK proof generation, and post-quantum commitments. As part of Sprint 1 hardening, we need to audit all cryptographic dependencies for version appropriateness and known vulnerabilities.

The spec mandates:

- ed25519-dalek should be 2.1+
- blake3 should be 1.5+

This audit covers two crates with cryptographic dependencies:

- **`omnia-adapters`** (`omnia-adapters/Cargo.toml`): ZK proof system (ark-\* crates, Poseidon hash, Powers of Tau)
- **`omnia-binding`** (`binding/Cargo.toml`): Post-quantum commitments and RF fingerprinting

## Audit Results

### 1. ed25519-dalek (binding crate)

| Field          | Value                              |
| -------------- | ---------------------------------- |
| **Specified**  | ≥ 2.1                              |
| **Cargo.toml** | `"2.1"` (with `rand_core` feature) |
| **Assessment** | ✅ **Safe**                        |

ed25519-dalek 2.x supersedes the 1.x line which had side-channel vulnerabilities (CVE-2020-12973 class). Version 2.x uses `curve25519-dalek` 4.x with constant-time operations by default. Used in `binding/src/quantum_commit.rs` for classical signature creation (`QuantumCommitment::sign_classical()`) and verification (`verify_ed25519()`). The `NodeKeypair` type from `omnia-substrate` wraps the `ed25519_dalek::SigningKey`.

**Recommendation**: Keep at current version. Monitor for any future advisories on the 2.x line.

### 2. blake3 (both crates)

| Field                         | Value       |
| ----------------------------- | ----------- |
| **Specified**                 | ≥ 1.5       |
| **omnia-adapters/Cargo.toml** | `"1.5"`     |
| **binding/Cargo.toml**        | `"1.5"`     |
| **Assessment**                | ✅ **Safe** |

BLAKE3 is a relatively new hash function with no known cryptographic vulnerabilities. Used extensively across the ZK and binding crates:

- **ZK crate**: Merkle tree construction (`merkle.rs::build_merkle_tree`, `compute_root_from_proof`), batch commitment hashing (`proof.rs::compute_batch_commitment`), Powers of Tau transcript hashing (`setup/contribution.rs`), Poseidon round constant generation (`poseidon.rs::generate_round_constants`), Ethereum adapter simulated tx hashes (`settlement/ethereum.rs`)
- **Binding crate**: Quantum commitment data hashing (`quantum_commit.rs::hash_data`), stub commitment creation (`new_stub`)

**Recommendation**: Keep at current version. The 1.5+ minimum ensures AVX-512 support and the optimized SIMD backend.

### 3. pqc_dilithium (binding crate)

| Field          | Value                       |
| -------------- | --------------------------- |
| **Specified**  | N/A (added for PQC support) |
| **Cargo.toml** | `"0.2"`                     |
| **Assessment** | ✅ **Safe**                 |

CRYSTALS-Dilithium is a NIST PQC standard for digital signatures. The `pqc_dilithium` crate provides the `Keypair::generate()`, `Keypair::sign()`, and `verify()` functions used in `binding/src/quantum_commit.rs` for hybrid and post-quantum signature modes:

```rust
pub fn sign_hybrid(data: &[u8], ed_keypair: &NodeKeypair, dilithium_keypair: &pqc_dilithium::Keypair) -> Result<Self, BindingError>
pub fn sign_post_quantum(data: &[u8], dilithium_keypair: &pqc_dilithium::Keypair) -> Result<Self, BindingError>
```

**Recommendation**: Monitor NIST for any updates to the Dilithium standard. Version 0.2 implements Dilithium as specified in FIPS 204. Watch for a 1.0 release that may include API changes.

### 4. ark-groth16 / ark-bn254 / ark-r1cs-std / ark-relations (ZK crate)

| Field          | Value                       |
| -------------- | --------------------------- |
| **Cargo.toml** | `"0.4"` (all ark-\* crates) |
| **Assessment** | ✅ **Safe**                 |

The `ark-*` ecosystem provides the Groth16 proof system on the BN254 curve. Used throughout the ZK crate:

- `ark-groth16` 0.4: `Groth16::setup()`, `Groth16::prove()`, `Groth16::verify()` in `omnia-adapters/src/prover.rs`
- `ark-bn254` 0.4: Bn254 pairing-friendly curve, `Fr` field element type used in circuits
- `ark-r1cs-std` 0.4: R1CS gadget library (`FpVar`, `Boolean`, `CondSelectGadget`, `EqGadget`) used in `omnia-adapters/src/circuit.rs` and `omnia-adapters/src/poseidon.rs`
- `ark-relations` 0.4: `ConstraintSynthesizer` trait, `ConstraintSystemRef`, `SynthesisError`
- `ark-serialize` 0.4: Canonical serialization for proofs and keys
- `ark-ec` 0.4: Elliptic curve operations for Powers of Tau
- `ark-ff` 0.4: Field arithmetic (`PrimeField`, `Field`, `Zero`, `UniformRand`)
- `ark-crypto-primitives` 0.4: `CircuitSpecificSetupSNARK` trait

The 0.4 line is the latest stable release. No known vulnerabilities.

**Recommendation**: Keep at 0.4. Watch for 0.5 release which may include BLS12-381 circuit support improvements.

### 5. rand / rand_chacha (ZK crate)

| Field                                       | Value                        |
| ------------------------------------------- | ---------------------------- |
| **omnia-adapters/Cargo.toml (rand)**        | `"0.8"`                      |
| **omnia-adapters/Cargo.toml (rand_chacha)** | `"0.3"`                      |
| **binding/Cargo.toml (rand)**               | `"0.8"`                      |
| **Assessment**                              | ⚠️ **Acceptable, with note** |

rand 0.8.x is used in the ZK crate for `ChaCha8Rng::from_entropy()` in trusted setup and proof generation (`prover.rs`). `ChaCha8Rng::from_seed()` is used for deterministic contributions in the trusted setup ceremony (`setup/contribution.rs`). The binding crate uses rand for `Fr::rand()` operations.

No known security vulnerabilities in rand 0.8.x.

**Recommendation**: Remain on 0.8.x for direct usage. Plan migration to 0.9.x in a future sprint to reduce dependency duplication with transitive dependencies.

### 6. ark-serialize (ZK crate)

| Field          | Value       |
| -------------- | ----------- |
| **Cargo.toml** | `"0.4"`     |
| **Assessment** | ✅ **Safe** |

Used for canonical serialization of Groth16 proofs and verifying keys in `omnia-adapters/src/prover.rs`:

- `serialize_proof()` uses `serialize_uncompressed()`
- `deserialize_proof()` uses `deserialize_uncompressed()`
- `serialize_verifying_key()` / `deserialize_verifying_key()` for VK persistence

Uncompressed serialization is used for simplicity and compatibility. Production deployments should consider compressed serialization to reduce proof size.

**Recommendation**: Consider switching to compressed serialization in a future sprint for reduced wire size.

### 7. serde / postcard (both crates)

| Field                     | Value                         |
| ------------------------- | ----------------------------- |
| **Cargo.toml (serde)**    | `"1.0"` with `derive` feature |
| **Cargo.toml (postcard)** | `"1"`                         |
| **Assessment**            | ✅ **Safe**                   |

Used for `ProofBundle` serialization (`proof_bundle.rs`), `MerkleProof` serialization (`merkle.rs`), `ProvenanceLog` serialization (`provenance.rs`), `PowersOfTau` serialization (`setup/powers_of_tau.rs`), `Contribution` and `ContributionProof` serialization (`setup/contribution.rs`), and `PqcKeyRotationRequest` serialization (`key_rotation.rs`). Also used for shard state serialization across all domain shards.

postcard is a `no_std`-compatible, deterministic, compact binary serialization format. It was chosen over bincode for the following reasons:

- **Deterministic encoding**: postcard always produces the same byte sequence for the same data, which is critical for consensus reproducibility.
- **`no_std` compatibility**: postcard works in embedded and WASM environments without `std`.
- **Active maintenance**: postcard is actively maintained and has no known vulnerabilities.

bincode 1.x is retained only for v0 backward compatibility (deserializing legacy data from before the migration). New code should use `postcard::to_allocvec()` and `postcard::from_bytes()` exclusively.

**Recommendation**: Complete removal of bincode dependency once all legacy data has been migrated or deprecated.

### 8. thiserror (both crates)

| Field          | Value       |
| -------------- | ----------- |
| **Cargo.toml** | `"2.0"`     |
| **Assessment** | ✅ **Safe** |

Used for error type derivation: `ProverError`, `ProofBundleError`, `SettlementError`, `RollupError`, `SetupError`, `BindingError`, `ProvenanceTrackerError`. thiserror 2.0 is the current release.

**Recommendation**: Keep at current version.

## Summary Table

| Crate                 | Location | Cargo.toml | Spec Met | Status                              |
| --------------------- | -------- | ---------- | -------- | ----------------------------------- |
| ed25519-dalek         | binding  | 2.1        | ✅ ≥2.1  | ✅ Safe                             |
| blake3                | both     | 1.5        | ✅ ≥1.5  | ✅ Safe                             |
| pqc_dilithium         | binding  | 0.2        | N/A      | ✅ Safe                             |
| ark-groth16           | zk       | 0.4        | N/A      | ✅ Safe                             |
| ark-bn254             | zk       | 0.4        | N/A      | ✅ Safe                             |
| ark-r1cs-std          | zk       | 0.4        | N/A      | ✅ Safe                             |
| ark-relations         | zk       | 0.4        | N/A      | ✅ Safe                             |
| ark-serialize         | zk       | 0.4        | N/A      | ✅ Safe                             |
| ark-ec                | zk       | 0.4        | N/A      | ✅ Safe                             |
| ark-ff                | zk       | 0.4        | N/A      | ✅ Safe                             |
| ark-crypto-primitives | zk       | 0.4        | N/A      | ✅ Safe                             |
| rand                  | both     | 0.8        | N/A      | ⚠️ Acceptable                       |
| rand_chacha           | zk       | 0.3        | N/A      | ✅ Safe                             |
| serde                 | both     | 1.0        | N/A      | ✅ Safe                             |
| postcard              | both     | 1          | N/A      | ✅ Safe                             |
| bincode               | both     | 1          | N/A      | ⚠️ Legacy only (v0 backward compat) |
| thiserror             | both     | 2.0        | N/A      | ✅ Safe                             |

## Action Items

1. **No immediate changes required** — all spec requirements are met and no known vulnerabilities exist in any cryptographic dependency.
2. **Future sprint**: Migrate rand from 0.8.x to 0.9.x to eliminate dependency duplication.
3. **Future sprint**: Remove bincode 1.x dependency entirely once all legacy v0 data has been migrated or deprecated.
4. **Future sprint**: Consider switching from uncompressed to compressed ark-serialize for reduced proof wire size.
5. **Ongoing**: Subscribe to RustSec advisories for all crypto crates; integrate `cargo audit` into CI when available.
6. **Ongoing**: Monitor NIST for updates to CRYSTALS-Dilithium standard (FIPS 204) and watch for `pqc_dilithium` crate updates.

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

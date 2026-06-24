# Cryptographic Migration Playbook

> 🎯 Audience: Developers
> 🔗 Context: Migration path for each cryptographic primitive if it is compromised
> 📅 Last Updated: 2026-06-24

## Overview

This document describes the migration path for each cryptographic primitive used by the Omnia Protocol if it is compromised. Each migration follows a 4-phase process:

1. **Disclosure**: Vulnerability announced via security advisory
2. **Deprecation**: Old scheme marked deprecated, new scheme available
3. **Migration**: Nodes upgrade, both schemes accepted during transition
4. **Sunset**: Old scheme no longer accepted, minimum version enforced

The protocol's cryptographic dependencies span two crates:

- **`omnia-adapters`** (`omnia-adapters/Cargo.toml`): Groth16 proofs on BN254, Poseidon hash, Powers of Tau ceremony, Merkle tree verification
- **`omnia-binding`** (`binding/Cargo.toml`): Ed25519 signatures via `ed25519-dalek` 2.1, CRYSTALS-Dilithium via `pqc_dilithium` 0.2, BLAKE3 1.5, ML-KEM-768 via `ml-kem` 0.2, hybrid commitment verification

## Cryptographic Primitives in Use

| Primitive           | Crate            | Implementation                              | File Reference                                                  |
| ------------------- | ---------------- | ------------------------------------------- | --------------------------------------------------------------- |
| Groth16 SNARK       | `omnia-adapters` | `ark-groth16` 0.4 on Bn254                  | `omnia-adapters/src/prover.rs`                                  |
| Poseidon hash       | `omnia-adapters` | Custom (Cauchy MDS + BLAKE3 RC) + Reference | `omnia-adapters/src/poseidon.rs`                                |
| BLAKE3              | both             | `blake3` 1.5                                | `omnia-adapters/src/merkle.rs`, `binding/src/quantum_commit.rs` |
| Ed25519             | `omnia-binding`  | `ed25519-dalek` 2.1                         | `binding/src/quantum_commit.rs`                                 |
| CRYSTALS-Dilithium  | `omnia-binding`  | `pqc_dilithium` 0.2                         | `binding/src/quantum_commit.rs`                                 |
| ML-KEM-768          | `omnia-binding`  | `ml-kem` 0.2 (FIPS-203)                     | `binding/src/quantum_commit.rs`                                 |
| Powers of Tau (PoK) | `omnia-adapters` | Fiat-Shamir on BN254 G1                     | `omnia-adapters/src/setup/contribution.rs`                      |

## Migration Paths

### If Ed25519 is Compromised

The binding crate already implements a phased commitment model via the `CommitmentPhase` enum:

| Step | Action                                                | Timeline  |
| ---- | ----------------------------------------------------- | --------- |
| 1    | Announce advisory, set deprecation date               | Day 0     |
| 2    | Set `CommitmentPhase::Hybrid` as default              | Day 1     |
| 3    | Nodes generate Dilithium keys, submit PoP             | Week 1    |
| 4    | Both Ed25519 and Dilithium accepted via `Hybrid`      | Weeks 1-6 |
| 5    | Require `Hybrid` or `PostQuantum` for new commitments | Week 6    |
| 6    | Sunset `ClassicalOnly`                                | Week 8    |

### If BLAKE3 Has a Collision

BLAKE3 is used for off-circuit Merkle tree construction, batch commitment hashing, data hashing, and transcript hashing. On-circuit Poseidon hash is unaffected.

| Step | Action                              | Timeline  |
| ---- | ----------------------------------- | --------- |
| 1    | Announce advisory                   | Day 0     |
| 2    | Add SHA3-256 fallback hash function | Week 1    |
| 3    | New events use SHA3-256 hashes      | Week 2    |
| 4    | Both hash schemes accepted          | Weeks 2-4 |
| 5    | Re-hash all stored events           | Weeks 4-8 |
| 6    | Sunset BLAKE3-only mode             | Week 8    |

### If BN254 Curve is Compromised

The most complex migration — requires new circuit, new Poseidon parameters, new trusted setup, and re-generation of all proofs.

| Step | Action                                          | Timeline   |
| ---- | ----------------------------------------------- | ---------- |
| 1    | Announce advisory                               | Day 0      |
| 2    | Parameterize circuit for BLS12-381              | Week 2     |
| 3    | Derive new Poseidon parameters for BLS12-381 Fr | Week 3     |
| 4    | New trusted setup ceremony on BLS12-381         | Week 4     |
| 5    | Migrate all active proofs                       | Weeks 4-12 |
| 6    | Sunset BN254 Groth16 proofs                     | Week 12    |

### If CRYSTALS-Dilithium is Compromised

| Step | Action                                                  | Timeline |
| ---- | ------------------------------------------------------- | -------- |
| 1    | Announce advisory                                       | Day 0    |
| 2    | Revert to `ClassicalOnly`                               | Day 1    |
| 3    | Deploy replacement PQC (SPHINCS+ or next NIST standard) | Week 2-4 |
| 4    | New hybrid mode with replacement                        | Week 4-8 |
| 5    | Sunset Dilithium                                        | Week 8   |

### If a Quantum Computer Breaks Classical Crypto (Q-Day)

| Step | Action                                                         | Timeline |
| ---- | -------------------------------------------------------------- | -------- |
| 1    | Emergency advisory                                             | Day 0    |
| 2    | Require `Hybrid` or `PostQuantum` immediately                  | Day 1    |
| 3    | All `ClassicalOnly` commitments invalid                        | Day 1    |
| 4    | Emergency key generation for validators without Dilithium keys | Day 1-3  |
| 5    | Resume with `PostQuantum` mode                                 | Day 3    |

**Mitigation:** Deploy `Hybrid` as default BEFORE Q-Day.

### If Poseidon Hash is Compromised

| Step | Action                                                  | Timeline  |
| ---- | ------------------------------------------------------- | --------- |
| 1    | Announce advisory                                       | Day 0     |
| 2    | Select replacement SNARK-friendly hash (Rescue, Vision) | Week 1    |
| 3    | Implement new hash gadget                               | Week 2-3  |
| 4    | Regenerate trusted setup                                | Week 4    |
| 5    | Migrate active proofs                                   | Weeks 4-8 |
| 6    | Sunset Poseidon-based proofs                            | Week 8    |

### VRF V1→V2 Migration

The VRF construction has two versions:

- **V1 (default)** — Legacy deterministic-hash VRF (Ed25519 signature + BLAKE3 derivation). This is the current default for backwards compatibility.
- **V2 (implemented per ADR-012 v2.0.0)** — ECVRF with Fiat-Shamir + Ed25519 per RFC 9381. Implemented in `omnia-crypto/src/vrf.rs` (`ecvrf_prove()` / `ecvrf_verify()` / `select_leader_v2()`). Provides uniqueness, unpredictability, and zero-knowledge properties that V1 does not formally satisfy.

V2 is implemented and available behind `VrfVersion::V2`, but V1 remains the default pending a coordinated network-wide migration. Migration steps:

1. **Disclosure** — Announce V1 deprecation date and V2 cutover date
2. **Deprecation** — Both V1 and V2 accepted; new validators encouraged to use V2
3. **Migration** — Network reaches supermajority on V2 leader selection
4. **Sunset** — V1 leader selection disabled at protocol version boundary

---

🔙 **Back**: [reference/](./) | 🔄 **Related**: [dependency-policy.md](./dependency-policy.md)
🚀 **Next**: [../use-cases/](../use-cases/) | 📜 **Source of Truth**: [Restructuring Blueprint](./blueprint-reference.md)

# Cryptographic Migration Playbook

**Version**: 4.0.0
**Last Updated**: 2026-05-16

## Overview

This document describes the migration path for each cryptographic primitive
used by the Omnia Protocol if it is compromised. Each migration follows a
4-phase process:

1. **Disclosure**: Vulnerability announced via security advisory
2. **Deprecation**: Old scheme marked deprecated, new scheme available
3. **Migration**: Nodes upgrade, both schemes accepted during transition
4. **Sunset**: Old scheme no longer accepted, minimum version enforced

The protocol's cryptographic dependencies span two crates:

- **`omnia-zk`** (`zk/Cargo.toml`): Groth16 proofs on BN254, Poseidon hash,
  Powers of Tau ceremony, Merkle tree verification
- **`omnia-binding`** (`binding/Cargo.toml`): Ed25519 signatures via
  `ed25519-dalek` 2.1, CRYSTALS-Dilithium via `pqc_dilithium` 0.2,
  BLAKE3 1.5, hybrid commitment verification

## Cryptographic Primitives in Use

| Primitive | Crate | Implementation | File Reference |
|-----------|-------|---------------|----------------|
| Groth16 SNARK | `omnia-zk` | `ark-groth16` 0.4 on Bn254 | `zk/src/prover.rs` |
| Poseidon hash | `omnia-zk` | Custom (Cauchy MDS + BLAKE3 RC) | `zk/src/poseidon.rs` |
| BLAKE3 | both | `blake3` 1.5 | `zk/src/merkle.rs`, `zk/src/proof.rs`, `binding/src/quantum_commit.rs` |
| Ed25519 | `omnia-binding` | `ed25519-dalek` 2.1 | `binding/src/quantum_commit.rs` |
| CRYSTALS-Dilithium | `omnia-binding` | `pqc_dilithium` 0.2 | `binding/src/quantum_commit.rs` |
| Powers of Tau (PoK) | `omnia-zk` | Fiat-Shamir on BN254 G1 | `zk/src/setup/contribution.rs` |

## Migration Paths

### If Ed25519 is Compromised

The binding crate already implements a phased commitment model via the
`CommitmentPhase` enum (`binding/src/quantum_commit.rs`):

```rust
pub enum CommitmentPhase {
    ClassicalOnly = 0,  // Only Ed25519 verified
    Hybrid = 1,         // Both Ed25519 and Dilithium verified
    PostQuantum = 2,    // Only Dilithium verified
}
```

| Step | Action | Timeline |
|------|--------|----------|
| 1 | Announce advisory, set deprecation date | Day 0 |
| 2 | Set `CommitmentPhase::Hybrid` as default in `PqcKeyRotationManager` | Day 1 |
| 3 | Nodes generate Dilithium keys via `pqc_dilithium::Keypair::generate()`, submit PoP | Week 1 |
| 4 | Both Ed25519 and Dilithium accepted via `CommitmentPhase::Hybrid` | Weeks 1-6 |
| 5 | Require `CommitmentPhase::Hybrid` or `PostQuantum` for new commitments | Week 6 |
| 6 | Sunset `ClassicalOnly` — all `QuantumCommitment.verify()` calls use `Hybrid` or `PostQuantum` | Week 8 |

The `PqcKeyRotationManager` (`binding/src/key_rotation.rs`) handles phase
transitions automatically. It enforces that phases only advance forward
(ClassicalOnly → Hybrid → PostQuantum) and never downgrade. Key rotation
requests require an authorization signature from the old key.

### If BLAKE3 Has a Collision

BLAKE3 is used for:
- Off-circuit Merkle tree construction (`zk/src/merkle.rs::compute_root_from_proof`)
- Batch commitment hashing (`zk/src/proof.rs::compute_batch_commitment`)
- Data hashing in quantum commitments (`binding/src/quantum_commit.rs::hash_data`)
- Powers of Tau transcript hashing (`zk/src/setup/contribution.rs`)

| Step | Action | Timeline |
|------|--------|----------|
| 1 | Announce advisory | Day 0 |
| 2 | Add SHA3-256 fallback hash function | Week 1 |
| 3 | New events use SHA3-256 hashes | Week 2 |
| 4 | Both hash schemes accepted for verification | Weeks 2-4 |
| 5 | Re-hash all stored events with SHA3-256 | Weeks 4-8 |
| 6 | Sunset BLAKE3-only mode | Week 8 |

**Important**: On-circuit Poseidon hash (`zk/src/poseidon.rs`) is unaffected
by a BLAKE3 collision because Poseidon operates on field elements, not
byte arrays. However, the off-circuit Merkle tree construction (`build_merkle_tree`)
uses BLAKE3, so those proofs would need regeneration with SHA3-256 leaf hashing.
The `poseidon_hash_to_fr()` function in `zk/src/merkle.rs` provides the
on-circuit-compatible alternative and would not be affected.

### If BN254 Curve is Compromised

This is the most complex migration because the entire ZK proof system,
Poseidon hash parameters, and Merkle tree structure are curve-specific.

| Step | Action | Timeline |
|------|--------|----------|
| 1 | Announce advisory | Day 0 |
| 2 | Parameterize `ExpandedRollupCircuit` for BLS12-381 scalar field | Week 2 |
| 3 | Derive new Poseidon parameters (t=3, R_F, R_P) for BLS12-381 Fr | Week 3 |
| 4 | New trusted setup ceremony (`SetupCeremony`) on BLS12-381 | Week 4 |
| 5 | Migrate all active proofs to new curve | Weeks 4-12 |
| 6 | Sunset BN254 Groth16 proofs | Week 12 |

**NOTE**: BN254 migration requires:
- New `ExpandedRollupCircuit` parameterized for BLS12-381 scalar field
- New Poseidon parameters for BLS12-381 scalar field (the Cauchy MDS matrix
  and BLAKE3-derived round constants in `zk/src/poseidon.rs` must be
  regenerated for the new field modulus)
- New `PowersOfTau` accumulator for BLS12-381 G1/G2 (`zk/src/setup/powers_of_tau.rs`)
- New `Contribution` and `ContributionProof` types for BLS12-381 (`zk/src/setup/contribution.rs`)
- Re-generation of all historical proofs (or accepting old proofs with a
  validity window)
- Update `OmniaRollup.sol` verifier contract for BLS12-381 pairing checks

### If CRYSTALS-Dilithium is Compromised

Dilithium is used in the binding crate for post-quantum signatures. If
compromised, the migration would fall back to classical-only mode while
a replacement PQC scheme is deployed.

| Step | Action | Timeline |
|------|--------|----------|
| 1 | Announce advisory | Day 0 |
| 2 | Revert `CommitmentPhase` to `ClassicalOnly` in `PqcKeyRotationManager` | Day 1 |
| 3 | Deploy replacement PQC scheme (SPHINCS+ or next NIST standard) | Week 2-4 |
| 4 | Update `QuantumCommitment` struct with new PQC fields | Week 4 |
| 5 | New hybrid mode with replacement scheme | Week 4-8 |
| 6 | Sunset Dilithium signatures | Week 8 |

### If a Quantum Computer Breaks Classical Crypto (Q-Day)

| Step | Action | Timeline |
|------|--------|----------|
| 1 | Emergency advisory | Day 0 |
| 2 | Require `CommitmentPhase::Hybrid` or `PostQuantum` immediately | Day 1 |
| 3 | All `ClassicalOnly` commitments considered invalid | Day 1 |
| 4 | Emergency key generation for validators without Dilithium keys | Day 1-3 |
| 5 | Network resumes with PQC-only mode (`CommitmentPhase::PostQuantum`) | Day 3 |

**Mitigation**: Deploy `CommitmentPhase::Hybrid` as default BEFORE Q-Day.
The `PqcKeyRotationManager` already supports phase transitions and can be
configured to start in `Hybrid` mode from genesis.

### If the Poseidon Hash Function is Compromised

Poseidon is used for on-circuit Merkle path verification and state
transition constraints in `ExpandedRollupCircuit` (`zk/src/circuit.rs`).

| Step | Action | Timeline |
|------|--------|----------|
| 1 | Announce advisory | Day 0 |
| 2 | Select replacement SNARK-friendly hash (e.g., Rescue, Vision) | Week 1 |
| 3 | Implement new hash gadget in `zk/src/poseidon.rs` (or new module) | Week 2-3 |
| 4 | Regenerate trusted setup with new hash constraints | Week 4 |
| 5 | Migrate all active proofs | Weeks 4-8 |
| 6 | Sunset Poseidon-based proofs | Week 8 |

**NOTE**: The current Poseidon implementation uses a Cauchy-constructed MDS
matrix and BLAKE3-derived round constants, which differ from the Filecoin/
Neptune reference constants. This is documented as a known breaking change
in `zk/src/poseidon.rs`. A migration to reference constants would also
invalidate existing proofs.

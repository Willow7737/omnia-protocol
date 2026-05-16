# Cryptographic Migration Playbook

## Overview
This document describes the migration path for each cryptographic primitive
if it is compromised. Each migration follows a 4-phase process:

1. **Disclosure**: Vulnerability announced via security advisory
2. **Deprecation**: Old scheme marked deprecated, new scheme available
3. **Migration**: Nodes upgrade, both schemes accepted during transition
4. **Sunset**: Old scheme no longer accepted, minimum version enforced

## Migration Paths

### If Ed25519 is Compromised
| Step | Action | Timeline |
|------|--------|----------|
| 1 | Announce advisory, set deprecation date | Day 0 |
| 2 | Deploy `SignatureScheme::HybridV1` support | Week 1 |
| 3 | Nodes generate Dilithium keys, submit PoP | Week 2 |
| 4 | Both Ed25519 and Hybrid accepted | Weeks 2-6 |
| 5 | Require `HybridV1` minimum for new events | Week 6 |
| 6 | Sunset Ed25519V1 - reject all V1 signatures | Week 8 |

### If BLAKE3 Has a Collision
| Step | Action | Timeline |
|------|--------|----------|
| 1 | Announce advisory | Day 0 |
| 2 | Deploy `HashScheme::Sha3V2` | Week 1 |
| 3 | New events use SHA3-256 hashes | Week 2 |
| 4 | Both hash schemes accepted for verification | Weeks 2-4 |
| 5 | Re-hash all stored events with SHA3-256 | Weeks 4-8 |
| 6 | Sunset Blake3V1 | Week 8 |

### If BN254 Curve is Compromised
| Step | Action | Timeline |
|------|--------|----------|
| 1 | Announce advisory | Day 0 |
| 2 | Deploy `ZkScheme::Groth16Bls12381V2` circuits | Week 2 |
| 3 | New trusted setup ceremony on BLS12-381 | Week 4 |
| 4 | Migrate all active proofs to new curve | Weeks 4-12 |
| 5 | Sunset Groth16Bn254V1 | Week 12 |

**NOTE**: BN254 migration is the most complex - all ZK proofs, Merkle trees,
and Poseidon hashes must be re-built on the new curve. This requires:
- New `ExpandedRollupCircuit` parameterized for BLS12-381
- New Poseidon parameters for BLS12-381 scalar field
- New Powers of Tau ceremony
- Re-generation of all historical proofs (or accepting old proofs with
  a validity window)

### If a Quantum Computer Breaks Classical Crypto (Q-Day)
| Step | Action | Timeline |
|------|--------|----------|
| 1 | Emergency advisory | Day 0 |
| 2 | Require `SignatureScheme::HybridV1` or `DilithiumV2` immediately | Day 1 |
| 3 | All Ed25519-only signatures considered invalid | Day 1 |
| 4 | Emergency key generation for validators without Dilithium keys | Day 1-3 |
| 5 | Network resumes with PQC-only mode | Day 3 |

**Mitigation**: Deploy `HybridV1` as default BEFORE Q-Day.

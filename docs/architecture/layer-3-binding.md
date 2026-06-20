# Layer 3: The Binding Layer

> 🎯 Audience: Developers
> 🔗 Context: Layer 3 anchors the digital system to physical reality without requiring trusted intermediaries
> 📅 Last Updated: 2026-05-20

## Overview

The Binding Layer anchors the digital system to physical reality without requiring trusted intermediaries (oracles). It provides provenance tracking, physical anchoring, and post-quantum cryptographic commitments.

## Components

### ProvenanceLog — ✅ Implemented

The provenance log is fully implemented as an append-only CRDT. It provides:

- Create, transfer, verify, destroy lifecycle for tracked items
- Complete ownership history (cryptographic birth certificate)
- No intermediaries needed for verification
- `ProvenanceTracker` with full lifecycle management

Located in: `binding/src/provenance.rs`

### PhysicalAnchor — ✅ Implemented (with stubs)

Combines RF fingerprinting stub, quantum commitments, and provenance into a unified verification interface.

- RF fingerprinting is a stub (Hamming distance comparison); real implementation requires SDR hardware (HackRF/USRP)
- Located in: `binding/src/anchor.rs`, `binding/src/rf_fingerprint.rs`, `binding/src/physical_shard.rs`

### Quantum Commitments — ✅ Implemented

The quantum commitment system uses a hybrid Ed25519 + CRYSTALS-Dilithium approach with phase transitions:

```rust
pub enum CommitmentPhase {
    ClassicalOnly = 0,  // Only Ed25519 verified
    Hybrid = 1,         // Both Ed25519 and Dilithium verified
    PostQuantum = 2,    // Only Dilithium verified
}
```

- Both `verify_ed25519()` and `verify_dilithium()` perform real cryptographic verification
- ML-KEM-768 (FIPS-203 algorithm; Rust implementation not NIST-certified) key encapsulation for post-quantum key exchange
- Constant-time comparisons via `subtle::ConstantTimeEq`
- X25519 ECDH + ML-KEM-768 hybrid mode for defense-in-depth

Located in: `binding/src/quantum_commit.rs`

### PQC Key Rotation — ✅ Implemented

`PqcKeyRotationManager` handles phase transitions automatically:

- Enforces that phases only advance forward (ClassicalOnly → Hybrid → PostQuantum)
- Key rotation requires authorization signature from old key
- Rotation state persisted as JSON, recoverable after process restart
- `KeyStoreBridge` integrates with `EncryptedKeyStore` for persistent rotation state

Located in: `binding/src/key_rotation.rs`, `binding/src/keystore_bridge.rs`

### Biometric Binding — ✅ Implemented

Privacy-preserving biometric anchors using `BLAKE3(salt || template)`. The template is never stored in cleartext.

Located in: `shards/src/identity/biometric.rs`

## What's a Stub ⚠️

| Feature               | Status             | What's Needed                                                                                          |
| --------------------- | ------------------ | ------------------------------------------------------------------------------------------------------ |
| RF fingerprinting     | ⚠️ Stub            | SDR hardware (HackRF/USRP) for real RF-DNA feature extraction                                          |
| Physical time anchors | 🌑 Not Implemented | Previously described as "Gravitational Timestamps" — protocol relies on logical time via vector clocks |
| Satellite mesh        | 🌑 Not Implemented | GPS + Galileo + Starlink cross-validation for location verification                                    |

## Cryptographic Migration

For the migration playbook if any cryptographic primitive is compromised, see [crypto-migration.md](../reference/crypto-migration.md).

---

🔙 **Back**: [architecture/](./) | 🔄 **Related**: [layer-4-identity.md](./layer-4-identity.md)
🚀 **Next**: [layer-4-identity.md](./layer-4-identity.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

# ADR-020: Kyber KEM / ML-KEM Integration
> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-05-20

## Status

Accepted

## Date

2026-05-19

## Version

1.0.0

## Decision

Migrate from the `pqc_kyber` crate (0.7.x) to the `ml-kem` crate for post-quantum key encapsulation. ML-KEM-768 is the FIPS-203 standardized variant of Kyber768 with identical wire format (same key and ciphertext sizes), eliminating the KyberSlash vulnerability (RUSTSEC-2023-0079) while maintaining backward compatibility.

## Context

The `pqc_kyber` crate version 0.7.x had a critical timing side-channel vulnerability known as KyberSlash (RUSTSEC-2023-0079). This vulnerability allows an attacker to recover the secret key through careful timing measurements of the decapsulation operation. While the `pqc_kyber` 0.8+ series addressed this, the crate is not FIPS-compliant and its long-term maintenance is uncertain.

ML-KEM (Module-Lattice-Based Key-Encapsulation Mechanism) is the FIPS-203 standardized version of Kyber, published by NIST in 2024. The key properties are:

1. **Wire-compatible**: ML-KEM-768 has the same public key size (1184 bytes), secret key size (2400 bytes), and ciphertext size (1088 bytes) as Kyber768.
2. **FIPS-203 algorithm (Rust implementation not NIST-certified)**: Officially standardized by NIST, providing regulatory compliance for production deployments.
3. **No KyberSlash**: The `ml-kem` crate implementation uses constant-time operations throughout.
4. **Actively maintained**: The `ml-kem` crate follows the RustCrypto project's maintenance standards.

## Alternatives Considered

### pqc_kyber 0.8+
Upgrade to `pqc_kyber` 0.8+ which patches KyberSlash. This is the simplest migration path but has drawbacks: not FIPS-203 algorithm (Rust implementation not NIST-certified) (implements the pre-standard Kyber), long-term maintenance uncertain, and doesn't align with NIST standardization direction.

### Custom Implementation
Implement ML-KEM from scratch following FIPS-203. Maximum control and auditability but significant implementation effort, risk of subtle bugs in constant-time requirements, and no community review. Cryptographic implementations should be left to specialized, well-audited crates.

## Consequences

### Positive
- Eliminates KyberSlash vulnerability (RUSTSEC-2023-0079)
- FIPS-203 algorithm (Rust implementation not NIST-certified) for regulatory environments
- Wire-compatible with Kyber768 — no migration needed for existing encrypted data
- Same key sizes: public key 1184 bytes, ciphertext 1088 bytes
- `ml-kem` crate is part of the RustCrypto ecosystem with strong maintenance guarantees
- Constant-time comparisons via `subtle::ConstantTimeEq` for all secret operations
- `generate_kyber_keypair()`, `kyber_encapsulate()`, `kyber_decapsulate()` API preserved

### Negative
- Dependency change requires re-testing all KEM integration points
- `ml-kem` crate has different API surface than `pqc_kyber` (adapted in `binding/src/quantum_commit.rs`)
- Some `omnia-binding` tests may need API adjustments for `ml-kem`'s type system
- Migration removes ability to interop with pre-standard Kyber implementations

### Trade-offs
- Chose `ml-kem` over `pqc_kyber` 0.8+ for FIPS-203 algorithm (Rust implementation not NIST-certified) and long-term standardization alignment
- Wire compatibility means no data migration — existing encrypted shares remain valid
- API naming (`kyber_*` functions) preserved for consistency, even though the underlying implementation is ML-KEM

---
🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

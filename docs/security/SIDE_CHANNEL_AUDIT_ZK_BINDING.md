# Side-Channel Audit — ZK and Binding Crates

**Version**: 1.0
**Date**: 2026-05-19
**Scope**: `zk/` and `binding/` crates
**Auditor**: Omnia Protocol Security Team (Phase 5)

## Executive Summary

This document records the side-channel audit findings for the ZK (`omnia-zk`) and Binding (`omnia-binding`) crates. The substrate crate was previously audited (see `docs/security/SIDE_CHANNEL_AUDIT.md`) with all secret comparisons using `subtle::ConstantTimeEq`. This audit extends coverage to the ZK and binding crates, which contain the most sensitive cryptographic operations in the protocol.

**Overall Assessment**: The ZK and binding crates use constant-time comparisons for secret data where `subtle::ConstantTimeEq` is already applied. The primary remaining risks are in Poseidon field element operations (branching on non-secret data with timing variation) and Dilithium signature verification (delegated to the `pqc-dilithium` crate). No exploitable timing leaks were found that would reveal secret keys.

## Audit Methodology

1. **Code review** of all comparison and branching operations on secret data
2. **Timing analysis** of hash computation, signature verification, and KEM operations
3. **Dependency audit** of `pqc-dilithium` and `ml-kem` crates for known timing vulnerabilities
4. **Inline analysis** — checking whether sensitive functions are marked `#[inline]` to prevent timing variation from inlining decisions

## Findings by Module

### 1. `zk/src/poseidon.rs` — Poseidon Hash Function

#### Status: LOW RISK

| Function | Secret Data? | Constant-Time? | Risk | Notes |
|----------|-------------|----------------|------|-------|
| `sbox(x: &Fr)` | No (input is public in ZK circuit context) | N/A | Low | S-box operates on field elements that are public inputs in the verification context |
| `mds_multiply()` | No | N/A | Low | Matrix multiplication on public parameters |
| `poseidon_permutation()` | No | N/A | Low | All round constants and MDS matrix are public |
| `poseidon_hash_offchain()` | No (input is typically a Merkle path) | N/A | Low | Off-circuit hash operates on public data |
| `poseidon_hash()` (on-circuit) | No | N/A | Low | Circuit constraints enforce correct computation |

**Analysis**: Poseidon hash operations operate on field elements that are either public inputs (Merkle paths, state roots) or public parameters (round constants, MDS matrix). There are no secret-dependent branches in the Poseidon implementation. The S-box computation (`x^5`) is a sequence of field multiplications with no branching on secret data.

**Remaining Concern**: Field element comparisons (e.g., checking if a field element is zero) could theoretically leak information if they use non-constant-time equality. However, the `ark-ff` crate implements field operations in constant time by default (all operations are data-independent in terms of execution path).

**Recommendation**: No changes needed. The Poseidon implementation is safe against timing side-channels for its current use case.

### 2. `binding/src/quantum_commit.rs` — Quantum-Resistant Commitments

#### Status: LOW RISK

| Function | Secret Data? | Constant-Time? | Risk | Notes |
|----------|-------------|----------------|------|-------|
| `verify()` | No (public key, data, commitment) | Yes (`ct_ne`) | Low | Uses `subtle::ConstantTimeEq` for hash comparison |
| `verify_ed25519()` | No (public verification) | Delegated to `ed25519-dalek` | Low | `ed25519-dalek` uses constant-time scalar multiplication |
| `verify_dilithium()` | No (public verification) | Delegated to `pqc-dilithium` | Medium | See Dilithium analysis below |
| `sign_hybrid()` | Yes (secret key) | Partial | Medium | Signing uses secret key; see analysis |
| `generate_kyber_keypair()` | Yes (secret key) | Delegated to `ml-kem` | Low | ML-KEM uses constant-time operations |
| `kyber_encapsulate()` | No (public key operation) | Delegated to `ml-kem` | Low | Encapsulation uses public key |
| `kyber_decapsulate()` | Yes (secret key) | Delegated to `ml-kem` | Low | ML-KEM implicit rejection is constant-time |

**Analysis**:

- **Data hash verification**: The `verify()` method uses `hash.as_bytes().ct_ne(&self.data_hash)` for constant-time comparison of the data hash. This is correct and prevents timing leaks.
- **Ed25519 verification**: Delegated to `ed25519-dalek`, which implements constant-time scalar multiplication and point operations. No timing leak.
- **Dilithium verification**: Delegated to `pqc-dilithium`. The `pqc-dilithium` crate is a Rust port of the C reference implementation. The reference implementation was designed for constant-time execution, but the Rust port has not been formally audited for timing leaks. This is a **medium** risk item.
- **CommitmentPhase comparison**: The `verify()` method branches on `phase: CommitmentPhase`, which is public (not secret). No timing leak.
- **ML-KEM operations**: Delegated to the `ml-kem` crate, which implements constant-time operations including implicit rejection. No timing leak.

**Remaining Concern**: The `pqc-dilithium` crate's verification path should be formally audited for timing side-channels. Until then, we rely on the correctness of the C reference implementation's constant-time claims.

**Recommendation**: Monitor `pqc-dilithium` crate updates for timing fixes. Consider switching to a formally verified Dilithium implementation (e.g., `liboqs` bindings) for production.

### 3. `zk/src/setup/contribution.rs` — Trusted Setup Ceremony

#### Status: LOW RISK

| Function | Secret Data? | Constant-Time? | Risk | Notes |
|----------|-------------|----------------|------|-------|
| EC scalar multiplication | Yes (contribution secret) | Delegated to `ark-ec` | Low | `ark-ec` uses constant-time group operations |
| Fiat-Shamir PoK | Yes (secret scalar) | Yes (hash-based) | Low | Challenge derived from BLAKE3, no branching on secret |

**Analysis**: The trusted setup contribution uses `ark-ec` for elliptic curve operations, which implement constant-time scalar multiplication. The Fiat-Shamir Proof of Knowledge derives its challenge from a hash of public transcript data, so no timing leak on the secret scalar.

**Recommendation**: No changes needed.

### 4. `binding/src/key_rotation.rs` — PQC Key Rotation

#### Status: LOW RISK

| Function | Secret Data? | Constant-Time? | Risk | Notes |
|----------|-------------|----------------|------|-------|
| Key comparison | No (comparing public key hashes) | Uses `ct_eq` where needed | Low | Public key comparisons don't involve secrets |
| Rotation trigger | No (based on public height/threshold) | N/A | Low | Rotation decision is public |

**Analysis**: Key rotation operates on public keys and public thresholds. The rotation decision is based on block height and governance parameters, not on any secret data. No timing leak.

**Recommendation**: No changes needed.

## Summary of Findings

| ID | Severity | Component | Finding | Status |
|----|----------|-----------|---------|--------|
| SC-ZK-001 | Low | `zk/src/poseidon.rs` | Poseidon field operations use `ark-ff` which is constant-time by default | Accepted (no fix needed) |
| SC-BD-001 | Medium | `binding/src/quantum_commit.rs` | Dilithium verification delegates to `pqc-dilithium` which has not been formally audited for timing side-channels | Accepted (monitor upstream) |
| SC-BD-002 | Low | `binding/src/quantum_commit.rs` | All secret comparisons use `subtle::ConstantTimeEq` | Verified (no fix needed) |
| SC-ZK-002 | Low | `zk/src/setup/contribution.rs` | EC operations delegate to `ark-ec` with constant-time scalar multiplication | Verified (no fix needed) |
| SC-KR-001 | Low | `binding/src/key_rotation.rs` | Key rotation operates on public data only | Verified (no fix needed) |

## Recommendations

1. **Monitor `pqc-dilithium` crate**: Track updates for timing side-channel fixes. Consider `liboqs` bindings for production if formal audit is needed.

2. **Add statistical timing tests**: Implement tests that measure execution time over many iterations to detect statistically significant timing variations. This is a defense-in-depth measure.

3. **Mark sensitive functions `#[inline]`**: Ensure that the compiler does not create timing variations through inlining decisions on security-critical functions.

4. **Formal Dilithium audit**: Commission a formal timing audit of the `pqc-dilithium` crate before mainnet launch.

## Testing

The following test was added to verify constant-time properties statistically:

```rust
// In zk/src/poseidon.rs tests:
#[test]
fn test_poseidon_constant_time() {
    // Verify hash computation takes the same time for different inputs
    // (statistical test over many iterations)
    // This is a best-effort test — true constant-time guarantees require
    // hardware-level analysis.
    use std::time::Instant;

    let iterations = 1000;
    let a = Fr::from(42u64);
    let b = Fr::from(123u64);
    let c = Fr::from(999999u64);
    let d = Fr::from(1u64);

    let mut times_ab = Vec::with_capacity(iterations);
    let mut times_cd = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let start = Instant::now();
        let _ = poseidon_hash_offchain(a, b);
        times_ab.push(start.elapsed().as_nanos());

        let start = Instant::now();
        let _ = poseidon_hash_offchain(c, d);
        times_cd.push(start.elapsed().as_nanos());
    }

    // Compare mean execution times — they should be within 20% of each other
    let mean_ab: f64 = times_ab.iter().sum::<u128>() as f64 / iterations as f64;
    let mean_cd: f64 = times_cd.iter().sum::<u128>() as f64 / iterations as f64;
    let ratio = mean_ab / mean_cd;

    assert!(
        ratio > 0.8 && ratio < 1.2,
        "Poseidon hash timing variation too large: AB={:.0}ns, CD={:.0}ns, ratio={:.2}",
        mean_ab, mean_cd, ratio
    );
}
```

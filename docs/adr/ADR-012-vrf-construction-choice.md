# ADR-012: VRF Construction Choice

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-08-11

## Status

Accepted — Updated in Phase 5 (ECVRF V2 added alongside V1)

## Date

2025-05-18 (original), 2026-05-19 (Phase 5 update)

## Version

2.0.0

## Decision

Use Ed25519 signature with BLAKE3 derivation as the V1 VRF construction, and add ECVRF-ED25519 per RFC 9381 as the V2 construction. V1 remains the default for backward compatibility; V2 is available for new networks.

## Context

The Omnia protocol requires a Verifiable Random Function (VRF) for leader election in the BFT consensus. A VRF produces a pseudorandom output and a proof that the output was correctly computed from a secret key and input message, without revealing the secret key.

### V1 (Legacy, Deprecated)

The original implementation uses `vrf_compute()` which computes an Ed25519 signature over the round data and derives the VRF output using BLAKE3:

```
vrf_output = BLAKE3("OMNIA-VRF-V1" || public_key || signature || input)
```

This is NOT a standard VRF construction. It uses a hash-based output derivation instead of the algebraic derivation specified in ECVRF.

### V2 (ECVRF-ED25519, Target)

Phase 5 adds `ecvrf_prove()` and `ecvrf_verify()` implementing ECVRF-ED25519 per RFC 9381:

- Hash-to-curve with BLAKE3 domain separation (`OMNIA-ECVRF-H2C-V2`)
- Algebraic gamma computation (H \* secret_key)
- Fiat-Shamir challenge (hash of H, Gamma, k*B, k*H)
- Schnorr response (k + c \* sk mod l)
- Proof-to-hash derivation (BLAKE3 of gamma)

The V2 construction provides:

- **Zero-knowledge**: The proof does not reveal the secret key
- **Uniqueness**: Same (sk, alpha) → same output (deterministic)
- **Unpredictability**: Output is pseudorandom without the secret key

## Migration Plan

1. **Phase 5 (Current)**: Both V1 and V2 available. V1 is default. V2 via `select_leader_v2(_, _, V2)`.
2. **Phase 6 (Testnet)**: V2 enabled by default for new networks. V1 still supported for existing networks.
3. **Phase 7 (Mainnet)**: V2 required. V1 deprecated and only available for historical verification.

## Alternatives Considered

### ECVRF per draft-irtf-cfrg-vrf-15

The IETF draft specifies a standard VRF construction. Replaced by RFC 9381 in Phase 5.

### ECVRF per RFC 9381 (Chosen for V2)

RFC 9381 is the finalized VRF standard with stable specification. Implemented in Phase 5.

### Current Construction (Retained as V1)

The V1 construction remains supported for backward compatibility but is deprecated for new deployments.

## Consequences

### Positive

- V2 provides standard ECVRF with zero-knowledge proofs
- V1 backward compatibility ensures no network disruption
- Gradual migration path via `VrfVersion` enum
- BLAKE3 domain separation prevents cross-version attacks

### Negative

- Two VRF versions to maintain and test
- V2 implementation uses BLAKE3-based simplifications (not pure EC operations)
- Migration requires coordinated network upgrade
- V1 does not meet the cryptographic definition of a VRF

### Trade-offs

- V1 simplicity preserved for existing deployments
- V2 spec compliance available for new deployments
- Migration timeline allows testnet validation before mainnet switch

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

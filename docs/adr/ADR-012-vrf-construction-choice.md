# ADR-012: VRF Construction Choice

## Status

Accepted

## Date

2025-05-18

## Version

1.0.0

## Decision

Use Ed25519 signature with BLAKE3 derivation as the VRF construction, rather than the IETF ECVRF standard (draft-irtf-cfrg-vrf-15 / RFC 9381).

## Context

The Omnia protocol requires a Verifiable Random Function (VRF) for leader election in the BFT consensus. A VRF produces a pseudorandom output and a proof that the output was correctly computed from a secret key and input message, without revealing the secret key.

The current implementation uses `compute_leader()` which computes an Ed25519 signature over the round data and derives the VRF output using BLAKE3:
```
vrf_output = BLAKE3("OMNIA-VRF-LEADER-V1" || round || secret_key)
```

This is NOT a standard VRF construction. It uses a hash-based output derivation instead of the algebraic derivation specified in ECVRF.

## Alternatives Considered

### ECVRF per draft-irtf-cfrg-vrf-15
The IETF draft specifies a standard VRF construction using elliptic curve operations. It provides:
- Algebraic pseudorandomness (output derived from curve point encoding)
- Proof of correctness via zero-knowledge proof
- Standardized verification algorithm
However, it requires significant additional implementation effort and a new dependency.

### ECVRF per RFC 9381
RFC 9381 is the finalized version of the VRF standard. Same benefits and drawbacks as the draft, but with stable specification.

### Current Construction (Hash-based)
The current construction is simple and uses existing primitives (Ed25519 + BLAKE3), but:
- The output is not derived algebraically from the signature
- There is no zero-knowledge proof of correct evaluation
- The construction is NOT interoperable with other VRF implementations
- It does not meet the formal definition of a VRF in the cryptographic literature

## Consequences

### Positive
- Simple implementation using existing cryptographic primitives
- No additional dependencies required
- BLAKE3 domain separation prevents cross-protocol attacks
- Sufficient for internal leader election where all participants are known

### Negative
- NOT a standard VRF — does not meet the cryptographic definition
- Not interoperable with other VRF implementations (e.g., Algorand, DFINITY)
- No formal security proof for the construction
- Future hard fork required to migrate to a standard VRF

### Trade-offs
- Chose implementation simplicity and zero-dependency over spec compliance
- The construction is secure for the specific use case (leader election among known validators)
- Phase 3 should address VRF spec compliance or formally justify the current construction

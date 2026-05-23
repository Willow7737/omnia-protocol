# ADR-013: DKG Protocol Selection
> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-05-20

## Status

Accepted

## Date

2025-05-18

## Version

1.0.0

## Decision

Use Feldman Verifiable Secret Sharing (VSS) based Distributed Key Generation for threshold signature key management.

## Context

The threshold signature system in `substrate/src/threshold.rs` requires key shares to be distributed among participants without a trusted dealer. With a trusted dealer, the dealer knows all shares, creating a single point of compromise — if the dealer is compromised, the entire threshold signature scheme is compromised.

A DKG protocol allows `n` participants to jointly generate a threshold key pair where:
- No single participant knows the group secret
- Each participant holds a share that can produce partial signatures
- The group public key is publicly known
- `t` participants can combine their partial signatures into a valid threshold signature

## Alternatives Considered

### Rosario-Gennaro DKG
The Rosario-Gennaro DKG (2007) is a widely-cited protocol that improves on Pedersen's original DKG by ensuring that the shared secret is uniformly distributed even when some participants are malicious. It requires two rounds of communication and Pedersen commitments.

### FROST-based DKG
FROST (Flexible Round-Optimized Schnorr Threshold signatures) includes a DKG protocol that is optimized for Schnorr signatures. It is simpler than Feldman VSS but is specific to Schnorr signature schemes and would require adaptation for BLS signatures.

### Trusted Dealer (Status Quo)
Keep the existing manual key share registration. This is simpler but creates a single point of compromise.

## Consequences

### Positive
- No trusted dealer — the group secret is never known to any single participant
- Feldman commitments allow public verification of share correctness
- Byzantine participant detection via commitment verification
- DKG output feeds directly into the existing `ThresholdKeyManager`

### Negative
- Synchronous protocol — requires all participants to be online during generation
- The simplified implementation uses BLAKE3-derived shares rather than polynomial evaluation over a finite field
- No complaint mechanism for participants who receive invalid shares
- Current implementation encrypts shares with XOR (should use AES-256-GCM in production)

### Trade-offs
- Chose Feldman VSS over Rosario-Gennaro for simplicity
- Synchronous model is acceptable for initial deployment
- Future phases should add complaint resolution and asynchronous support

---
🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

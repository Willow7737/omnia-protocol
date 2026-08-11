# ADR-010: Encrypted Keystore Design

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-08-11

## Status

Accepted

## Date

2025-05-18

## Version

1.0.0

## Decision

Use AES-256-GCM with HKDF-SHA256 key derivation and BLAKE3 domain separation for validator key encryption at rest.

## Context

Phase 0 identified that the original keystore used XOR-based encryption, which provides no authentication and is vulnerable to known-plaintext attacks. The keystore must protect validator private keys on disk, support passphrase-based key derivation, and enable key rotation with cryptographic proof.

The keystore needs to:

- Encrypt Ed25519 secret keys at rest with authenticated encryption
- Derive encryption keys from user passphrases with per-encryption random salts
- Support key rotation with a verifiable proof (old key signs new key)
- Maintain backward compatibility with legacy XOR-encrypted stores during migration

## Alternatives Considered

### age Encryption

age is a modern file encryption tool/library with a simple API and strong defaults. However, it adds a heavy dependency and its recipient-based model doesn't align well with passphrase-derived key encryption.

### Hardware Security Modules (HSMs)

HSMs provide the strongest key protection but require specialized hardware, increasing deployment complexity and cost. Not suitable for a protocol that should run on commodity hardware.

### Chacha20-Poly1305

ChaCha20-Poly1305 is a viable alternative AEAD cipher that performs better on platforms without AES hardware acceleration. However, AES-256-GCM has broader hardware support (AES-NI) and is more widely audited.

## Consequences

### Positive

- AES-256-GCM provides authenticated encryption (encrypt-then-MAC in a single operation)
- HKDF-SHA256 with per-encryption salt prevents deterministic ciphertext
- BLAKE3 domain separation prevents cross-protocol hash collisions
- Legacy XOR stores can still be loaded (automatic upgrade on next write)
- Key rotation produces a verifiable Ed25519 signature proof

### Negative

- Software-only solution — no HSM integration yet
- Passphrase strength determines security (no minimum entropy enforcement)
- If the passphrase is lost, keys are unrecoverable (by design)

### Trade-offs

- Chose simplicity and auditability over HSM-level security
- Chose AES-256-GCM over ChaCha20-Poly1305 for hardware acceleration support
- Backward compatibility adds complexity but is necessary for migration

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

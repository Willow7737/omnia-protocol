# Layer 4: Identity Hardening

> 🎯 Audience: Developers
> 🔗 Context: Layer 4 enables self-sovereign identity where individuals, AI agents, and collectives own their identity forever
> 📅 Last Updated: 2026-05-20

## Overview

Identity is implemented within the `omnia-shards` crate (`IdentityShard`), not as a separate crate/layer. It provides self-sovereign identity where individuals, AI agents, and collectives own their identity forever.

## Components

### Decentralized Identifiers (DIDs) — ✅ Implemented

The `did:omnia:` method is fully implemented with validation.

**Format:** `did:omnia:<hex_public_key>` where `<hex_public_key>` is a 64-character hex string representing a 32-byte Ed25519 public key.

**Validation rules** (implemented in `shards/src/identity/did.rs`):

- Must start with `did:omnia:` (the `DID_PREFIX` constant)
- The method-specific identifier must be exactly 64 hex characters (32 bytes)
- The hex must be valid (no non-hex characters)

**Properties:**

- You create it yourself (no authority issues it)
- It's cryptographically verifiable
- It cannot be revoked or censored
- It's portable across platforms

### Shamir's Secret Sharing — ✅ Implemented

Social recovery uses **Shamir's Secret Sharing over GF(256)** (implemented in `shards/src/identity/recovery.rs`):

1. Your key is split into N shares using `ShamirRecovery::split(secret, threshold, total)`
2. Any threshold number of shares (e.g., 3 of 5) can reconstruct the key via `ShamirRecovery::reconstruct(shares)`
3. If you lose your key, guardians provide their shares
4. The key is reconstructed from the threshold number of shares using Lagrange interpolation
5. No single guardian has your full key (K-1 shares reveal nothing)

The GF(256) arithmetic uses the AES irreducible polynomial (0x11B) for reduction. The threshold must be at least 2.

**Recovery flow:**

- `IdentityOp::ConfigureRecovery` — splits secret, persists encrypted shares
- `IdentityOp::RecoverDid` — reconstructs secret, derives new Ed25519 keypair, adds to `doc.authentication` (rotation, not replacement)
- `recovery_count` incremented to prevent replay attacks
- Encrypted share storage with AES-256-GCM (BLAKE3 + HKDF-SHA256 key derivation)

### Biometric Anchors — ✅ Implemented

Privacy-preserving biometric anchors using `BLAKE3(salt || template)`. The raw template is never stored in cleartext.

Located in: `shards/src/identity/biometric.rs`

### AI Agent Identity — ✅ Implemented

AI agent identities with 5 capability types (in `shards/src/identity/agent.rs`):

```rust
pub enum AgentCapability {
    FinancialTransfer { max_amount: u64, currency: String },
    DataProcessing { domains: Vec<String>, max_records: u64 },
    ContractExecution { contract_types: Vec<String> },
    ComputeProof { max_compute_units: u64 },
    GovernanceVote { max_weight: u64 },
}
```

The `GovernanceVote` capability includes a `max_weight` parameter that limits the agent's maximum quadratic voting weight.

### Social Recovery — ✅ Implemented

Social recovery with configurable guardian threshold. The `IdentityOp::ConfigureRecovery` operation splits the secret and stores the threshold/total configuration. The `IdentityOp::RecoverDid` operation reconstructs the secret from K+ shares.

The reconstructed secret is used to rotate the DID's public key and authentication methods via `complete_recovery()`, which adds the recovered key to DID authentication (rotation, not replacement). A `recovery_count` is incremented to prevent replay attacks.

## Reputation System — Partially Implemented

| Component                              | Status                                   |
| -------------------------------------- | ---------------------------------------- |
| ✅ Exponential reputation decay        | Implemented (fixed-point PPM arithmetic) |
| ✅ Quadratic voting weight calculation | Implemented                              |
| 🌑 Full reputation scoring             | Not yet implemented                      |
| 📋 Reputation thresholds               | Planned                                  |

---

🔙 **Back**: [architecture/](./) | 🔄 **Related**: [layer-3-binding.md](./layer-3-binding.md)
🚀 **Next**: [layer-5-economics.md](./layer-5-economics.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

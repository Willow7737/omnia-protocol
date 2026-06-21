# Layer 2: Domain Shards

> 🎯 Audience: Developers
> 🔗 Context: Layer 2 organizes different types of activity into specialized lanes with optimized state management
> 📅 Last Updated: 2026-05-20

## Overview

Layer 2 organizes different types of activity into specialized lanes, each with optimized consensus and state management. Each domain shard is a projection of the unified state that maintains its own state tree, processes transactions relevant to its domain (via `EventProcessor` trait), can reference state from other shards atomically, and contributes to the global state root.

## Implemented Shards (6 total)

| Shard            | Purpose                                       | Location                        |
| ---------------- | --------------------------------------------- | ------------------------------- |
| 💰 Financial     | Balances, transfers, replay protection        | `shards/src/financial/`         |
| 🆔 Identity      | DID management, credentials, social recovery  | `shards/src/identity/`          |
| 📦 Physical      | Object registration, provenance tracking      | `shards/src/physical/`          |
| 🧮 Computational | AI training, proofs                           | `shards/src/computational/`     |
| 🧬 Biological    | Health records, bio-signals, consent registry | `shards/src/biological/`        |
| 📊 Economics     | UBC, governance, useful work                  | `shards/src/economics_shard.rs` |

## ShardRouter

The `ShardRouter` dispatches events to the correct shard by domain. Processing order:

1. Deserialize payload into `ShardPayload`
2. Check nonce for replay protection (`RedbNonceStore` for persistence)
3. Look up fee via `FeeSchedule::fee_for_op()`
4. Deduct fee from caller's UBC quota via `QuotaSystem::spend()`
5. Route operation to target shard

If the caller has insufficient UBC, the operation is rejected with `ShardError::InsufficientFee`.

A `ShardRouter::new_without_fees()` constructor is available for testing.

Located in: `shards/src/router.rs`

## Cross-Shard Transactions

Cross-shard messaging with causality proofs is implemented in `shards/src/cross_shard.rs`. A single transaction can atomically touch multiple shards via the `ShardRouter`. `CrossShardMessage` carries causality verification.

## Fee Enforcement

The `FeeSchedule` maps each `ShardOp` variant to a fixed `u64` fee:

| Domain            | Fee (UBC) |
| ----------------- | --------- |
| Financial         | 10        |
| Computational     | 5         |
| Physical          | 3         |
| Identity          | 2         |
| Biological        | 3         |
| Cross-Shard       | 15        |
| Economics/Default | 3         |

Fees are deducted atomically before shard dispatch. No fee refund on operation failure.

## Replay Protection

Per-creator nonce tracking with `RedbNonceStore` persistence across restarts. Production nodes **MUST** use persistent nonce storage; in-memory nonce tracking (the fallback when no data dir is configured) loses replay protection state on restart.

## Key Shard Details

### FinancialShard

⚠️ The `FinancialShard` uses **strict causal ordering** (not CRDTs) for balance consistency. This is critical — using CRDTs for financial balances could lead to double-spend scenarios under concurrent operations.

### IdentityShard

- `did:omnia:` method with full validation (`shards/src/identity/did.rs`)
- Shamir's Secret Sharing over GF(256) for social recovery (`shards/src/identity/recovery.rs`)
- Privacy-preserving biometric anchors: `BLAKE3(salt || template)` — template never stored in cleartext (`shards/src/identity/biometric.rs`)
- `AgentIdentity` with 5 capability types (`shards/src/identity/agent.rs`)
- Encrypted share storage with AES-256-GCM
- Recovery adds new key to DID authentication (rotation, not replacement)

### PhysicalShard

- `PhysicalOp::AnchorItem` creates immutable provenance entries
- `PhysicalOp::TransferOwnership` records transfers in append-only log
- `PhysicalOp::VerifyChain` validates complete provenance chain
- `ProvenanceTracker` with full create/transfer/verify/destroy lifecycle

### BiologicalShard

- Consent registry with `ConsentRecord`
- `BiologicalOp::GrantAccess` / `RevokeAccess` for patient-controlled data sharing
- `BiologicalOp::QueryWithZkProof` — ZK proof for privacy-preserving queries (stub verifier for now)

## Shard Architecture Diagram

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│  Financial  │    │  Identity   │    │  Physical   │
│   Shard ✅  │    │   Shard ✅  │    │   Shard ✅  │
└──────┬──────┘    └──────┬──────┘    └──────┬──────┘
       │                  │                  │
       └────────┬─────────┴────────┬────────┘
                │                  │
         ┌──────┴──────┐    ┌──────┴──────┐
         │ Computational│    │  Biological │
         │   Shard ✅   │    │   Shard ✅  │
         └──────┬──────┘    └──────┬──────┘
                │                  │
                └────────┬─────────┘
                         │
                  ┌──────┴──────┐
                  │  Economics  │
                  │   Shard ✅  │
                  └──────┬──────┘
                         │
                  ┌──────┴──────┐
                  │  ShardRouter│
                  │ (EventProc) │
                  └──────┬──────┘
                         │
                  ┌──────┴──────┐
                  │  Substrate  │
                  │ (CausalGraph│
                  └─────────────┘
```

---

🔙 **Back**: [architecture/](./) | 🔄 **Related**: [trait-boundaries.md](./trait-boundaries.md)
🚀 **Next**: [layer-3-binding.md](./layer-3-binding.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

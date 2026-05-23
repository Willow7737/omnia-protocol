# Layer 5: Economics
> 🎯 Audience: Developers
> 🔗 Context: Layer 5 creates a monetary system that serves people, not extracts from them
> 📅 Last Updated: 2026-05-20

## Overview

The Economics layer implements a monetary system designed to serve participants through Universal Basic Compute, quadratic voting, and a fee-based sustainability model.

## Universal Basic Compute (UBC) — ✅ Implemented

Every identity receives a soulbound (non-transferable) monthly quota via the UBC token (implemented in `economics/src/ubc.rs`).

Key parameters (from `economics/src/quota.rs`):
- **Default quota**: 1,000 UBC/month (`DEFAULT_UBC_QUOTA`)
- **Epoch duration**: 30 days (2,592,000,000 ms, `DEFAULT_EPOCH_DURATION_MS`)
- **Monthly reset**: Balances are reset to the monthly quota at each epoch boundary; unspent balance is forfeited (anti-hoarding)
- **Useful-work rewards**: Extra UBC can be earned via `UbcToken::reward()` (additive, not reset)

### UBC Token Model

- `UbcToken::mint_monthly()` — resets balance to monthly quota at epoch boundaries
- `UbcToken::spend()` — consumes UBC for transactions (destroyed, not transferred)
- `UbcToken::reward()` — adds UBC for useful-work contributions (additive, not reset at epoch boundaries)

UBC tokens are **soulbound** — they cannot be transferred between identities.

### Proof-of-Useful-Work — ⚠️ Stub

3 work types defined in `economics/src/useful_work.rs`:
- `AiTraining { model_hash, training_data_hash }` — AI model training
- `ScientificSimulation { simulation_id, params_hash }` — Distributed computation
- `DistributedStorage { data_hash, storage_duration }` — Data hosting

Verification is currently a stub (`UsefulWorkProof::verify_stub()`) that checks for non-zero result hash and positive compute units. Reward amount equals compute units consumed (1:1 ratio).

## Governance — ✅ Implemented (partially)

### Quadratic Voting

Voting power = `isqrt(stake)`, where `isqrt` is the integer square root via Newton's method defined in `economics/src/fixed_point.rs`.

**Effect:** One large stakeholder has proportionally less power than many small stakeholders. This prevents whale dominance while still rewarding commitment.

Implemented in `economics/src/governance.rs` with multiplicative reputation decay using fixed-point PPM arithmetic (no floating-point). The `VoteChoice` enum supports For, Against, Abstain.

### Reputation Decay

Decay formula (fixed-point PPM arithmetic, no f64):

```
effective_weight = base_weight * remaining_ppm / BASIS_PPM
where remaining_ppm = (BASIS_PPM - decay_rate.ppm())^epochs / BASIS_PPM^epochs
```

Default decay rate: `DecayRate::ten_percent()` = 100,000 PPM per epoch.

**Effect:** Power cannot concentrate. Even early adopters must stay active to maintain influence.

### Time-Locked Voting — ✅ Implemented

Stake must be locked for a minimum duration before it grants voting power, preventing flash loan attacks:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `min_lock_duration` | 100 blocks | Minimum lock (~8 min at 5s finality) |
| `max_lock_duration` | 100,000 blocks | Maximum lock (~6 days at 5s finality) |
| `strict_enforcement` | true | No early withdrawals |

Freshly-locked stake has zero voting power until the lock matures.

### Governance Quorum — ✅ Implemented

`GovernanceState::quorum_percentage` (default 67%) — total votes cast must represent ≥ 67% of total possible voting weight.

Time-lock: `GovernanceState::time_lock_ms` (default 86,400,000 ms = 24 hours) — after finalization, `execution_time` is set to `current_time_ms + time_lock_ms`.

### What's Not Yet Implemented

| Feature | Status | Notes |
|---------|--------|-------|
| Conviction voting | 📋 Planned | Graduated multipliers based on lock duration |
| Delegation | 📋 Planned | Delegating voting power to trusted representatives |
| Treasury | 🌑 Aspirational | No transaction fee distribution mechanism |
| RPGF | 🌑 Aspirational | Retroactive Public Goods Funding — no implementation |

## Slashing — ✅ Implemented

The `SlashingEngine` tracks three offense types with configurable thresholds. ADR-011 defines a gradual slashing model with 3-tier escalation:

| Offense | 1st | 2nd | 3rd+ |
|---------|-----|-----|-------|
| Equivocation | Jailed (5%, 1000r) | Jailed (25%, 5000r) | Ejected (100%) |
| LivenessViolation | Warning (1%) | Warning (1%) | Jailed (5%, 500r) |
| InvalidAttestation | Warning (2%) | Jailed (10%, 2000r) | Ejected (100%) |

Persistent storage via `RedbSlashingStore`. The `omnia-node` binary configures redb persistence automatically.

## Fee Structure — ✅ Implemented

The `FeeSchedule` maps operations to fixed UBC fees:

| Category | Fee (UBC) |
|----------|-----------|
| Identity operations | 2 |
| Physical operations | 3 |
| Biological operations | 3 |
| Economics/Default | 3 |
| Computational operations | 5 |
| Financial operations | 10 |
| Cross-shard operations | 15 |

---
🔙 **Back**: [architecture/](./) | 🔄 **Related**: [layer-4-identity.md](./layer-4-identity.md)
🚀 **Next**: [zk-rollup-settlement.md](./zk-rollup-settlement.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

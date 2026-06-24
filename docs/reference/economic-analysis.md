# Economic Parameter Analysis

> 🎯 Audience: Architects
> 🔗 Context: Comprehensive analysis of economic parameters for mainnet readiness
> 📅 Last Updated: 2026-05-20

**Phase F2 Deliverable**
**Sprint 6 — Security Hardening**
**Version:** v0.1.68

---

## 1. Overview

This document provides a comprehensive analysis of the economic parameters in the Omnia Protocol. It examines the fee schedule, spam resistance mechanisms, quota system, governance parameters, time-locked voting, and slashing economics. The goal is to validate that the current parameters provide adequate security and economic incentives for mainnet deployment.

All parameter values referenced herein are sourced from the actual codebase: the `omnia-shards` crate (`shards/src/`) and the `omnia-economics` crate (`economics/src/`). Where the code implements a feature, the document describes the implementation precisely; where features are planned but not yet coded, they are clearly marked as recommendations or future work.

---

## 2. Fee Schedule Parameters

### 2.1 Current Fee Schedule

The `FeeSchedule` (defined in `shards/src/fee_schedule.rs`) specifies per-operation fees in UBC (Universal Basic Compute) units. The struct has seven flat-fee fields, one per domain plus a cross-shard fee and a default fallback:

```rust
// shards/src/fee_schedule.rs
pub struct FeeSchedule {
    pub financial_op_fee: u64,
    pub computational_op_fee: u64,
    pub physical_op_fee: u64,
    pub identity_op_fee: u64,
    pub biological_op_fee: u64,
    pub cross_shard_fee: u64,
    pub default_fee: u64,
}
```

The `FeeSchedule::standard()` constructor provides the production defaults:

| Domain                 | Fee Field              | Fee (UBC) | Relative Cost |
| ---------------------- | ---------------------- | --------- | ------------- |
| Financial              | `financial_op_fee`     | 10        | 5× baseline   |
| Computational          | `computational_op_fee` | 5         | 2.5× baseline |
| Physical               | `physical_op_fee`      | 3         | 1.5× baseline |
| Identity               | `identity_op_fee`      | 2         | 1× baseline   |
| Biological             | `biological_op_fee`    | 3         | 1.5× baseline |
| Cross-Shard            | `cross_shard_fee`      | 15        | 7.5× baseline |
| Economics              | `default_fee`          | 3         | 1.5× baseline |
| Default (unrecognized) | `default_fee`          | 3         | 1.5× baseline |

The `Economics` variant of `ShardOp` falls back to `default_fee` (see `FeeSchedule::fee_for_op()`). A `FeeSchedule::zero()` constructor is also available for testing.

### 2.2 Fee Rationale

**Financial operations (10 UBC)** are priced highest because they:

- Modify account balances (high-value state changes)
- Are the primary target for spam and abuse
- Require strict causal ordering (non-CRDT)

**Cross-shard operations (15 UBC)** are priced above financial operations because they:

- Require coordination across multiple shards
- Consume more network bandwidth (causality proofs)
- Are inherently more expensive to process

**Identity operations (2 UBC)** are priced lowest because they:

- Are infrequent (DID creation is a one-time operation)
- Are critical for network participation (low barriers to entry)
- Modify relatively simple state

**Computational operations (5 UBC)** are priced mid-range because they:

- May involve expensive proof verification
- Are expected to be the highest-volume operation type
- Balance between spam resistance and accessibility

---

### 2.3 Fee Enforcement in the ShardRouter

The `ShardRouter` (`shards/src/router.rs`) enforces fees through the `route_event()` method. The flow is:

1. **Payload deserialization**: The event's byte payload is deserialized into a `ShardPayload`.
2. **Replay protection**: The nonce is checked against `last_nonces` (and persisted via a `NonceStore`).
3. **Fee lookup**: `fee_schedule.fee_for_op(&payload.operation)` returns the UBC cost.
4. **Quota deduction**: If `fee > 0`, the router calls `self.quota.spend(&did, fee)`, converting the creator's Ed25519 public key to a DID via `ShardRouter::pubkey_to_did()`.
5. **Dispatch**: If the fee deduction succeeds, the operation is routed to the target shard.

If the caller has insufficient UBC, the operation is rejected with `ShardError::InsufficientFee`.

There is no formula-based fee calculation (no base fee + per-byte fee). The fee schedule is purely a flat per-operation-type lookup. A `ShardRouter::new_without_fees()` constructor creates a router with `FeeSchedule::zero()` for testing scenarios.

---

## 3. Spam Resistance Analysis

### 3.1 Cost Model

Assuming 1 UBC = $0.001 (tentative mainnet pricing), the cost of a sustained spam attack:

| Attack Type          | Ops/Second | UBC/Op | Cost/Second | Cost/Hour | Cost/Day   |
| -------------------- | ---------- | ------ | ----------- | --------- | ---------- |
| Financial spam       | 1,000      | 10     | $10.00      | $36,000   | $864,000   |
| Computational spam   | 1,000      | 5      | $5.00       | $18,000   | $432,000   |
| Identity spam        | 1,000      | 2      | $2.00       | $7,200    | $172,800   |
| Cross-shard spam     | 1,000      | 15     | $15.00      | $54,000   | $1,296,000 |
| Mixed spam (average) | 1,000      | 6.7    | $6.70       | $24,120   | $578,880   |

### 3.2 UBC Quota as Spam Defense

The UBC system provides a monthly quota that limits the total compute any single identity can consume:

| Parameter         | Value                      | Code Reference                                          |
| ----------------- | -------------------------- | ------------------------------------------------------- |
| Default UBC quota | 1,000 UBC/month            | `DEFAULT_UBC_QUOTA` in `economics/src/quota.rs`         |
| Epoch duration    | 2,592,000,000 ms (30 days) | `DEFAULT_EPOCH_DURATION_MS` in `economics/src/quota.rs` |
| Monthly epochs    | 1                          | Single 30-day epoch per month                           |

**Quota exhaustion analysis:**

| Operation Mix               | Ops Until Quota Exhausted | Time to Exhaust (at 1 op/s) |
| --------------------------- | ------------------------- | --------------------------- |
| Financial only (10 UBC)     | 100                       | ~2 minutes                  |
| Computational only (5 UBC)  | 200                       | ~3 minutes                  |
| Identity only (2 UBC)       | 500                       | ~8 minutes                  |
| Cross-shard only (15 UBC)   | 67                        | ~1 minute                   |
| Realistic mix (avg 6.7 UBC) | ~150                      | ~2.5 minutes                |

**Conclusion**: A single identity with the default 1,000 UBC/month quota can only sustain ~150 operations before exhaustion. At realistic transaction rates, this provides approximately 2–8 minutes of sustained usage per month per identity. This is adequate for normal usage but insufficient for spam attacks.

### 3.3 Sybil Resistance of UBC Quota

An attacker creating N identities gains N × 1,000 = 1,000N UBC/month:

| Identities | Total UBC  | Financial Ops | Cost (DID creation) | Net Economic Feasibility |
| ---------- | ---------- | ------------- | ------------------- | ------------------------ |
| 1          | 1,000      | 100           | $0.002              | Feasible                 |
| 10         | 10,000     | 1,000         | $0.02               | Feasible                 |
| 100        | 100,000    | 10,000        | $0.20               | Feasible                 |
| 1,000      | 1,000,000  | 100,000       | $2.00               | Feasible                 |
| 10,000     | 10,000,000 | 1,000,000     | $20.00              | Feasible (cheap)         |

**Problem**: Without Sybil-resistant DID creation, the UBC quota system is vulnerable to identity farming. An attacker can create 10,000 identities for ~$20 and gain 10M UBC/month — enough for 1,000,000 financial operations.

**Mitigation**: See Recommendation #3 (Sybil-resistant DID creation) below.

---

## 4. Quota Exhaustion Analysis

### 4.1 Legitimate User Quota Consumption

For a typical user performing normal operations:

| Operation            | Monthly Frequency | UBC/Op | Total UBC |
| -------------------- | ----------------- | ------ | --------- |
| DID creation         | 1                 | 2      | 2         |
| Financial transfers  | 50                | 10     | 500       |
| Computational tasks  | 20                | 5      | 100       |
| Identity updates     | 5                 | 2      | 10        |
| Physical anchoring   | 10                | 3      | 30        |
| Cross-shard messages | 15                | 15     | 225       |
| **Total**            |                   |        | **867**   |

**Result**: A typical user consumes 867 UBC/month, leaving only 133 UBC (13.3%) as buffer. This is tight — any unexpected usage could exhaust the quota before month-end.

### 4.2 Power User Quota Consumption

| Operation            | Monthly Frequency | UBC/Op | Total UBC |
| -------------------- | ----------------- | ------ | --------- |
| DID creation         | 3                 | 2      | 6         |
| Financial transfers  | 200               | 10     | 2,000     |
| Computational tasks  | 100               | 5      | 500       |
| Identity updates     | 20                | 2      | 40        |
| Physical anchoring   | 50                | 3      | 150       |
| Cross-shard messages | 50                | 15     | 750       |
| **Total**            |                   |        | **3,446** |

**Result**: A power user needs 3,446 UBC/month — 3.4× the default quota. This is achievable through:

- Proof-of-Useful-Work rewards (earning extra UBC via `UbcToken::reward()`)
- Higher-tier identity verification (increased quota, future feature)

### 4.3 Recommendations for Quota Parameters

| Parameter         | Current Value              | Code Location                                        | Recommended Value | Rationale                                 |
| ----------------- | -------------------------- | ---------------------------------------------------- | ----------------- | ----------------------------------------- |
| Default UBC quota | 1,000                      | `economics/src/quota.rs` `DEFAULT_UBC_QUOTA`         | 2,000             | Provides adequate buffer for normal users |
| Epoch duration    | 30 days (2,592,000,000 ms) | `economics/src/quota.rs` `DEFAULT_EPOCH_DURATION_MS` | 30 days           | Keep — monthly cycle is user-friendly     |
| Power user quota  | N/A                        | Not implemented                                      | 10,000            | For verified high-volume identities       |

---

## 5. Fee Recommendations for Mainnet

### 5.1 Mainnet Fee Schedule Proposal

Based on the spam resistance analysis, we recommend the following mainnet fee schedule:

| Domain        | Testnet Fee | Mainnet Fee (Recommended) | Change | Rationale                                      |
| ------------- | ----------- | ------------------------- | ------ | ---------------------------------------------- |
| Financial     | 10 UBC      | 25 UBC                    | +150%  | Higher value state changes warrant higher fees |
| Computational | 5 UBC       | 10 UBC                    | +100%  | Proof verification is CPU-intensive            |
| Physical      | 3 UBC       | 8 UBC                     | +167%  | Asset anchoring has real-world value           |
| Identity      | 2 UBC       | 3 UBC                     | +50%   | Low barrier to entry maintained                |
| Biological    | 3 UBC       | 8 UBC                     | +167%  | Consent operations have legal implications     |
| Cross-shard   | 15 UBC      | 35 UBC                    | +133%  | Cross-shard coordination is expensive          |
| Default       | 3 UBC       | 8 UBC                     | +167%  | Consistent with mid-tier operations            |

### 5.2 Dynamic Fee Adjustment (Future)

The current fee schedule is static — `FeeSchedule::fee_for_op()` returns a constant per variant. For long-term sustainability, we recommend implementing dynamic fee adjustment similar to EIP-1559:

- **Base fee**: Algorithmically adjusted based on network congestion
- **Priority fee**: Optional tip for faster inclusion
- **Fee burning**: Burn a portion of fees to manage token supply
- **Fee cap**: Maximum fee per operation to prevent fee spikes

This is planned for Phase 2 (see R7 in THREAT_MODEL.md).

### 5.3 Spam Cost at Recommended Mainnet Fees

| Attack Type      | Ops/Second | Fee/Op | Cost/Second | Cost/Hour | Cost/Day   |
| ---------------- | ---------- | ------ | ----------- | --------- | ---------- |
| Financial spam   | 1,000      | 25     | $25.00      | $90,000   | $2,160,000 |
| Mixed spam (avg) | 1,000      | 15.3   | $15.30      | $55,080   | $1,321,920 |

At recommended mainnet fees, a 24-hour sustained spam attack costs $1.3M–$2.2M — a significant economic deterrent.

---

## 6. Governance Parameters

### 6.1 Quadratic Voting Analysis

The governance module (`economics/src/governance.rs`) implements quadratic voting with multiplicative reputation decay using fixed-point arithmetic:

**Formula**: `voting_weight = isqrt(stake)` where `isqrt` is integer square root via Newton's method (defined in `economics/src/fixed_point.rs`).

```rust
// economics/src/governance.rs
pub fn set_weight(&mut self, did: &str, stake: u64) {
    let weight = isqrt(stake).max(1);
    self.voting_weights.insert(did.to_string(), weight);
    self.last_active.insert(did.to_string(), 0);
}
```

| Stake (tokens) | Voting Weight | Weight/Stake Ratio | Power Advantage vs. 1-token holder |
| -------------- | ------------- | ------------------ | ---------------------------------- |
| 1              | 1             | 1.000              | 1×                                 |
| 10             | 3             | 0.300              | 3×                                 |
| 100            | 10            | 0.100              | 10×                                |
| 1,000          | 31            | 0.031              | 31×                                |
| 10,000         | 100           | 0.010              | 100×                               |
| 100,000        | 316           | 0.003              | 316×                               |
| 1,000,000      | 1,000         | 0.001              | 1,000×                             |

**Key Property**: To double your voting power, you must quadruple your stake. This prevents plutocratic dominance:

| Action                    | Additional Stake Needed | Marginal Cost per Vote |
| ------------------------- | ----------------------- | ---------------------- |
| 1st vote                  | 1                       | 1                      |
| 2nd vote (2→4)            | 3                       | 3                      |
| 3rd vote (4→9)            | 5                       | 5                      |
| 10th vote (81→100)        | 19                      | 19                     |
| 100th vote (9,801→10,000) | 199                     | 199                    |

### 6.2 Reputation Decay

The decay rate is configured as parts-per-million (PPM) per epoch, implemented in `economics/src/fixed_point.rs`:

- **Default**: `DecayRate::ten_percent()` = 100,000 PPM = 10% decay per epoch
- **Mechanism**: `effective_weight = base_weight * remaining_ppm / BASIS_PPM`
- **Basis**: `BASIS_PPM = 1_000_000` (fixed-point arithmetic, no f64 anywhere in the economics crate)
- **All results are bit-for-bit identical across platforms** (x86, ARM, etc.)

The decay formula is computed iteratively via `DecayRate::remaining_ppm_after(epochs)`, which multiplies by `(BASIS_PPM - rate)` and divides by `BASIS_PPM` per epoch, avoiding overflow through checked arithmetic:

```rust
// economics/src/fixed_point.rs
pub fn remaining_ppm_after(&self, epochs: u64) -> u64 {
    // ... see code for full implementation
    let remaining_per_epoch = BASIS_PPM - self.ppm;
    let mut result: u64 = BASIS_PPM;
    for _ in 0..epochs {
        result = result
            .checked_mul(remaining_per_epoch)
            .and_then(|v| v.checked_div(BASIS_PPM))
            .unwrap_or(0);
        if result == 0 { break; }
    }
    result
}
```

**Decay trajectory** (10% per epoch, base weight = 100):

| Epochs Inactive | Remaining PPM | Effective Weight |
| --------------- | ------------- | ---------------- |
| 0               | 1,000,000     | 100              |
| 1               | 900,000       | 90               |
| 2               | 810,000       | 81               |
| 5               | 590,490       | 59               |
| 10              | 348,678       | 34               |
| 20              | 121,577       | 12               |
| 30              | 42,391        | 4                |
| 50              | 5,153         | 0 (rounds down)  |

**Result**: After 50 epochs of inactivity, a voter's weight drops to effectively zero. This ensures that only active participants have governance influence.

### 6.3 Governance Attack Analysis

**Whale attack**: An adversary with 1,000,000 tokens has 1,000 voting weight. To pass a proposal against the will of 1,000 small holders (1 token each = 1,000 weight total), the whale would need >1,000,000 tokens (1,000 weight). This is a stalemate — neither side can easily dominate.

**Sybil governance attack**: Creating 1,000 identities with 1 token each gives 1,000 × 1 = 1,000 voting weight — equivalent to 1 identity with 1,000,000 tokens. This means quadratic voting alone does not prevent Sybil governance attacks. Mitigation: identity verification requirements for governance participation.

### 6.4 Time-Locked Voting — Implemented

Time-locked voting is **implemented** in `economics/src/time_lock.rs`. It prevents flash loan attacks by requiring stake to be locked for a minimum duration before it grants voting power.

The `TimeLockConfig` struct defines the parameters:

| Parameter             | Default Value                           | Code Field           |
| --------------------- | --------------------------------------- | -------------------- |
| Minimum lock duration | 100 blocks (~8 min at 5s finality)      | `min_lock_duration`  |
| Maximum lock duration | 100,000 blocks (~6 days at 5s finality) | `max_lock_duration`  |
| Strict enforcement    | true                                    | `strict_enforcement` |

The `LockedStake` struct tracks individual locks:

```rust
// economics/src/time_lock.rs
pub struct LockedStake {
    pub owner: NodeId,
    pub amount: u64,
    pub lock_start: u64,
    pub lock_end: u64,
    pub released: bool,
}
```

**Flash loan prevention**: A freshly-locked stake has zero voting power until `current_height >= lock_end`. This means borrowed funds cannot be used for voting, because the lock must mature before any power is granted.

The `TimeLockVoting` struct provides:

- `lock()` — Lock stake for a duration (enforces min/max bounds)
- `voting_power()` — Sum of mature, non-released stakes
- `release_expired()` — Release mature stakes
- `can_vote()` — Check if a node has any voting power

---

## 7. Slashing Parameters

### 7.1 Current Slashing Parameters

The `SlashingEngine` (defined in `substrate/src/slashing.rs`) uses the following parameters:

> **Note:** ADR-011 graded slashing was implemented in Phase 4 (H-1). The binary model described above has been replaced with graded tiers (Warning, Jailed, Ejected) with configurable thresholds. See `omnia-consensus/src/slashing.rs`.

| Parameter                  | Value        | Description                                          |
| -------------------------- | ------------ | ---------------------------------------------------- |
| Slash threshold            | 500 points   | Points at which a node is slashed (stake forfeited)  |
| Ejection threshold         | 2,000 points | Points at which a node is ejected from validator set |
| Equivocation points        | 500          | Points for double-signing                            |
| Liveness violation points  | 100          | Points for being offline too long                    |
| Invalid attestation points | 300          | Points for attesting to invalid data                 |

### 7.2 Offense Accumulation Analysis

| Scenario                                            | Offenses  | Total Points | Outcome                  |
| --------------------------------------------------- | --------- | ------------ | ------------------------ |
| Single equivocation                                 | 1 × 500   | 500          | Slashed (at threshold)   |
| Accumulated liveness violations                     | 5 × 100   | 500          | Slashed (at threshold)   |
| Mixed offenses (1 liveness + 1 invalid attestation) | 100 + 300 | 400          | Warned (below threshold) |
| Persistent liveness violations                      | 20 × 100  | 2,000        | Ejected (at threshold)   |
| Multiple equivocations                              | 4 × 500   | 2,000        | Ejected (at threshold)   |

### 7.3 Slashing Economics

Assuming a validator stakes 10,000 tokens ($10,000 at $1/token):

| Offense                                 | Slash Points | Stake Forfeited     | Economic Penalty                 |
| --------------------------------------- | ------------ | ------------------- | -------------------------------- |
| Liveness violation                      | 100          | 0 (warned)          | $0 (reputation damage only)      |
| Invalid attestation                     | 300          | 0 (warned)          | $0 (reputation damage only)      |
| First equivocation                      | 500          | 10,000 (full stake) | $10,000                          |
| Second equivocation (after first slash) | 500          | 10,000 (full stake) | $10,000                          |
| Persistent liveness (20 violations)     | 2,000        | 10,000 (ejected)    | $10,000 + loss of future rewards |

**Issue**: The current implementation slashes the entire stake when the threshold is reached. This is a binary outcome — there is no partial slashing. A validator with 500 points (threshold = 500) loses 100% of their stake.

### 7.4 Recommended Slashing Parameters for Mainnet

| Parameter                  | Current Value | Recommended Value                      | Rationale                                         |
| -------------------------- | ------------- | -------------------------------------- | ------------------------------------------------- |
| Slash threshold            | 500           | 500                                    | Keep — single equivocation triggers slash         |
| Ejection threshold         | 2,000         | 1,500                                  | Lower — eject persistent offenders sooner         |
| Equivocation points        | 500           | 500                                    | Keep — equivocation is the most severe offense    |
| Liveness violation points  | 100           | 50                                     | Lower — avoid penalizing brief network issues     |
| Invalid attestation points | 300           | 400                                    | Higher — invalid attestations undermine trust     |
| Slash percentage           | 100%          | Proportional (points/threshold × 100%) | Graduated — partial slashing for partial offenses |
| Slashing reward            | 0%            | 10% of slashed stake                   | Incentivize whistle-blowing                       |

### 7.5 Proportional Slashing Formula

We recommend replacing the binary slash with proportional slashing:

```
slash_percentage = min(100%, (total_points - slash_threshold + offense_points) / ejection_threshold * 100%)
slash_amount = stake * slash_percentage
```

**Example**: Validator with 10,000 stake, 500 points (first equivocation):

```
slash_percentage = (500 - 500 + 500) / 1500 * 100% = 33.3%
slash_amount = 10,000 * 0.333 = 3,333 tokens
```

This is more proportional to the offense severity and avoids the "all-or-nothing" problem.

### 7.6 Slashing Rewards

To incentivize the detection and reporting of Byzantine behavior, we recommend awarding 10% of slashed stake to the reporter:

| Offense                     | Slash Amount | Reporter Reward | Burned | Treasury |
| --------------------------- | ------------ | --------------- | ------ | -------- |
| Equivocation (proportional) | 3,333        | 333             | 1,667  | 1,333    |
| Full ejection               | 10,000       | 1,000           | 5,000  | 4,000    |

**Distribution**:

- 10% to reporter (whistle-blower incentive)
- 50% burned (deflationary pressure)
- 40% to treasury (RPGF and community funding)

> **v0.1.69 audit fix (C-6):** fees are non-refundable on operation failure.

---

## 8. Summary of Recommendations

| #   | Parameter                    | Current         | Recommended                          | Priority       |
| --- | ---------------------------- | --------------- | ------------------------------------ | -------------- |
| 1   | Default UBC quota            | 1,000           | 2,000                                | Medium         |
| 2   | Financial op fee             | 10 UBC          | 25 UBC                               | High (mainnet) |
| 3   | Cross-shard fee              | 15 UBC          | 35 UBC                               | High (mainnet) |
| 4   | Identity op fee              | 2 UBC           | 3 UBC                                | Low            |
| 5   | Liveness violation points    | 100             | 50                                   | Medium         |
| 6   | Invalid attestation points   | 300             | 400                                  | Medium         |
| 7   | Ejection threshold           | 2,000           | 1,500                                | Medium         |
| 8   | Slash percentage             | 100% (binary)   | Proportional                         | High (mainnet) |
| 9   | Slashing reporter reward     | 0%              | 10%                                  | High (mainnet) |
| 10  | Sybil-resistant DID creation | Not implemented | Implement (biometric or stake-based) | High           |
| 11  | Dynamic fee adjustment       | Not implemented | Implement (EIP-1559-style)           | Low (Phase 2)  |

### Implementation Priority

1. **Before mainnet**: Items #2, #3, #8, #9, #10 — these are critical for economic security
2. **Phase 1**: Items #1, #5, #6, #7 — these improve the system but are not blocking
3. **Phase 2**: Items #4, #11 — these are optimizations for the mature protocol

---

_This analysis should be updated when economic parameters are modified or when market conditions change significantly._

---

🔙 **Back**: [Reference Index](../) | 🔄 **Related**: [Roadmap](./roadmap.md)
🚀 **Next**: [Benchmark Gates](./benchmark-gates.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

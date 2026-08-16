# OMNIA Token Decision Sheet

> **Status**: DRAFT — Aligned to Financial Specification v1.0-draft  
> **Last Updated**: 2026-08-15  
> **Owner**: Economics Lead  
> **Source of Truth**: `docs/financial/financial-specification.md` (Sections 3, 5, 7, 10, 11, 19)  
> **Classification**: Internal — Pre-Mainnet

---

## 1. Purpose

This document records every material decision about the OMNIA token, as derived from the Omnia Protocol Financial Specification. It is the single reference for implementers, auditors, and regulators. Where this document and the Financial Specification conflict, the Financial Specification prevails.

---

## 2. Token Identity

| Property | Value | Source |
|----------|-------|--------|
| **Token Name** | OMNIA | Spec §3.2 |
| **Network** | Omnia Protocol (Substrate-based) |
| **Standard** | Native Substrate BALANCE pallet (to evolve to asset-registry model per Spec §4) |
| **Decimals** | 9 | `asset-registry/src/types.rs` (`AssetId::OMNIA`) |
| **Transferability** | Fully transferable | Spec §3.2 |
| **Value Behavior** | Floating — no fixed GHS redemption promise | Spec §10, §19 |
| **Hard Cap** | 1,000,000,000 OMNIA (working assumption, subject to final genesis reconciliation) | Spec §5.1 |

---

## 3. Relationship to UBC

UBC and OMNIA are **deliberately separate financial layers**. This is a foundational architectural invariant.

| Dimension | UBC | OMNIA |
|-----------|-----|-------|
| **Nature** | Participation and compute allowance | Native transferable economic asset |
| **Purpose** | Basic identity and participation, compute access | Value transfer, merchant payments, staking collateral (future), governance (future) |
| **Transferability** | Non-transferable between user accounts | Fully transferable |
| **Issuance** | Epoch/eligibility protocol (reset/replenished) | Genesis, bounded treasury/governance policy |
| **Supply relation** | Excluded from OMNIA monetary supply | Separate supply tracking |
| **Marketing** | MUST NOT be marketed as money or a tradable token | Floating transferable digital asset within Omnia ecosystem |

**Design rules (from Spec §3.1):**

- UBC MUST remain non-transferable between user accounts.
- UBC MUST be separately identified in wallet, API, accounting, and event schemas.
- No UBC operation can create a transferable OMNIA balance (Spec §4.4 invariant).
- UBC MUST NOT be burned as OMNIA (Spec §7.3).

---

## 4. Relationship to External Assets

External assets (Bitcoin, etc.) are separate from the OMNIA monetary system (Spec §3.3):

- Every external asset MUST have its own `AssetId`, chain identifier, address model, decimal precision, confirmation policy, fee asset, custody model, adapter status, and reconciliation process.
- Bitcoin MUST NOT be represented as newly minted OMNIA.
- A Bitcoin adapter MAY record an external settlement reference but MUST NOT invoke the native OMNIA mint authority.
- No external-chain adapter can invoke native OMNIA minting (Spec §4.4 invariant).
- External-chain fees MUST NOT be misrepresented as OMNIA burns (Spec §7.3).

---

## 5. Monetary Policy

### 5.1 Supply Model

Working hard cap: **1,000,000,000 OMNIA** (Spec §5.1). This is a design assumption for modeling and implementation scaffolding, not a public promise until genesis configuration, allocation contracts, treasury policy, reward schedule, and governance limits are reconciled and independently reviewed.

The hard cap MUST be enforced by a protocol invariant (Spec §5.1):

```text
circulating_supply
+ locked_supply
+ treasury_supply
+ escrow_supply
+ unissued_reward_budget
≤ hard_cap
```

Burns reduce total supply permanently. Unissued rewards MUST remain outside circulating supply and MUST not be counted as already minted.

### 5.2 Genesis Allocation Framework

From Spec §5.2 — working model, must be finalized through economic model and governance process before genesis:

| Bucket | Share | Amount | Release Principle |
|--------|------:|-------:|-------------------|
| **Network incentives** | 40% | 400,000,000 | Decaying, bounded, performance-linked rewards |
| **Team and contributors** | 15% | 150,000,000 | Code-enforced vesting; four-year vest with one-year cliff |
| **Early investors/seed** | 10% | 100,000,000 | Only if actual investors exist; subject to legal and disclosure review |
| **Ecosystem fund** | 15% | 150,000,000 | Milestone-based grants and partnerships |
| **Treasury reserve** | 10% | 100,000,000 | Multisig-controlled operations and contingency reserve |
| **Liquidity and market operations** | 10% | 100,000,000 | Transparent liquidity and settlement facility; no price guarantee |
| **Total** | **100%** | **1,000,000,000** | — |

These percentages MUST NOT be treated as final merely because they sum to 100%. The final model must show why each bucket is needed, who controls it, when it unlocks, and how it affects circulating supply.

### 5.3 Reward Schedule

From Spec §5.3 — proposed initial network-incentive schedule:

| Year | OMNIA Issued | Cumulative from Incentive Pool |
|------|-------------:|-------------------------------:|
| Year 1 | 80,000,000 | 80,000,000 (20% of 400M) |
| Year 2 | 60,000,000 | 140,000,000 (35%) |
| Year 3 | 45,000,000 | 185,000,000 (46.25%) |
| Year 4 | 34,000,000 | 219,000,000 (54.75%) |

First four years consume 219,000,000 OMNIA (54.75% of the 400M incentive pool). The remaining 181,000,000 MUST have a fully specified schedule before the reward pool is activated.

**The final emission specification MUST define** (Spec §5.3):

- Epoch or block reward
- Start and end of every era
- Whether emission decreases by era or by a halving schedule
- Treatment of unclaimed rewards
- Treatment of inactive or slashed validators
- Maximum reward authority
- Whether governance can change emissions
- Required notice and timelock for changes
- Relationship between validator rewards, ecosystem grants, and treasury spending

No reward authority may mint outside the hard cap.

### 5.4 Pilot Acquisition Model

The first closed Ghana mobile-money pilot MUST use a **capped treasury allocation**, not automatic new issuance (Spec §5.4):

```text
GHS payment → verified payment order → reserve from approved OMNIA pilot inventory → allocate OMNIA
```

Pilot inventory MUST be a separately tracked sub-allocation with:

- Fixed maximum amount
- Approved treasury wallet(s)
- Daily and monthly limits
- Price and quote policy
- End date or review date
- Pause conditions
- Public or auditor-accessible reporting
- Documented policy for replenishment or closure

No automatic minting may occur merely because mobile-money demand exceeds available inventory. Future issuance requires a separate monetary-policy decision, legal review, and governance approval.

---

## 6. Fees and Burn Policy

### 6.1 Fee Separation

UBC and OMNIA fees MUST remain distinct (Spec §7.1):

| Activity | Initial Policy |
|----------|---------------|
| Basic identity and participation | UBC allowance or sponsored protocol quota |
| Basic compute access | UBC-based per existing economics |
| Native OMNIA transfer | OMNIA fee path (to be introduced, bounded) |
| Optional priority inclusion | OMNIA priority fee |
| Ghana mobile-money payment | Provider fee + transparently disclosed Omnia charge |
| Merchant payment | OMNIA network fee, subject to pilot limits |
| External-chain transaction | External chain's fee asset or explicit conversion |
| Governance proposal | OMNIA deposit or fee (after governance implemented) |

OMNIA MUST NOT become mandatory for basic participation unless a future governance decision explicitly changes that policy after evidence and review.

### 6.2 OMNIA Fee Formula

From Spec §7.2:

```text
user_fee = base_fee + priority_fee + applicable_service_fee
burned_amount = base_fee × burn_ratio
validator_amount = priority_fee + permitted validator share
protocol_amount = permitted treasury or operational share
```
The exact formula MUST include maximums, minimums, rounding rules, fee-exemption rules, and behavior during congestion.

### 6.3 Burn Policy

From Spec §7.3 — conservative initial approach:

| Parameter | Value |
|-----------|-------|
| **Initial burn ratio** | 0–5% of the OMNIA base-fee component |
| **Initial governance ceiling** | 10–25%, subject to final approval |

The initial ratio MUST NOT be presented as a guarantee of appreciation. It is an accounting policy that removes supply when actual OMNIA fee activity occurs.

**Burn policy requirements:**

- UBC MUST NOT be burned as OMNIA.
- External-chain fees MUST NOT be misrepresented as OMNIA burns.
- Every burn MUST emit an event.
- The supply API MUST show total minted, total burned, and current supply.
- Increases require usage data, a public proposal, and a timelock.
- Governance MUST NOT exceed the hard-coded ceiling without a separately reviewed protocol upgrade.

---

## 7. Issuance Authority

From Spec §6.1 — OMNIA issuance MUST be divided into clearly defined authorities:

| Authority | Permitted Action |
|-----------|-----------------|
| **Genesis authority** | Establishes the approved initial supply and allocation |
| **Treasury allocation authority** | Transfers already-issued OMNIA from approved treasury inventory |
| **Reward authority** | Releases only the approved reward budget |
| **Governance authority** | Changes bounded policy parameters after timelock, not unlimited supply |
| **External adapter authority** | No native OMNIA minting authority |

A service that verifies a payment MUST NOT automatically gain unlimited mint authority.

---

## 8. Floating Value & Redemption

OMNIA launches as a **floating asset** (Spec §10):

- GHS buys a quoted quantity of OMNIA.
- The quote is valid only for the stated period.
- The quantity delivered may vary with market conditions and fees.
- Omnia makes no fixed-value or fixed-redemption promise.
- Merchants may price goods in GHS and convert at checkout.
- Future redemption requires a separate reserve, operational, and legal design.

A future fixed-redemption or GHS-linked product MUST NOT be introduced by simply adding a buyback button. It would require defined reserves, eligibility, redemption timing, liquidity, accounting, disclosures, and regulatory review.

---

## 9. Staking

Staking is a **future OMNIA workstream, not a launch assumption** (Spec §11). Before OMNIA is marketed as security collateral, the protocol MUST specify validator eligibility, active-set size, stake/delegation model, selection algorithm, concentration limits, bonding/unbonding, key rotation, downtime/double-signing evidence, slashing formula, appeals, reward calculation, attack-cost model, validator diversity targets, and independent security review.

Dollar-denominated minimums (e.g., fixed `$50,000` stake) MUST NOT be hard-coded. Thresholds SHOULD be derived from protocol units, attack cost, required validator diversity, and liquidity analysis.

Liquid staking is deferred until base staking is proven, audited, and reviewed for systemic risk.

---

## 10. Governance

Governance MAY eventually control bounded protocol parameters, treasury spending, grants, fee ratios, and future issuance policies. Governance MUST NOT have an unbounded ability to mint OMNIA (Spec §12).

Required controls: proposal deposit/spam prevention, snapshot timing, quorum/approval threshold, delegation, delegation cooldown, flash-governance resistance, public proposal rationale and code diff, 48–72 hour minimum execution timelock for ordinary changes, longer delay for supply/issuance/redemption changes, emergency pause with no silent balance alteration, emergency authority expiry, post-incident reporting.

Supply, treasury, and redemption decisions MUST be separated from ordinary parameter changes and require stronger evidence and approval.

---

## 11. Merchant Payments

Merchants price goods in GHS and convert to OMNIA at checkout via time-limited quote (Spec §9):

```text
merchant enters GHS price
  → Omnia generates time-limited OMNIA quote
  → customer scans QR or opens payment request
  → customer authorizes signed OMNIA transfer
  → merchant receives confirmed payment
  → receipt and reconciliation record created
```

The first pilot MUST NOT promise merchants that OMNIA can always be converted to GHS. Merchant GHS exit models (supplier network, MoMo-out partner, treasury buyback, external liquidity) all remain available for later approval but are NOT guaranteed in the first pilot.

---

## 12. Security Invariants

Highest-priority property tests from Spec §16:

```text
UBC cannot become OMNIA
OMNIA cannot become another asset
external adapters cannot mint OMNIA
payment callbacks cannot bypass verification
duplicate callbacks cannot duplicate allocation
refunds cannot leave delivered balances outstanding
supply cannot exceed the hard cap
```

Additional invariants from Spec §4.4:

```text
no operation can move one asset as another asset
no UBC operation can create a transferable OMNIA balance
no external-chain adapter can invoke native OMNIA minting
no payment callback can create a balance without verified order state
no order can allocate more OMNIA than its reserved inventory
no failed or refunded order can remain economically delivered
```

---

## 13. Launch Gates (from Spec §17)

| Gate | Requirement |
|------|------------|
| **Gate 0** | Reproducible baseline (repo, release automation, build, deploy, 5-node testnet, monitoring, rollback) |
| **Gate 1** | Financial specification approved internally (OMNIA name, symbol, decimals, floating behavior, pilot inventory, treasury policy, asset registry, supply model, fee boundary) |
| **Gate 2** | Asset-aware protocol (registry, asset-scoped balances, supply events, migrations, invariants pass testnet and adversarial testing) |
| **Gate 3** | Payment core (payment service, provider adapter, quote, webhook verification, order states, refunds, reconciliation, failure matrix pass in sandbox) |
| **Gate 4** | Five-node testnet validation (complete simulated mobile-money flow, node restart, duplicate callbacks, provider failures, refunds, zero unexplained reconciliation differences) |
| **Gate 5** | Wallet staging (buy treasury-allocated OMNIA, accurate status/fees, failure recovery, receipt, send to another wallet) |
| **Gate 6** | Merchant pilot (onboard, QR payments, reconciliation, refunds, support; no GHS exit promise unless separately approved) |
| **Gate 7** | Ghana controlled beta (product role, payment partner, customer limits, KYC/AML, disclosures, data processing, complaints path, regulator/partner approvals documented) |
| **Gate 8** | Public readiness (security review, treasury reporting, liquidity policy, support operations, incident response, provider contracts, financial reconciliation, legal classification) |

---

## 14. Deferred Capabilities

From Spec §18 — these capabilities MUST NOT be implemented until their stated prerequisites are complete:

| Capability | Reconsider Only When |
|-----------|---------------------|
| OMNIA staking | Validator economics, attack-cost model, selection, slashing, independent security review complete |
| Liquid staking | Base staking proven, derivative contracts audited, systemic risk approved |
| Public exchange venues | Legal classification, security, liquidity, treasury disclosure, real merchant circulation established |
| BTC wallet support | Production-grade adapter, custody, recovery, finality, security review complete |
| Automatic OMNIA issuance | Pilot demand, inventory model, legal review, governance policy justify it |
| Fixed GHS redemption | Reserves, liquidity, redemption operations, disclosures, authorization complete |
| Broad multi-asset settlement | Asset registry, adapter framework, reconciliation, compliance controls mature |

---

## 15. Final Baseline Decisions

From Spec §19 — the agreed baseline for implementation:

| Decision | Baseline |
|----------|---------|
| UBC | Non-transferable participation and compute allowance |
| OMNIA | Native transferable economic asset |
| Basic access | UBC or sponsored quota, not mandatory OMNIA purchase |
| Asset model | Explicit asset registry and asset-scoped balances |
| Working hard cap | 1,000,000,000 OMNIA, subject to final genesis reconciliation |
| Pilot acquisition | Treasury allocation from capped inventory |
| Pilot minting | No automatic new minting |
| Initial value behavior | Floating OMNIA; no fixed GHS redemption promise |
| Initial burn | Small bounded OMNIA base-fee burn, initially 0–5% target range |
| Staking | Deferred separate workstream |
| Liquid staking | Deferred |
| Merchant pricing | GHS display with time-limited OMNIA quote |
| Merchant GHS exit | Not guaranteed in first pilot |
| External assets | Separate chain-specific assets and adapters |
| Public sale | Deferred until classification and controls resolved |
| Exchange listings | Deferred until operational, legal, security, and usage gates pass |
# Ghana Regulatory Classification & Partner Diligence Questions

> **Status**: OPEN — Requires Legal Counsel Input
> **Last Updated**: 2026-08-15
> **Owner**: Compliance Lead
> **Spec Reference**: Financial Specification §1, §8.1, §17 Gates 7–8
> **Classification**: Confidential — Legal Work Product
> **Deadline**: Must be resolved before Gate 7 (Ghana controlled beta)

---

## 1. Purpose

This document captures every open regulatory and partner-related question that must be answered before Omnia Protocol can legally operate the MoMo bridge in Ghana. Per the Financial Specification §1, BoG's virtual-asset materials identify wallet providers, virtual-asset issuers, dealing services, tokenization services, and fintech innovators using virtual assets as requiring regulatory consideration. The financial design treats compliance classification as an input to the product, not a post-launch footnote.

Per Spec §8.1, BoG describes its regulatory sandbox as a supervised environment for testing innovative financial products and business models. A sandbox may be a suitable controlled-testing path, but it is not itself authorization for public operation.

---

## 2. Regulatory Classification

### 2.1 Bank of Ghana (BoG)

| # | Question | Risk | Blocks | Source |
|---|----------|------|--------|--------|
| 1 | Is OMNIA classified as "electronic money" under the **Payment Systems Act, 2019 (Act 987)**? If so, Omnia Labs must obtain an E-Money Issuer license. | **Critical** | Bridge operations, mainnet | Spec §1, BoG virtual-assets page |
| 2 | Does the MoMo bridge constitute a "payment service provider" under the **Payment Systems Act**? | **Critical** | Bridge integration | Spec §1 |
| 3 | Is OMNIA classified as a "security" under the **Securities Industry Act, 2016 (Act 929)**? | **Critical** | Token distribution, on-ramp, listings | Spec §1 |
| 4 | Does the staking mechanism constitute an "investment scheme" under Act 929? | **High** | Staking design (deferred but needs answer) | Spec §11 |
| 5 | Is the MoMo bridge subject to the **Foreign Exchange Act, 2006 (Act 723)**? | **High** | Bridge operations, OMNIA pricing | Spec §1 |
| 6 | What are the **AML/CFT** obligations for the bridge operator? Per Spec §16, the system MUST include authenticated provider callbacks, server-side payment verification, and audit logs. | **High** | Bridge on-ramp, wallet KYC | Anti-Money Laundering Act 2020 (Act 1044), Spec §16 |
| 7 | Does BoG require **sandbox** participation before live operations? Per Spec §8.1, a sandbox may be suitable but is not authorization. | **Medium** | Launch timeline | Spec §8.1, BoG sandbox FAQ |
| 8 | **Data protection** obligations under the **Data Protection Act, 2012 (Act 843)** for KYC data, transaction history, wallet addresses? | **Medium** | Wallet design, data retention | Spec §16 |

### 2.2 Securities and Exchange Commission (SEC) Ghana

| # | Question | Risk | Blocks | Source |
|---|----------|------|--------|--------|
| 9 | Has SEC Ghana issued binding regulation addressing blockchain tokens or digital assets? | **High** | All token design | Spec §1, SEC VASP Bill |
| 10 | If OMNIA is a security, what is the registration process, timeline, cost? Are there utility-token exemptions? | **High** | Token launch | Spec §19 (public sale deferred) |
| 11 | Does the testnet airdrop (vested, non-transferable until mainnet) constitute a "public offer"? | **Medium** | Testnet allocation | Spec §5.2 |

### 2.3 Ghana Revenue Authority (GRA)

| # | Question | Risk | Blocks | Source |
|---|----------|------|--------|--------|
| 12 | Is there a tax obligation on OMNIA transactions? Is GHS→OMNIA a taxable event? Are staking rewards taxable income? | **Medium** | Treasury accounting, user disclosures | Spec §6.3 |
| 13 | Does the protocol need to withhold tax on bridge transactions? | **Medium** | Fee model, bridge pricing | Spec §7.1 |

---

## 3. Mobile Money Partner Questions

### 3.1 MTN Mobile Money (MoMo)

| # | Question | Risk | Blocks | Source |
|---|----------|------|--------|--------|
| 14 | What is MTN MoMo's **API access process** for third-party payment providers? | **Critical** | All MTN integration | Spec §8 |
| 15 | Does MTN require a **partnership agreement / MoU** before production API access? What are commercial terms? Per Spec §8.5, provider fees MUST be obtained through a current commercial quote. | **Critical** | Bridge economics, launch | Spec §8.5 |
| 16 | What are MTN's **transaction limits** for third-party API integrations? (per-transaction, daily, monthly) | **High** | Bridge capacity, UX | Spec §15 (per-order GHS limit) |
| 17 | Does MTN support **disbursements (push payments)** via API? The bridge needs both collection and disbursement. | **Critical** | Bridge architecture | Spec §9.3 (MoMo-out partner) |
| 18 | What is MTN's **settlement cycle** for API partners? (T+0, T+1, T+2?) | **High** | Liquidity sizing | Spec §6.3 |
| 19 | Does MTN provide **webhook/callback** notifications? Reliability and retry mechanism? | **Medium** | State machine design (PAYMENT_PENDING→PAYMENT_VERIFIED) | Spec §8.3 |

### 3.2 Telecel Cash (formerly Vodafone Cash)

| # | Question | Risk | Blocks |
|---|----------|------|--------|
| 20 | Same questions (14–19) for Telecel Cash. Does it have a comparable developer API? | **Critical** | Telecel integration |

### 3.3 AT Money (AirtelTigo Money)

| # | Question | Risk | Blocks |
|---|----------|------|--------|
| 21 | Same questions (14–19) for AT Money. Is API accessible to third parties? | **High** | AT integration |

---

## 4. Operational & Commercial

| # | Question | Risk | Blocks | Source |
|---|----------|------|--------|--------|
| 22 | What **corporate entity** should operate the bridge? Local subsidiary required? | **High** | Corporate structure, partner agreements | Spec §17 Gate 7 |
| 23 | What **minimum paid-up capital** for a PSP license under BoG regulations? | **High** | Fundraising, treasury | Spec §6.2 |
| 24 | **Local data residency** requirements? Must KYC/transaction data be stored in Ghana? | **Medium** | Infrastructure | Spec §16 |
| 25 | **Insurance or guarantee** requirements for entities handling consumer funds? | **Medium** | Treasury reserves | Spec §6.2 |
| 26 | Is there a **sandbox or pilot program** we can participate in? | **Low** | Launch timeline | Spec §8.1 |

---

## 5. Priority Matrix

| Priority | Question #s | Target | Gate |
|----------|-------------|--------|------|
| **P0 — Blocking** | 1, 2, 3, 14, 15, 17, 22 | Phase 1 Week 4 | Gate 3 |
| **P1 — High** | 4, 5, 6, 9, 16, 18, 23 | Phase 1 Week 6 | Gate 4 |
| **P2 — Important** | 7, 8, 10, 11, 12, 13, 19, 20, 21, 24, 25 | Phase 2 Week 2 | Gate 5 |
| **P3 — Nice to Have** | 26 | Phase 2 Week 4 | Gate 6 |

---

## 6. Regulatory Scenarios

### Scenario A: OMNIA is E-Money (BoG License Required)
Ghanaian entity, paid-up capital, AML/CFT program, fit-and-proper board criteria. Timeline: 6–18 months.

### Scenario B: OMNIA is a Security (SEC Registration Required)
Prospectus, SEC review, ongoing disclosure. Timeline: 3–12 months.

### Scenario C: OMNIA is Unclassified (Most Likely)
Token itself unregulated, but bridge operation likely requires PSP license. This aligns with comparable jurisdictions (Nigeria, Kenya, South Africa).

### Scenario D: Full Regulatory Ban
Ghana follows restrictive stance. Operations relocate; Ghana launch postponed.

---

## 7. Decision Log

| Date | Question | Answer | Source | Impact |
|------|----------|--------|--------|--------|
| — | (No questions resolved yet) | — | — | — |

---

## 8. References

| Act | Year | Relevance |
|-----|------|----------|
| Payment Systems Act | 2019 (Act 987) | Payment systems, e-money, PSPs |
| Securities Industry Act | 2016 (Act 929) | Securities offering and trading |
| Anti-Money Laundering Act | 2020 (Act 1044) | AML/CFT obligations |
| Data Protection Act | 2012 (Act 843) | Personal data collection and storage |
| Foreign Exchange Act | 2006 (Act 723) | Foreign exchange transactions |
| Banks and SDTI Act | 2016 (Act 930) | Deposit-taking definitions |
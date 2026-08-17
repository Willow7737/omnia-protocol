# Treasury Inventory and Multisig Policy

> **Status**: DRAFT — Aligned to Financial Specification v1.0-draft
> **Last Updated**: 2026-08-15
> **Owner**: Governance Lead
> **Spec Reference**: Financial Specification §6
> **Classification**: Internal — Pre-Mainnet

---

## 1. Purpose

This policy governs the multi-signature controls and on-chain treasury mechanisms for Omnia Protocol's pooled funds. Per the Financial Specification §6.2, treasury assets MUST be held under multisignature or equivalent separation-of-duties controls.

The person who controls treasury inventory MUST NOT be the sole person approving software changes or payment reconciliation that moves that inventory (Spec §6.2).

---

## 2. Issuance Authority

Per Spec §6.1, OMNIA issuance is divided into clearly defined authorities:

| Authority | Permitted Action |
|-----------|------------------|
| **Genesis authority** | Establishes the approved initial supply and allocation |
| **Treasury allocation authority** | Transfers already-issued OMNIA from approved treasury inventory |
| **Reward authority** | Releases only the approved reward budget |
| **Governance authority** | Changes bounded policy parameters after timelock, not unlimited supply |
| **External adapter authority** | No native OMNIA minting authority |

A service that verifies a payment MUST NOT automatically gain unlimited mint authority.

---

## 3. Treasury Controls

Per Spec §6.2, the treasury policy MUST define:

- [x] Signatory roles and quorum (defined below)
- [x] Maximum transaction amount (defined below)
- [x] Daily and monthly spending limits (defined below)
- [x] Inventory reservation limits (defined below)
- [x] Approved counterparties (defined below)
- [x] Emergency pause authority (defined below)
- [x] Reconciliation frequency (defined below)
- [x] Public reporting frequency (defined below)
- [x] Key rotation and loss recovery (defined below)
- [x] Related-party transaction disclosure (defined below)
- [x] Incident escalation (defined below)

---

## 4. Off-Chain Multisig Configuration

| Parameter | Value |
|-----------|-------|
| **Platform** | Gnosis Safe (Ethereum L1) or Substrate Multisig pallet |
| **Threshold** | 3 of 5 signers |
| **Network** | Ethereum mainnet (for USDC/USDT) + Omnia mainnet (for OMNIA) |
| **Fallback** | 7-of-9 emergency recovery (time-locked 72 hours) |

### Keyholders

| Key # | Role | Organization | Replacement Process |
|--------|------|--------------|---------------------|
| 1 | Protocol Founder | Omnia Labs | Board vote + multisig rotation |
| 2 | CTO / Lead Architect | Omnia Labs | Board vote + multisig rotation |
| 3 | Finance / Operations | Omnia Labs | Board vote + multisig rotation |
| 4 | Independent Director | External | Community governance nomination |
| 5 | Legal / Compliance Counsel | External firm | Board vote + legal review |

### Keyholder Requirements

- Each keyholder MUST use a hardware wallet. Software wallets are NOT permitted for treasury keys.
- Each keyholder MUST store recovery seed in a physically separate, geographically distributed location.
- Keyholders MUST sign within 48 hours of receiving a transaction proposal.
- If a keyholder is unreachable for 7+ days, remaining signers may initiate key rotation.
- No two keyholders may share the same physical location for seed storage.

### Transaction Limits

| Amount (USD equiv.) | Approval Required | Time Lock |
|---------------------|-------------------|----------|
| < $1,000 | 3-of-5 multisig | None |
| $1,000 – $10,000 | 3-of-5 multisig | 24 hours |
| $10,000 – $100,000 | 4-of-5 multisig | 48 hours |
| $100,000 – $1,000,000 | 5-of-5 multisig | 72 hours |
| > $1,000,000 | 5-of-5 multisig + community referendum | 7 days |

---

## 5. Treasury Accounting

Per Spec §6.3, treasury accounting MUST separately track:

- OMNIA held for pilot allocation
- OMNIA held for liquidity or settlement
- OMNIA held for ecosystem grants
- OMNIA held as operating reserve
- Locked and vested allocations
- Provider fee subsidies
- Refunds and reserved inventory
- Realized and unrealized conversion effects
- All external funds received and paid

No customer or treasury balance discrepancy may be silently written off. Every difference MUST have an owner, reason, status, and resolution record (Spec §14).

---

## 6. Pilot Inventory

Per Spec §5.4, the pilot inventory is a separately tracked sub-allocation with:

- Fixed maximum amount
- Approved treasury wallet(s)
- Daily and monthly limits
- Price and quote policy
- End date or review date
- Pause conditions
- Public or auditor-accessible reporting
- Documented policy for replenishment or closure

No automatic minting may occur merely because mobile-money demand exceeds available inventory.

---

## 7. On-Chain Treasury Pallet

| Parameter | Value | Notes |
|-----------|-------|-------|
| **Proposal Bond** | 100 OMNIA | Deposited by proposer, refunded if proposal passes |
| **Proposal Bond Burn** | 50% | Burned if proposal is rejected (anti-spam) |
| **Spend Period** | 24 days | Number of days between spend periods |
| **Burn** | 0% | Unspent rolls over; no auto-burn |

Governance MUST NOT have an unbounded ability to mint OMNIA (Spec §12). Supply, treasury, and redemption decisions MUST be separated from ordinary parameter changes and require stronger evidence and approval.

### Governance Controls

Per Spec §12:

- Proposal deposit or spam prevention
- Snapshot timing
- Quorum and approval threshold
- Delegation + delegation cooldown
- Flash-governance resistance
- Public proposal rationale and code diff
- 48–72 hour minimum execution timelock for ordinary changes
- Longer delay for supply, issuance, or redemption changes
- Emergency pause with no silent balance alteration
- Emergency authority expiry
- Post-incident reporting

---

## 8. Financial Reporting and Reconciliation

Per Spec §14, the system MUST maintain a double-entry-style operational ledger. At minimum, it must reconcile:

```text
provider payment records
↔ payment orders
↔ treasury inventory reservations
↔ OMNIA allocation events
↔ wallet balances
↔ merchant receipts
↔ refunds and reversals
```

### Daily Controls

Per Spec §14, daily controls SHOULD include:

- Provider-to-order reconciliation
- Order-to-allocation reconciliation
- Treasury inventory reconciliation
- Total minted and burned reconciliation
- Outstanding refund report
- Manual-review report
- Failed and uncertain on-chain allocation report
- Subsidy and provider-fee report
- Merchant settlement report
- Incident and exception sign-off

---

## 9. Emergency Procedures

### Emergency Pause

Per Spec §12, the emergency pause:

- MUST NOT silently alter any balance
- MUST have a defined expiry
- MUST be followed by post-incident reporting
- MUST pause new allocations without destroying existing balances or preventing users from viewing transaction history (Spec §15)

### Key Rotation

- Annual scheduled rotation (5-of-5 to approve)
- Emergency rotation if 2+ keyholders unavailable (7-of-9 threshold, 72-hour time lock)
- Compromised key: immediate removal, 24-hour fund sweep, 7-day incident report

### Incident Escalation

Per Spec §6.2:

1. Operator detects anomaly → documents in incident log
2. If treasury inventory at risk → trigger emergency pause
3. Notify all keyholders within 1 hour
4. Within 4 hours: assess scope, determine if funds are affected
5. Within 24 hours: initial incident report
6. Within 7 days: full incident report with root cause and remediation

---

## 10. Approved Counterparties

| Category | Approved Use | Approval Process |
|----------|-------------|------------------|
| MTN MoMo | Pilot payment provider | Partner agreement + compliance review |
| Telecel Cash | Pilot payment provider | Partner agreement + compliance review |
| AT Money | Pilot payment provider | Partner agreement + compliance review |
| Regulated exchanges | OMNIA liquidity (future) | Legal + compliance + governance approval |
| Grant recipients | Ecosystem grants | Governance proposal + milestone reporting |

Payment-provider fees MUST be obtained through a current commercial quote. Public documentation does not establish one universal fee for every Ghana method, volume, or contract (Spec §8.5).

---

## 11. Pre-Mainnet Checklist

- [ ] 5 keyholders identified, onboarded, hardware wallets provisioned
- [ ] Multisig deployed on testnet with 3-of-5 threshold
- [ ] Test transaction executed and verified by all keyholders
- [ ] Emergency recovery (7-of-9) configuration tested
- [ ] Key rotation procedure tested on testnet
- [ ] Treasury pallet configured with correct parameters in genesis
- [ ] Pilot inventory sub-allocation tracked separately
- [ ] Treasury accounting system reconciling all Spec §6.3 categories
- [ ] Daily reconciliation controls operational (Spec §14)
- [ ] Emergency pause tested
- [ ] Audit firm selected and engagement letter signed
- [ ] This policy reviewed and approved by legal counsel

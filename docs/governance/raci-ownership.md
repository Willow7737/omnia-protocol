# RACI Ownership Table

> **Status**: ACTIVE — Foundation Sprint Baseline
> **Last Updated**: 2026-08-15
> **Owner**: Project Lead
> **Spec Reference**: Financial Specification §20

---

## Legend

| Letter | Meaning | Description |
|--------|---------|-------------|
| **R** | Responsible | Does the work. Only one R per row. |
| **A** | Accountable | Owns the outcome. Approves the deliverable. Only one A per row. |
| **C** | Consulted | Provides input before/during the work. Two-way communication. |
| **I** | Informed | Notified of progress/outcome. One-way communication. |

## Roles

| Code | Name | Scope |
|------|------|-------|
| **PL** | Project Lead | Overall delivery, sprint planning, stakeholder management |
| **AL** | Architecture Lead | Protocol design, pallet architecture, ADRs, technical decisions |
| **EL** | Economics Lead | Token design, monetary policy, incentive modeling |
| **GL** | Governance Lead | Multisig policy, on-chain governance, compliance |
| **CL** | Compliance Lead | Regulatory classification, partner due diligence, Ghana legal |
| **RE** | Release Engineer | CI/CD, Docker builds, version management, monitoring |
| **BE** | Bridge Engineer | MoMo bridge integration, payment-order implementation, provider adapters |
| **FE** | Frontend Engineer | Wallet UI, dashboard, merchant tools |
| **SM** | Security & Audit | Code review, penetration testing, audit coordination |
| **OP** | Operations | Reconciliation, incident response, manual review, provider management |

---

## Foundation Sprint (Spec §20, items 1–8)

| Task / Deliverable | PL | AL | EL | GL | CL | RE | BE | FE | SM | OP |
|--------------------|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| 1. Reproducible protocol & testnet baseline | A | I | I | I | I | **R** | I | I | I | I |
| 2. OMNIA decision sheet | A | C | **R** | C | C | I | I | I | I | I |
| 3. Asset registry and asset-scoped balance foundation | A | **R** | C | I | I | C | I | I | C | I |
| 4. Treasury inventory and multisig policy | A | I | C | **R** | C | I | I | I | C | C |
| 5. Ghana classification and partner-diligence questions | A | I | I | C | **R** | I | I | I | I | I |
| 6. Payment-order state machine | A | **R** | I | I | I | I | C | I | C | C |
| 7. RACI ownership table | **R** | C | C | C | C | I | I | I | I | C |
| 8. Financial specification review | A | C | **R** | C | **R** | I | I | I | C | I |

---

## Code Work Sprint 1 (Spec §20, items 1–3)

| Task / Deliverable | PL | AL | EL | GL | CL | RE | BE | FE | SM | OP |
|--------------------|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| Asset registry pallet (AssetDefinition, registration, queries) | A | **R** | C | I | I | I | I | I | C | I |
| Asset-scoped balance model (balance[asset_id][account]) | A | **R** | I | I | I | I | I | I | C | I |
| Supply, issuance, burn events | A | **R** | C | I | I | I | I | I | C | I |
| Property tests for §4.4 invariants | A | C | I | I | I | I | I | I | **R** | I |
| Treasury allocation interface with hard limits | A | **R** | C | **R** | I | I | I | I | C | I |
| CI/CD for pallet tests | I | I | I | I | I | **R** | I | I | I | I |

---

## Code Work Sprint 2–3 (Spec §20, items 4–5)

| Task / Deliverable | PL | AL | EL | GL | CL | RE | BE | FE | SM | OP |
|--------------------|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| Payment-order service (24-state machine) | A | **R** | I | I | I | I | **R** | I | C | I |
| Normalized provider adapter (MTN/Telecel/AT) | A | C | I | I | C | I | **R** | I | C | I |
| Quote service (time-limited, all §8.4 fields) | A | I | C | I | I | I | **R** | C | I | I |
| Webhook verification (authenticated, idempotent) | A | I | I | I | I | I | **R** | I | **R** | I |
| Reconciliation system (Spec §14) | A | C | I | I | I | I | **R** | I | I | **R** |
| Refund system | A | C | I | I | I | I | **R** | I | C | **R** |
| Circuit breakers (Spec §15 limits) | A | **R** | I | I | I | I | C | I | C | **R** |

---

## Code Work Sprint 4–5 (Spec §20, items 6–7)

| Task / Deliverable | PL | AL | EL | GL | CL | RE | BE | FE | SM | OP |
|--------------------|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| Five-node testnet validation (full simulated flow) | A | **R** | I | I | I | **R** | **R** | I | **R** | **R** |
| Wallet Buy OMNIA flow | A | I | C | I | I | I | C | **R** | C | I |
| Wallet send/receive OMNIA | A | I | I | I | I | I | I | **R** | I | I |
| Wallet payment history (state machine viz) | A | I | I | I | I | I | C | **R** | I | I |
| Merchant pilot tools (QR, receipts, reconciliation) | A | I | I | I | I | I | C | **R** | I | **R** |

---

## Gate Approvals (Spec §17)

| Gate | Approver (A) | R |
|------|:-----------:|---|
| Gate 0: Reproducible baseline | PL | RE |
| Gate 1: Financial spec approved | PL | EL, CL |
| Gate 2: Asset-aware protocol | PL | AL, SM |
| Gate 3: Payment core | PL | BE, SM |
| Gate 4: Five-node testnet validation | PL | AL, BE, RE, SM, OP |
| Gate 5: Wallet staging | PL | FE |
| Gate 6: Merchant pilot | PL | FE, OP, BE |
| Gate 7: Ghana controlled beta | PL | CL, BE, OP |
| Gate 8: Public readiness | PL | All |

---

## Escalation Path

1. **R cannot proceed** (blocked): Escalate to A within 24 hours
2. **A cannot resolve**: Escalate to PL within 48 hours
3. **PL cannot resolve**: Full team standup
4. **Cross-gate conflict**: PL resolves with input from relevant A-role owners
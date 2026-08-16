# ADR-028: Payment Order State Machine

> **Status**: Accepted
> **Date**: 2026-08-15
> **Owner**: Architecture Lead
> **Supersedes**: None
> **Spec Reference**: Financial Specification §8.2, §8.3, §15

---

## Context

The Omnia Protocol's Ghana mobile-money bridge converts GHS payments into treasury-allocated OMNIA. Each payment follows a multi-step lifecycle that must be strictly ordered, auditable, and resilient to failures including duplicate callbacks, out-of-order events, provider timeouts, reversals, partial payments, and refunds.

The Financial Specification (§8.2) defines the canonical state machine. This ADR codifies it for implementation.

## Decision

Implement the payment order state machine as defined in Financial Specification §8.2, with the 9 happy-path states and 15 failure/recovery states.

### Happy Path States

```text
CREATED
  → QUOTED
  → PAYMENT_PENDING
  → PAYMENT_VERIFIED
  → RISK_REVIEW
  → RISK_APPROVED
  → INVENTORY_RESERVED
  → ALLOCATION_SUBMITTED
  → ALLOCATION_FINALIZED
  → DELIVERED
```

### Failure and Recovery States

```text
QUOTE_EXPIRED
PAYMENT_FAILED
PAYMENT_REVERSED
PAYMENT_UNDERPAID
PAYMENT_OVERPAID
PAYMENT_TIMEOUT
RISK_REJECTED
INVENTORY_UNAVAILABLE
ALLOCATION_FAILED
ON_CHAIN_TIMEOUT
ON_CHAIN_UNCERTAIN
REFUND_PENDING
REFUNDED
MANUAL_REVIEW
CANCELLED
```

### State Descriptions

#### Happy Path

| State | Description | Authorized By |
|-------|-------------|---------------|
| `CREATED` | Payment order created. No funds locked yet. | Sender / Wallet
| `QUOTED` | Time-limited OMNIA quote generated (rate, amount, fees, expiry). | System (quote service) |
| `PAYMENT_PENDING` | Waiting for mobile-money provider callback. | System (awaiting external event) |
| `PAYMENT_VERIFIED` | Provider callback received and independently verified server-side. | Backend (verification service) |
| `RISK_REVIEW` | Order queued for risk assessment. | System (risk engine) |
| `RISK_APPROVED` | Risk check passed. | System (risk engine) |
| `INVENTORY_RESERVED` | OMNIA reserved from treasury pilot inventory. | Treasury service |
| `ALLOCATION_SUBMITTED` | On-chain OMNIA allocation transaction submitted. | Backend (chain service) |
| `ALLOCATION_FINALIZED` | On-chain transaction finalized (confirmed in block). | System (chain confirmation) |
| `DELIVERED` | OMNIA delivered to recipient wallet. Terminal state. | System (delivery service) |

#### Failure States

| State | Description | Recovery |
|-------|-------------|----------|
| `QUOTE_EXPIRED` | Quote timed out before payment. | New quote needed → back to `CREATED` or `CANCELLED` |
| `PAYMENT_FAILED` | Provider reported payment failure. | → `REFUND_PENDING` (if funds held) or `CANCELLED` |
| `PAYMENT_REVERSED` | Provider reversed a previously successful payment. | → `REFUND_PENDING` |
| `PAYMENT_UNDERPAID` | Amount received less than quoted. | → `REFUND_PENDING` or `MANUAL_REVIEW` |
| `PAYMENT_OVERPAID` | Amount received more than quoted. | → `REFUND_PENDING` (excess) or `MANUAL_REVIEW` |
| `PAYMENT_TIMEOUT` | No provider callback within timeout. | → `MANUAL_REVIEW` or `CANCELLED` |
| `RISK_REJECTED` | Risk engine rejected the order. | → `REFUND_PENDING` (if funds held) or `CANCELLED` |
| `INVENTORY_UNAVAILABLE` | Insufficient treasury inventory to fulfill order. | → `REFUND_PENDING` or retry after replenishment |
| `ALLOCATION_FAILED` | On-chain allocation transaction failed. | → `REFUND_PENDING` or retry |
| `ON_CHAIN_TIMEOUT` | On-chain transaction not finalized within timeout. | → `ON_CHAIN_UNCERTAIN` or `MANUAL_REVIEW` |
| `ON_CHAIN_UNCERTAIN` | Transaction status ambiguous (may or may not have been included). | → `MANUAL_REVIEW` (requires manual reconciliation) |
| `REFUND_PENDING` | Refund initiated, awaiting processing. | → `REFUNDED` or `REFUND_FAILED` (retry) |
| `REFUNDED` | Funds returned to sender. Terminal state. | — |
| `MANUAL_REVIEW` | Requires human intervention. | → any appropriate state after review |
| `CANCELLED` | Order cancelled. Terminal state. | — |

### Valid Transition Matrix

```rust
ValidTransitions: {
    // Happy path
    (CREATED, QUOTED),
    (QUOTED, PAYMENT_PENDING),
    (PAYMENT_PENDING, PAYMENT_VERIFIED),
    (PAYMENT_VERIFIED, RISK_REVIEW),
    (RISK_REVIEW, RISK_APPROVED),
    (RISK_APPROVED, INVENTORY_RESERVED),
    (INVENTORY_RESERVED, ALLOCATION_SUBMITTED),
    (ALLOCATION_SUBMITTED, ALLOCATION_FINALIZED),
    (ALLOCATION_FINALIZED, DELIVERED),

    // Quote failure
    (CREATED, CANCELLED),
    (QUOTED, QUOTE_EXPIRED),
    (QUOTED, CANCELLED),

    // Payment failures
    (PAYMENT_PENDING, PAYMENT_FAILED),
    (PAYMENT_PENDING, PAYMENT_TIMEOUT),
    (PAYMENT_VERIFIED, PAYMENT_REVERSED),

    // Amount discrepancies
    (PAYMENT_VERIFIED, PAYMENT_UNDERPAID),
    (PAYMENT_VERIFIED, PAYMENT_OVERPAID),
    (PAYMENT_UNDERPAID, REFUND_PENDING),
    (PAYMENT_UNDERPAID, MANUAL_REVIEW),
    (PAYMENT_OVERPAID, REFUND_PENDING),
    (PAYMENT_OVERPAID, MANUAL_REVIEW),

    // Risk
    (RISK_REVIEW, RISK_REJECTED),
    (RISK_REJECTED, REFUND_PENDING),
    (RISK_REJECTED, CANCELLED),

    // Inventory
    (RISK_APPROVED, INVENTORY_UNAVAILABLE),
    (INVENTORY_UNAVAILABLE, REFUND_PENDING),
    (INVENTORY_UNAVAILABLE, CANCELLED),

    // Allocation failures
    (INVENTORY_RESERVED, ALLOCATION_FAILED),
    (ALLOCATION_SUBMITTED, ALLOCATION_FAILED),
    (ALLOCATION_FAILED, REFUND_PENDING),
    (ALLOCATION_FAILED, INVENTORY_RESERVED),  // retry

    // On-chain issues
    (ALLOCATION_SUBMITTED, ON_CHAIN_TIMEOUT),
    (ALLOCATION_SUBMITTED, ON_CHAIN_UNCERTAIN),
    (ON_CHAIN_TIMEOUT, MANUAL_REVIEW),
    (ON_CHAIN_UNCERTAIN, MANUAL_REVIEW),

    // Refund path
    (PAYMENT_FAILED, REFUND_PENDING),
    (PAYMENT_REVERSED, REFUND_PENDING),
    (PAYMENT_TIMEOUT, MANUAL_REVIEW),
    (REFUND_PENDING, REFUNDED),

    // Manual review exits
    (MANUAL_REVIEW, REFUND_PENDING),
    (MANUAL_REVIEW, CANCELLED),
    (MANUAL_REVIEW, PAYMENT_PENDING),  // retry after investigation
    (MANUAL_REVIEW, RISK_REVIEW),       // re-evaluate
    (MANUAL_REVIEW, INVENTORY_RESERVED), // retry after inventory replenished
}
```

### Order Requirements

Per Spec §8.3, every order MUST contain:

- Unique order ID
- Customer and recipient references
- Asset ID
- GHS amount
- OMNIA quantity
- Exchange rate and quote timestamp
- Quote expiration
- Provider reference
- Provider fee
- Omnia fee
- Recipient public key
- Inventory reservation reference
- Risk decision
- Payment and allocation status
- Refund status
- Immutable event history

### Idempotency and Verification

Per Spec §8.3:

- The client MUST NOT declare payment success.
- The provider event MUST be authenticated.
- The backend MUST independently verify the transaction before allocation.
- Duplicate callbacks, out-of-order events, provider timeouts, reversals, partial payments, and refunds MUST be handled idempotently.

### Quote and Disclosure

Per Spec §8.4, at checkout the wallet MUST display: GHS amount, OMNIA quantity, quoted rate, quote expiry, payment-provider fee, Omnia fee, any spread or price impact, estimated delivery time, floating-value disclosure, refund/failure policy, and destination address.

The product MUST NOT state or imply that OMNIA equals GHS or is guaranteed to retain its purchase value.

### Risk Limits and Circuit Breakers

Per Spec §15, before public operation the system MUST implement configurable limits for:

| Limit | Purpose |
|-------|----------|
| Per-order GHS limit | Limits payment and fraud exposure |
| Daily customer limit | Controls cumulative risk |
| Daily merchant limit | Controls business and settlement exposure |
| Treasury allocation limit | Prevents inventory drain |
| Provider exposure limit | Limits unreconciled payment risk |
| Manual-review threshold | Routes unusual orders to operations |
| Refund exposure limit | Prevents uncontrolled liability |
| Price movement tolerance | Pauses allocation when quotes become stale |
| On-chain pending timeout | Prevents indefinite uncertain delivery |
| Aggregate subsidy budget | Prevents unbounded acquisition spending |

Circuit breakers MUST be able to pause new allocations without destroying existing balances or preventing users from viewing transaction history.

## Consequences

### Positive

- **Complete failure coverage**: 15 failure/recovery states cover every identified failure mode from the Financial Spec, including the nuanced distinction between `PAYMENT_UNDERPAID` and `PAYMENT_OVERPAID`.
- **Idempotency by design**: The transition matrix prevents duplicate state changes. Terminal states are no-ops.
- **Manual review escape hatch**: `MANUAL_REVIEW` can transition to multiple recovery states, giving operations team flexibility without bypassing the state machine.
- **Audit trail**: Every transition emits an event with order_id, from/to states, and authorizing actor. The immutable event history (Spec §8.3) enables full reconstruction.
- **Circuit breaker integration**: Risk limits from Spec §15 are directly enforced by the state machine (e.g., `INVENTORY_UNAVAILABLE` when treasury allocation limit is hit).

### Negative

- **25 states total**: This is more complex than typical payment state machines. Justification: the Financial Spec requires distinct handling for underpaid/overpaid, uncertain on-chain states, and inventory unavailability — merging these would lose critical distinction for reconciliation and compliance.
- **Manual review bottleneck**: `MANUAL_REVIEW` is a human-in-the-loop state. At scale, this needs tooling (dashboard, auto-triage rules, SLA monitoring). This is an operations concern, not a protocol concern.
- **State explosion in tests**: 25 states with ~35 transitions means the test matrix is large. Property-based testing is essential.

## Implementation Plan

| Phase | Work | Duration |
|-------|------|----------|
| Phase 1 | Define `PaymentState` enum (25 variants), `PaymentOrder` struct with all Spec §8.3 fields | Sprint 1 Week 1 |
| Phase 1 | Implement transition matrix and `advance_state()` with per-transition authorization | Sprint 1 Week 1–2 |
| Phase 1 | Add timeout hooks for `QUOTE_EXPIRED`, `PAYMENT_TIMEOUT`, `ON_CHAIN_TIMEOUT` | Sprint 1 Week 2 |
| Phase 1 | Unit tests: every valid transition, every invalid transition, idempotency on terminal states | Sprint 1 Week 2–3 |
| Phase 1 | Property tests: no failed order remains economically delivered (Spec §4.4) | Sprint 1 Week 3 |
| Phase 2 | Provider adapter trait (normalized interface for MTN/Telecel/AT) | Sprint 2 |
| Phase 2 | Quote service (time-limited OMNIA quote with all §8.4 disclosure fields) | Sprint 2 |
| Phase 2 | Webhook verification service (authenticated callbacks, idempotent processing) | Sprint 2–3 |
| Phase 2 | Reconciliation system (Spec §14: provider↔order↔inventory↔allocation↔wallet) | Sprint 3 |
| Phase 3 | Circuit breaker integration with Spec §15 risk limits | Sprint 4 |

## Related

- Financial Specification §8 (Ghana mobile-money bridge)
- Financial Specification §8.3 (Order requirements)
- Financial Specification §15 (Risk limits and circuit breakers)
- ADR-027: Asset Registry (PaymentOrder references `AssetId`)
- `docs/governance/treasury-multisig-policy.md` (treasury inventory that feeds `INVENTORY_RESERVED`)
- `docs/compliance/ghana-partner-questions.md` (provider API access for `PAYMENT_PENDING` → `PAYMENT_VERIFIED`)
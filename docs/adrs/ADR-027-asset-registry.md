# ADR-027: Asset Registry and Asset-Scoped Balance Model

> **Status**: Accepted
> **Date**: 2026-08-15
> **Owner**: Architecture Lead
> **Supersedes**: None
> **Spec Reference**: Financial Specification §4

---

## Context

The current single-asset financial representation must evolve into an explicit asset-aware model before multi-asset wallet support is enabled (Spec §4 intro). The protocol needs to handle at least three distinct asset types:

1. **UBC** — non-transferable participation allowance, not money
2. **OMNIA** — native transferable economic asset
3. **External assets** (e.g., BTC) — separate chain-specific assets with their own semantics

Without an asset registry, these assets are referenced by ad-hoc string names or scattered enum variants across pallets. This creates mismatch risk, prevents runtime extensibility, and makes regulatory/compliance queries impossible.

## Decision

Implement an on-chain **Asset Registry** with a rich `AssetDefinition` struct and **asset-scoped balances** as the fundamental accounting model.

### AssetDefinition

Per Spec §4.1, the following semantics MUST exist (actual Rust representation may differ structurally but MUST cover these fields):

```rust
pub struct AssetDefinition {
    pub asset_id: AssetId,
    pub symbol: String,
    pub display_name: String,
    pub decimals: u8,
    pub asset_class: AssetClass,
    pub transferability: Transferability,
    pub issuer: IssuerId,
    pub mint_policy: MintPolicy,
    pub burn_policy: BurnPolicy,
    pub fee_policy: FeePolicy,
    pub chain_scope: ChainScope,
    pub status: AssetStatus,
}
```

### Asset Classes

Per Spec §4.2:

| Asset Class | Example | Transferable? | Issuance Authority |
|-------------|---------|:---:|-------------------|
| Participation allowance | UBC | No | Epoch/eligibility protocol |
| Native economic asset | OMNIA | Yes | Genesis, bounded treasury or governance policy |
| External settlement asset | BTC | Depends on custody model | External chain or qualified partner |
| Future payment unit | Undecided | TBD | Separate legally reviewed policy |

### Balance Model

Per Spec §4.3 — balances MUST be scoped by asset:

```text
balance[asset_id][account_id] = amount
```

The protocol MUST NOT use a single untyped amount field for multiple assets.

### Required Invariants

Per Spec §4.4 — these MUST be enforced by code and tested as property-based invariants:

```text
For every asset:
    total_supply = account_balances
                 + locked_balances
                 + treasury_balances
                 + escrow_balances

minted - burned = total_supply_delta

no operation can move one asset as another asset
no UBC operation can create a transferable OMNIA balance
no external-chain adapter can invoke native OMNIA minting
no payment callback can create a balance without verified order state
no order can allocate more OMNIA than its reserved inventory
no failed or refunded order can remain economically delivered
```

Every supply change MUST emit an auditable event containing: asset ID, amount, authority, reason, reference, timestamp, and resulting total supply.

### Well-Known Asset IDs

| AssetId | Symbol | Asset Class | Transferable | Issuer |
|---------|--------|-------------|:---:|--------|
| 0 | OMNIA | Native economic asset | Yes | Genesis authority |
| 1 | UBC | Participation allowance | No | Epoch/eligibility protocol |
| 2 | BTC | External settlement asset | Depends on custody | External chain |

### Genesis Registration

OMNIA (0) and UBC (1) are registered in genesis config. External assets (BTC, etc.) are registered via governance after their adapter achieves production readiness. No asset should be marketed as supported merely because an adapter stub exists (Spec §13).

### External Asset Requirements

Per Spec §13, for every external asset, Omnia MUST define:

- Supported chain and network
- Address creation and recovery
- Custody model
- Transaction construction
- Fee estimation
- Confirmation threshold
- Reorganization handling
- Provider or adapter outage behavior
- Reconciliation source
- User-facing asset status
- Legal and compliance classification

### Storage Layout

```rust
#[pallet::storage]
pub type AssetDefinitions<T: Config> = StorageMap<
    _,
    Identity,
    AssetId,
    AssetDefinition,
    OptionQuery,
>;

#[pallet::storage]
pub type NextAssetId<T: Config> = StorageValue<_, AssetId, ValueQuery>;
```

### Events

```rust
pub enum Event<T: Config> {
    AssetRegistered { id: AssetId, symbol: Vec<u8>, asset_class: AssetClass },
    AssetMetadataUpdated { id: AssetId },
    AssetFrozen { id: AssetId },
    AssetUnfrozen { id: AssetId },
    SupplyChanged { asset_id: AssetId, delta: Balance, reason: Vec<u8>, total_supply: Balance },
}
```

### Errors

```rust
pub enum Error<T: Config> {
    AssetNotFound,
    AssetAlreadyExists,
    AssetFrozen,
    UnauthorizedRegistration,
    InvalidDecimals,
    InvalidSymbol,
    InvalidAssetClass,
    InvariantViolation,
    SupplyExceedsHardCap,
}
```

### Extrinsics

| Extrinsic | Caller | Purpose |
|-----------|--------|----------|
| `register_asset(definition)` | Root / Governance | Register a new asset type with full `AssetDefinition` |
| `update_metadata(id, partial)` | Root / Governance | Modify asset metadata fields |
| `freeze_asset(id)` | Governance / Emergency | Freeze all transfers for an asset |
| `unfreeze_asset(id)` | Governance | Unfreeze an asset |

## Consequences

### Positive

- **Single source of truth**: Every pallet, RPC endpoint, and frontend component queries the same registry. Asset mismatches become impossible.
- **Runtime extensibility**: New currencies (NGN, KES, XOF) can be added via governance without a runtime upgrade.
- **Regulatory clarity**: BoG or partners can query the on-chain registry to see exactly which assets are supported and their properties.
- **Invariant enforcement**: Property tests on `total_supply = sum(balances)` and cross-asset contamination prevent entire classes of financial bugs.
- **Auditability**: Every supply change emits a structured event with asset ID, amount, authority, reason, and resulting total.

### Negative

- **Storage overhead**: Each `AssetDefinition` entry is ~200+ bytes with all fields. With 10 assets this is negligible; at 1000+ assets, a benchmark is needed.
- **Governance dependency**: Adding new assets requires a governance vote. For time-sensitive additions, a Technical Committee fast-track path may be needed (deferred to Phase 2).
- **Migration cost**: Pallets that currently reference assets directly must be refactored to query the registry. This is a one-time cost.

### Neutral

- The `AssetDefinition` struct is deliberately rich (11 fields). Some fields (e.g., `chain_scope`, `fee_policy`) may be sparsely populated for simple assets like OMNIA. This is acceptable — the struct is designed for the general case.
- The registry does NOT manage balances. It is metadata-only. Balance accounting remains in respective pallets, but all MUST be asset-scoped.

## Implementation Plan

| Phase | Work | Duration |
|-------|------|----------|
| Phase 1 | Define `AssetId`, `AssetClass`, `Transferability`, `MintPolicy`, `BurnPolicy`, `FeePolicy`, `ChainScope`, `AssetStatus` types | Sprint 1 Week 1 |
| Phase 1 | Implement `AssetDefinition` struct and `AssetRegistry` pallet with CRUD extrinsics | Sprint 1 Week 1–2 |
| Phase 1 | Register OMNIA and UBC in genesis config | Sprint 1 Week 2 |
| Phase 1 | Implement asset-scoped balance wrapper or integration with existing Balances pallet | Sprint 1 Week 2–3 |
| Phase 1 | Property-based tests for all §4.4 invariants | Sprint 1 Week 3 |
| Phase 1 | RPC endpoint `asset_registry_metadata(id)` and `asset_registry_supply(id)` | Sprint 1 Week 3 |
| Phase 2 | Refactor `PaymentOrder` pallet to use `AssetId` | Sprint 3 |
| Phase 2 | Integrate with external asset adapter framework | Sprint 4 |

## Related

- Financial Specification §4 (Asset registry and accounting model)
- ADR-028: Payment Order State Machine (uses AssetId for payment currencies)
- `docs/economics/omnia-coin.md` (token decisions derived from this registry)
- `economics/src/ubc.rs` (existing UBC implementation — must be reconciled with registry)
- `node/src/api/financial.rs` (existing financial API — must use registry)
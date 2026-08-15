# Omnia Protocol Financial Specification

## Final Draft for Engineering, Treasury, Operations, Governance, and Legal Review

**Status:** Final design baseline for implementation planning. Not yet a public offering document, legal opinion, investment prospectus, or authorization to operate a Ghanaian payment or virtual-asset service.

**Version:** 1.0-draft

**Scope:** UBC, OMNIA, asset accounting, supply and issuance, treasury, fees, burns, payment rails, merchant settlement, external assets, staking, governance, controls, and launch gates.

---

## 1. Purpose and design thesis

Omnia is designed as a **mobile-first digital-money and settlement network** with three deliberately separate financial layers:

1. **UBC** gives each eligible identity a basic, non-transferable right to participate in the network and consume defined compute or protocol resources.
2. **OMNIA** is the native transferable economic asset used for value transfer, merchant payments, future staking collateral, selected network fees, and bounded governance utility.
3. **External assets** such as Bitcoin are separate assets with their own chain, address, custody, fee, finality, and adapter semantics. They are not created by the native OMNIA monetary system.

The intended Ghana distribution loop is:

```text
GHS mobile money
    → verified payment order
    → treasury-held OMNIA allocation during the pilot
    → user wallet
    → peer transfer or merchant payment
    → optional future settlement or conversion
```

The project should initially present OMNIA as a **floating transferable digital asset accepted within the Omnia ecosystem**. It must not describe OMNIA as Ghanaian legal tender, a guaranteed GHS equivalent, a deposit, or a fixed-value redeemable instrument unless the necessary legal, reserve, operational, and regulatory structure exists.

Bank of Ghana’s virtual-asset materials identify wallet providers, virtual-asset issuers, dealing services, tokenization services, fintech innovators using virtual assets, and related activities as requiring regulatory consideration [1]. The financial design therefore treats compliance classification as an input to the product, not a post-launch footnote.

---

## 2. Normative language

The words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are used as implementation requirements.

| Term | Meaning |
|---|---|
| MUST | Required for conformance to this specification |
| MUST NOT | Prohibited by this specification |
| SHOULD | Strongly recommended; deviation requires a recorded reason |
| SHOULD NOT | Strongly discouraged; deviation requires approval |
| MAY | Optional implementation choice |

---

## 3. Financial-layer separation

### 3.1 UBC

UBC is the network’s **participation and compute allowance**. It is not the investment asset, payment currency, staking collateral, or transferable settlement asset.

UBC MUST remain:

- non-transferable between user accounts;
- associated with the eligibility and epoch rules already defined by the protocol;
- reset or replenished according to the protocol’s epoch mechanism;
- excluded from the OMNIA monetary supply;
- unavailable to external-chain adapters as a minting source;
- separately identified in wallet, API, accounting, and event schemas.

UBC MUST NOT be marketed as money or a tradable token.

### 3.2 OMNIA

OMNIA is the **native transferable economic asset**. It is the only native asset in this specification intended for ordinary value transfer and merchant payment.

At launch, OMNIA MAY support:

- wallet-to-wallet transfer;
- merchant payments;
- transparent transfer or priority fees;
- treasury allocation through the Ghana pilot bridge;
- governance deposits or voting utility after governance is implemented;
- staking collateral only after the validator-security workstream is complete;
- settlement inside the Omnia ecosystem.

OMNIA MUST NOT automatically be treated as:

- legal tender;
- a GHS stablecoin;
- a deposit or bank balance;
- a guaranteed-return investment;
- a fixed redemption claim;
- a substitute for a licensed payment institution;
- a representation of Bitcoin or another external asset.

### 3.3 External assets

Every external asset MUST have its own `AssetId`, chain identifier, address model, decimal precision, confirmation policy, fee asset, custody model, adapter status, and reconciliation process.

Bitcoin MUST NOT be represented as newly minted OMNIA. A Bitcoin adapter MAY record an external settlement reference, but it MUST NOT invoke the native OMNIA mint authority.

---

## 4. Asset registry and accounting model

The current single-asset financial representation must evolve into an explicit asset-aware model before multi-asset wallet support is enabled.

### 4.1 Required asset definition

```rust
struct AssetDefinition {
    asset_id: AssetId,
    symbol: String,
    display_name: String,
    decimals: u8,
    asset_class: AssetClass,
    transferability: Transferability,
    issuer: IssuerId,
    mint_policy: MintPolicy,
    burn_policy: BurnPolicy,
    fee_policy: FeePolicy,
    chain_scope: ChainScope,
    status: AssetStatus,
}
```

The actual Rust representation may differ, but the semantics MUST exist.

### 4.2 Asset classes

| Asset class | Example | Transferable? | Issuance authority |
|---|---|---:|---|
| Participation allowance | UBC | No | Epoch/eligibility protocol |
| Native economic asset | OMNIA | Yes | Genesis, bounded treasury or governance policy |
| External settlement asset | BTC | Depends on custody model | External chain or qualified partner |
| Future payment unit | Undecided | To be determined | Separate legally reviewed policy |

### 4.3 Balance model

Balances MUST be scoped by asset:

```text
balance[asset_id][account_id] = amount
```

The protocol MUST NOT use a single untyped amount field for multiple assets.

### 4.4 Required invariants

The following properties MUST be enforced by code and tested as property-based invariants:

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

Every supply change MUST emit an auditable event containing asset ID, amount, authority, reason, reference, timestamp, and resulting total supply.

---

## 5. OMNIA monetary policy

### 5.1 Working supply assumption

The current working model uses:

```text
Maximum OMNIA supply: 1,000,000,000 OMNIA
```

This is a **design assumption for modeling and implementation scaffolding**, not a public promise until the genesis configuration, allocation contracts, treasury policy, reward schedule, and governance limits have been reconciled and independently reviewed.

Once ratified, the hard cap MUST be enforced by a protocol invariant:

```text
circulating_supply
+ locked_supply
+ treasury_supply
+ escrow_supply
+ unissued_reward_budget
≤ hard_cap
```

Burns reduce total supply permanently. Unissued rewards MUST remain outside circulating supply and MUST not be counted as already minted.

### 5.2 Genesis allocation framework

The following allocation framework remains the working model and must be finalized through the economic model and governance process before genesis:

| Bucket | Working share | Working amount | Release principle |
|---|---:|---:|---|
| Network incentives | 40% | 400,000,000 | Decaying, bounded, performance-linked rewards |
| Team and contributors | 15% | 150,000,000 | Code-enforced vesting; four-year vest with one-year cliff as working assumption |
| Early investors/seed | 10% | 100,000,000 | Only if actual investors exist; subject to legal and disclosure review |
| Ecosystem fund | 15% | 150,000,000 | Milestone-based grants and partnerships |
| Treasury reserve | 10% | 100,000,000 | Multisig-controlled operations and contingency reserve |
| Liquidity and market operations | 10% | 100,000,000 | Transparent liquidity and settlement facility; no price guarantee |

These percentages MUST NOT be treated as final merely because they sum to 100%. The final model must show why each bucket is needed, who controls it, when it unlocks, and how it affects circulating supply.

### 5.3 Reward schedule

The proposed initial network-incentive schedule is:

```text
Year 1: 80,000,000 OMNIA
Year 2: 60,000,000 OMNIA
Year 3: 45,000,000 OMNIA
Year 4: 34,000,000 OMNIA
```

These first four years consume 219,000,000 OMNIA, or 54.75% of the working 400,000,000 incentive pool. The remaining 181,000,000 MUST have a fully specified schedule before the reward pool is activated.

The final emission specification MUST define:

- epoch or block reward;
- start and end of every era;
- whether emission decreases by era or by a halving schedule;
- treatment of unclaimed rewards;
- treatment of inactive or slashed validators;
- maximum reward authority;
- whether governance can change emissions;
- required notice and timelock for changes;
- relationship between validator rewards, ecosystem grants, and treasury spending.

No reward authority may mint outside the hard cap.

### 5.4 Pilot acquisition inventory

The first closed Ghana mobile-money pilot MUST use a **capped treasury allocation**, not automatic new issuance.

```text
GHS payment
    → verified payment order
    → reserve from approved OMNIA pilot inventory
    → allocate OMNIA
```

The pilot inventory MUST be a separately tracked sub-allocation with:

- a fixed maximum amount;
- approved treasury wallet(s);
- daily and monthly limits;
- price and quote policy;
- end date or review date;
- pause conditions;
- public or auditor-accessible reporting;
- a documented policy for replenishment or closure.

No automatic minting may occur merely because mobile-money demand exceeds available inventory. Future issuance requires a separate monetary-policy decision, legal review, and governance approval.

---

## 6. Issuance, treasury, and custody

### 6.1 Issuance authority

OMNIA issuance MUST be divided into clearly defined authorities:

| Authority | Permitted action |
|---|---|
| Genesis authority | Establishes the approved initial supply and allocation |
| Treasury allocation authority | Transfers already-issued OMNIA from approved treasury inventory |
| Reward authority | Releases only the approved reward budget |
| Governance authority | Changes bounded policy parameters after timelock, not unlimited supply |
| External adapter authority | No native OMNIA minting authority |

A service that verifies a payment MUST NOT automatically gain unlimited mint authority.

### 6.2 Treasury controls

Treasury assets MUST be held under multisignature or equivalent separation-of-duties controls. The treasury policy MUST define:

- signatory roles and quorum;
- maximum transaction amount;
- daily and monthly spending limits;
- inventory reservation limits;
- approved counterparties;
- emergency pause authority;
- reconciliation frequency;
- public reporting frequency;
- key rotation and loss recovery;
- related-party transaction disclosure;
- incident escalation.

The person who controls treasury inventory MUST NOT be the sole person approving software changes or payment reconciliation that moves that inventory.

### 6.3 Treasury accounting

Treasury accounting MUST separately track:

- OMNIA held for pilot allocation;
- OMNIA held for liquidity or settlement;
- OMNIA held for ecosystem grants;
- OMNIA held as operating reserve;
- locked and vested allocations;
- provider fee subsidies;
- refunds and reserved inventory;
- realized and unrealized conversion effects;
- all external funds received and paid.

---

## 7. Fees and burn policy

### 7.1 Fee separation

UBC and OMNIA fees MUST remain distinct.

| Activity | Initial policy |
|---|---|
| Basic identity and participation | UBC allowance or sponsored protocol quota |
| Basic compute access | UBC-based according to existing economics |
| Native OMNIA transfer | OMNIA fee path to be introduced and bounded |
| Optional priority inclusion | OMNIA priority fee |
| Ghana mobile-money payment | Provider fee plus transparently disclosed Omnia charge |
| Merchant payment | OMNIA network fee, subject to pilot limits |
| External-chain transaction | External chain’s fee asset or explicit conversion service |
| Governance proposal | OMNIA deposit or fee after governance is implemented |

OMNIA MUST NOT become mandatory for basic participation unless a future governance decision explicitly changes that policy after evidence and review.

### 7.2 OMNIA fee formula

The initial OMNIA fee model SHOULD separate:

```text
user_fee = base_fee + priority_fee + applicable_service_fee
burned_amount = base_fee × burn_ratio
validator_amount = priority_fee + permitted validator share
protocol_amount = permitted treasury or operational share
```

The exact formula MUST include maximums, minimums, rounding rules, fee-exemption rules, and behavior during congestion.

### 7.3 Burn policy

The protocol SHOULD implement burn accounting from the beginning but begin conservatively:

```text
initial burn ratio: 0–5% of the OMNIA base-fee component
initial governance ceiling: 10–25%, subject to final approval
```

The initial ratio MUST NOT be presented as a guarantee of appreciation. It is an accounting policy that removes supply when actual OMNIA fee activity occurs.

Burn policy requirements:

- UBC MUST NOT be burned as OMNIA;
- external-chain fees MUST NOT be misrepresented as OMNIA burns;
- every burn MUST emit an event;
- the supply API MUST show total minted, total burned, and current supply;
- increases require usage data, a public proposal, and a timelock;
- governance MUST NOT exceed the hard-coded ceiling without a separately reviewed protocol upgrade.

---

## 8. Ghana mobile-money bridge

### 8.1 Launch posture

The initial bridge is a **treasury-funded OMNIA acquisition service**. It is not an automatic minting service, fixed redemption promise, or general exchange for every supported cryptoasset.

A payment partner SHOULD handle the local payment interaction while Omnia controls order state, quote display, inventory reservation, allocation policy, and reconciliation. Bank of Ghana describes its regulatory sandbox as a supervised environment for testing innovative financial products and business models [2]. A sandbox may be a suitable controlled-testing path, but it is not itself authorization for public operation.

### 8.2 Payment order state machine

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

Failure and recovery states MUST include:

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

### 8.3 Order requirements

Every order MUST contain:

- unique order ID;
- customer and recipient references;
- asset ID;
- GHS amount;
- OMNIA quantity;
- exchange rate and quote timestamp;
- quote expiration;
- provider reference;
- provider fee;
- Omnia fee;
- recipient public key;
- inventory reservation reference;
- risk decision;
- payment and allocation status;
- refund status;
- immutable event history.

The client MUST NOT declare payment success. The provider event MUST be authenticated, and the backend MUST independently verify the transaction before allocation. Duplicate callbacks, out-of-order events, provider timeouts, reversals, partial payments, and refunds MUST be handled idempotently.

### 8.4 Quote and customer disclosure

At checkout, the wallet MUST display:

- GHS amount;
- OMNIA quantity;
- quoted rate;
- quote expiry;
- payment-provider fee;
- Omnia fee;
- any spread or price impact;
- estimated delivery time;
- floating-value disclosure;
- refund or failure policy;
- destination address.

The product MUST NOT state or imply that OMNIA equals GHS or is guaranteed to retain its purchase value.

### 8.5 Payment-provider economics

Payment-provider fees MUST be obtained through a current commercial quote. Public documentation does not establish one universal fee for every Ghana method, volume, or contract [3].

The pilot MAY subsidize provider fees from a capped treasury acquisition budget. The subsidy MUST be separately accounted for and MUST have a sunset or review condition.

The intended transition is:

```text
closed beta: treasury absorbs most/all cost within capped budget
expanded beta: transparent cost split
public operation: sustainable user-paid or merchant-paid pricing
```

---

## 9. Merchant payments and settlement

### 9.1 Merchant onboarding

Merchants are not ordinary wallet users. The pilot MUST define merchant identity, business category, settlement preference, support contact, limits, refunds, and risk tier.

A disclosed and capped treasury onboarding grant MAY be used for selected pilot merchants. It MUST have a purpose, amount cap, duration, milestones, and aggregate reporting. It MUST NOT be disguised price support or an undisclosed investment arrangement.

### 9.2 Merchant payment flow

During the floating-asset pilot, merchants SHOULD price goods in GHS and convert the requested amount into OMNIA at checkout:

```text
merchant enters GHS price
    → Omnia generates time-limited OMNIA quote
    → customer scans QR or opens payment request
    → customer authorizes signed OMNIA transfer
    → merchant receives confirmed payment
    → receipt and reconciliation record created
```

The merchant interface MUST show the GHS price, OMNIA amount, quote expiration, payment status, and receipt reference.

### 9.3 Merchant exit and GHS settlement

The first pilot MUST NOT promise a merchant that OMNIA can always be converted to GHS. The following models remain available for later approval:

| Model | Status |
|---|---|
| Merchant holds OMNIA | Available in pilot if merchant accepts volatility |
| Supplier network | Future circular-settlement option |
| MoMo-out partner | Requires licensed/qualified partner, liquidity, KYC/AML, and reconciliation |
| Treasury buyback | Requires reserve, pricing, policy, and legal review |
| External liquidity venue | Requires legal, custody, liquidity, and counterparty controls |

The pilot MUST measure merchant demand for GHS settlement before committing to a buyback or redemption mechanism.

---

## 10. Floating value and redemption

OMNIA launches as a **floating asset**.

At launch:

- GHS buys a quoted quantity of OMNIA;
- the quote is valid only for the stated period;
- the quantity delivered may vary with market conditions and fees;
- Omnia makes no fixed-value or fixed-redemption promise;
- merchants may price goods in GHS and convert at checkout;
- future redemption requires a separate reserve, operational, and legal design.

A future fixed-redemption or GHS-linked product MUST NOT be introduced by simply adding a buyback button. It would require defined reserves, eligibility, redemption timing, liquidity, accounting, disclosures, and regulatory review.

---

## 11. Staking and network security

Staking is a future OMNIA workstream, not a launch assumption.

Before OMNIA is marketed as security collateral, the protocol MUST specify:

- validator eligibility;
- active-set size;
- stake and delegation model;
- selection algorithm;
- concentration limits;
- bonding and unbonding;
- validator key rotation;
- downtime evidence;
- double-signing evidence;
- slashing formula and maximum;
- appeals and false-positive handling;
- reward calculation;
- treatment of slashed funds;
- attack-cost model;
- validator diversity targets;
- independent security review.

Dollar-denominated minimums such as a fixed `$50,000` stake MUST NOT be hard-coded. Thresholds SHOULD be derived from protocol units, attack cost, required validator diversity, and liquidity analysis.

Liquid staking is deferred until base staking is proven, audited, and reviewed for systemic risk. It is not required for the first OMNIA payment loop.

---

## 12. Governance and monetary control

Governance MAY eventually control bounded protocol parameters, treasury spending, grants, fee ratios, and future issuance policies. Governance MUST NOT have an unbounded ability to mint OMNIA.

Required governance controls include:

- proposal deposit or spam prevention;
- snapshot timing;
- quorum and approval threshold;
- delegation;
- delegation cooldown;
- flash-governance resistance;
- public proposal rationale and code diff;
- 48–72 hour minimum execution timelock for ordinary changes;
- longer delay for supply, issuance, or redemption changes;
- emergency pause with no silent balance alteration;
- emergency authority expiry;
- post-incident reporting.

Supply, treasury, and redemption decisions MUST be separated from ordinary parameter changes and require stronger evidence and approval.

---

## 13. External assets and adapters

The external-asset layer is separate from the OMNIA monetary system.

For every external asset, Omnia MUST define:

- supported chain and network;
- address creation and recovery;
- custody model;
- transaction construction;
- fee estimation;
- confirmation threshold;
- reorganization handling;
- provider or adapter outage behavior;
- reconciliation source;
- user-facing asset status;
- legal and compliance classification.

The current adapter inventory indicates that Bitcoin and several other integrations are not yet equivalent to a production retail wallet. No asset should be marketed as supported merely because an adapter stub exists.

---

## 14. Financial reporting and reconciliation

The system MUST maintain a double-entry-style operational ledger outside or alongside the protocol state for payment and treasury activity.

At minimum, it must reconcile:

```text
provider payment records
↔ payment orders
↔ treasury inventory reservations
↔ OMNIA allocation events
↔ wallet balances
↔ merchant receipts
↔ refunds and reversals
```

Daily controls SHOULD include:

- provider-to-order reconciliation;
- order-to-allocation reconciliation;
- treasury inventory reconciliation;
- total minted and burned reconciliation;
- outstanding refund report;
- manual-review report;
- failed and uncertain on-chain allocation report;
- subsidy and provider-fee report;
- merchant settlement report;
- incident and exception sign-off.

No customer or treasury balance discrepancy may be silently written off. Every difference MUST have an owner, reason, status, and resolution record.

---

## 15. Risk limits and circuit breakers

Before public operation, Omnia MUST implement configurable limits for:

| Limit | Purpose |
|---|---|
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

---

## 16. Security requirements

The financial system MUST include:

- authenticated provider callbacks;
- server-side payment verification;
- idempotency keys;
- replay protection;
- signed allocation requests;
- multisig treasury controls;
- strict separation of UBC and OMNIA code paths;
- asset-ID validation at every boundary;
- rate limits;
- audit logs;
- secrets isolation;
- key rotation;
- backup and recovery testing;
- fuzz and property tests for supply and transfer invariants;
- incident response and pause procedures;
- independent security review before public use.

The highest-priority property tests are:

```text
UBC cannot become OMNIA
OMNIA cannot become another asset
external adapters cannot mint OMNIA
payment callbacks cannot bypass verification
duplicate callbacks cannot duplicate allocation
refunds cannot leave delivered balances outstanding
supply cannot exceed the hard cap
```

---

## 17. Launch gates

### Gate 0: reproducible baseline

The repository, release automation, build, deployment, five-node testnet, monitoring, and rollback process are reproducible.

### Gate 1: financial specification

OMNIA name, symbol, decimals, floating behavior, pilot inventory, treasury policy, asset registry, supply model, and fee boundary are approved internally.

### Gate 2: asset-aware protocol

The registry, asset-scoped balances, supply events, migrations, and invariants pass testnet and adversarial testing.

### Gate 3: payment core

The payment service, provider adapter, quote, webhook verification, order states, refunds, reconciliation, and failure matrix pass in sandbox.

### Gate 4: five-node testnet validation

The complete simulated mobile-money flow works across the existing five-node testnet, including node restart, duplicate callbacks, provider failures, allocation failure, refunds, and zero unexplained reconciliation differences.

### Gate 5: wallet staging

A user can buy treasury-allocated OMNIA, see accurate status and fees, recover from failure, receive a receipt, and send OMNIA to another wallet.

### Gate 6: merchant pilot

Selected merchants can onboard, receive QR payments, reconcile receipts, handle refunds, and obtain support. No GHS exit promise exists unless separately approved.

### Gate 7: Ghana controlled beta

The exact product role, payment partner, customer limits, KYC/AML process, disclosures, data processing, complaints path, and regulator/partner approvals are documented.

### Gate 8: public readiness

Security review, treasury reporting, liquidity policy, support operations, incident response, provider contracts, financial reconciliation, and legal classification are complete.

---

## 18. Deferred capabilities and reconsideration gates

| Capability | Reconsider only when |
|---|---|
| OMNIA staking | Validator economics, attack-cost model, selection, slashing, and independent security review are complete |
| Liquid staking | Base staking is proven, derivative contracts are audited, and systemic risk is approved |
| Public exchange venues | Legal classification, security, liquidity, treasury disclosure, and real merchant circulation are established |
| BTC wallet support | Production-grade adapter, custody, recovery, finality, and security review are complete |
| Automatic OMNIA issuance | Pilot demand, inventory model, legal review, and governance policy justify it |
| Fixed GHS redemption | Reserves, liquidity, redemption operations, disclosures, and authorization are complete |
| Broad multi-asset settlement | Asset registry, adapter framework, reconciliation, and compliance controls are mature |

---

## 19. Final financial decisions

The following are the agreed baseline decisions for implementation:

| Decision | Final baseline |
|---|---|
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
| Public sale | Deferred until classification and controls are resolved |
| Exchange listings | Deferred until operational, legal, security, and usage gates pass |

---

## 20. Immediate implementation order

The first engineering and operations sprint MUST produce:

1. A reproducible protocol and testnet baseline.
2. The approved OMNIA decision sheet.
3. `docs/economics/omnia-coin.md`.
4. `docs/architecture/asset-registry.md`.
5. Treasury inventory and multisig policy.
6. Ghana classification and partner-diligence question list.
7. `docs/architecture/payment-order-state-machine.md`.
8. A RACI ownership table.

The first code work SHOULD then be:

1. Asset registry and asset-scoped balance foundation.
2. Supply, issuance, burn, and event invariants.
3. Treasury allocation interface with hard limits.
4. Payment-order service and normalized provider adapter.
5. Reconciliation and refund system.
6. Five-node testnet validation.
7. Wallet Buy OMNIA flow.
8. Merchant pilot tools.

The wallet purchase screen MUST NOT be the first implementation because it depends on the asset registry, treasury policy, payment state machine, and reconciliation contract.

---

## 21. References

[1]: https://www.bog.gov.gh/virtual-assets/ "Bank of Ghana — Virtual Assets"

[2]: https://www.bog.gov.gh/news/frequently-asked-questions-on-bog-regulatory-sandbox/ "Bank of Ghana — Regulatory Sandbox FAQ"

[3]: https://developer.flutterwave.com/v3.0/docs/ghana "Flutterwave — Ghana Mobile Money Documentation"

[4]: https://sec.gov.gh/press-release-passage-of-the-virtual-asset-service-providers-bill/ "Ghana Securities and Exchange Commission — Virtual Asset Service Providers Bill"

[5]: https://github.com/Willow7737/omnia-protocol/blob/main/economics/src/ubc.rs "Omnia UBC implementation"

[6]: https://github.com/Willow7737/omnia-protocol/blob/main/node/src/api/financial.rs "Omnia financial asset API"

[7]: https://github.com/Willow7737/Omnia-Wallet/blob/main/lib/features/send/send_screen.dart "Omnia Wallet asset-specific send flow"

[8]: https://github.com/Willow7737/omnia-protocol/blob/main/docs/stub-inventory.md "Omnia adapter implementation inventory"

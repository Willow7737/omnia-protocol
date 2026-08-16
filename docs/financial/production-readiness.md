# Omnia Financial Runtime Production Readiness

This runbook defines the controls required before the Ghana-first OMNIA acquisition and merchant-settlement path can be used with real customer funds. It does not alter the monetary policy: **UBC remains a non-transferable participation allowance, OMNIA remains the transferable native asset, the pilot is treasury-funded, and OMNIA has no fixed GHS redemption promise**.

## Runtime modes and secrets

Development and test nodes may use the in-memory payment store and deterministic sandbox credentials. A node started with `OMNIA_RUNTIME_MODE=production` fails closed unless all of the following are configured:

| Setting | Requirement |
|---|---|
| `OMNIA_PAYMENT_STORE_PATH` | Durable redb path on persistent storage with backups and restricted permissions |
| `OMNIA_QUOTE_SIGNING_SEED` | Secret seed managed outside source control and rotated under an approved key procedure |
| `OMNIA_GHANA_PROVIDER_SECRET` | Real provider callback secret, not the sandbox placeholder |
| `OMNIA_GHANA_PROVIDER_KEY` | Real provider/service credential, not the sandbox placeholder |
| `OMNIA_PAYMENT_WORKER_INTERVAL_MS` | Optional recovery-sweep interval; defaults to 30 seconds and must not be used as a substitute for provider SLAs |

Secrets must be injected by the deployment secret manager. They must not appear in READMEs, mobile builds, QR payloads, logs, or crash reports.

## Persistence and restart recovery

`RedbPaymentStore` persists order events, snapshots, and side-effect completion markers atomically. The node recovery worker enumerates active orders and replays the stored event history after restart. A recovery failure is logged as an operator-visible error; the worker never interprets a missing callback as success, never consumes treasury inventory by itself, and never credits a wallet without an authenticated chain-delivery operation.

Operators must back up the redb file using a consistent filesystem snapshot or a stopped-node copy. Restore drills must verify that an order cannot produce two provider refunds, two treasury consumptions, or two delivery notifications after restart.

## Provider operations

The sandbox adapter is suitable only for deterministic development and integration tests. A real Ghana provider adapter must be implemented against the provider's current authenticated API contract, including initiation, callback signature validation, replay protection, status polling, reversal handling, refund initiation, timeout behavior, and provider/reference/amount binding. The adapter must be tested with provider-supplied sandbox vectors before production credentials are enabled.

Provider callbacks remain the source of payment facts. The wallet may display a pending state, but it cannot mark an order successful. Every callback must be deduplicated by order, provider reference, and authenticated event identity.

## Reconciliation, refunds, and chain delivery

A production deployment must run a durable reconciliation process over provider records, payment-order events, treasury reservations, on-chain allocation records, and wallet delivery acknowledgements. Discrepancies must enter manual review rather than being silently repaired. Refund workers may initiate a provider refund only after the order state and reservation state authorize it, and refund initiation must be idempotent through the payment-store outbox.

The chain-delivery service must be a separately authenticated service role. It must submit the exact quoted net OMNIA quantity to the wallet settlement account, record the chain transaction and finality evidence, consume the matching treasury reservation exactly once, and advance the order only after finality and reconciliation checks succeed. It must never mint a new OMNIA balance as a shortcut around treasury inventory.

## Merchant controls

Merchant QR requests must contain a merchant identifier, expiry, GHS amount, quoted net OMNIA amount, settlement account, and request identifier. The wallet signs a transfer to the settlement account; it does not treat a DID or display label as an address. Merchant confirmation and receipt issuance must be performed by the settlement service, with a replay-safe request identifier and a durable audit record.

## Ghana compliance and operational sign-off

Before any public launch, the operator must obtain independent Ghana legal and compliance review covering licensing perimeter, customer identification, AML/CFT controls, sanctions screening, transaction monitoring, consumer disclosures, complaints, privacy, record retention, safeguarding, provider contracts, and merchant onboarding. The codebase can enforce technical gates and disclosures; it cannot substitute for regulatory approval, a qualified compliance officer, or a provider agreement.

A launch approval record should include the approved provider contract, secret-management evidence, backup-and-restore evidence, reconciliation drill results, refund drill results, incident contacts, monitoring dashboards, rate limits, and an explicit decision about whether the pilot is restricted to invited testers.

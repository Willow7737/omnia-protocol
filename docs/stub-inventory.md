# Stub & Partial Implementation Inventory

This document catalogs all stub, placeholder, and partial implementations in the Omnia Protocol codebase. Items are tracked so they are not mistaken for production-ready features.

> Last updated: 2026-08-16 (quote-backed financial path and merchant pilot interfaces integrated)

---

## Layer 3: Binding Layer

### RF Fingerprinting — **STUB** ⚠️

- **File**: `binding/src/rf_fingerprint.rs`
- **Status**: Stub — requires SDR hardware (HackRF/USRP) for production use
- **What exists**: Data model plus byte-array/Hamming-distance simulator. `RfFingerprint::stub` is compiled only for tests or the `test-utils` feature, and `verify` fails closed when `OMNIA_ENV=production`.
- **What's missing**: Real SDR integration, signal capture, fingerprint extraction, matching algorithm
- **Blocker**: Hardware dependency — requires physical SDR device
- **Phase**: Planned for Phase 1

---

## Layer 5: Economics

### Proof-of-Useful-Work — **PARTIAL** ⚠️

- **File**: `economics/src/useful_work.rs`
- **Status**: Partial — 3 work types defined, not production-ready
- **What exists**:
  - `UsefulWorkType` enum with 3 variants (ML training, scientific compute, distributed storage)
  - `UsefulWorkProof::verify` requires an Ed25519 verifier signature and rejects empty signatures in all builds
  - Basic reward calculation logic
  - `EconomicsShard::SubmitWork` remains admin-gated in production until real zkML/folding proof verification exists
- **What's missing**:
  - Real work verification (current signature only proves trusted verifier attestation, not the work itself)
  - Benchmark-based difficulty adjustment
  - Anti-Sybil measures for work submission
  - Integration with external compute providers
- **Phase**: Planned for Phase 2

---

## Phase 0: ZK-Rollup Settlement Adapters

> **Live adapters not tracked here**: The Ethereum live adapter (`ethereum-live` feature, `omnia-adapters/src/settlement/ethereum/live.rs`) and Bitcoin live adapter (`bitcoin-live` feature) are production-integrated and `is_live() == true`. This section only covers adapters that are still stubs, partial, or have unresolved caveats.

### Bitcoin Settlement Adapter — **LIVE** ✅ (with legacy stub coexisting)

- **File**: `omnia-adapters/src/settlement/bitcoin/live.rs` (live), `omnia-adapters/src/settlement/bitcoin/mod.rs` (legacy stub)
- **Feature flag**: `bitcoin-live`
- **Status**: Live — `BitcoinSettlementAdapter` implements `SettlementAdapter` against a real Bitcoin Core node via `bitcoincore-rpc`
- **What exists**:
  - `submit_root`: anchors state roots as OP_RETURN outputs (`OMNIA1` prefix + 32-byte root), using `createrawtransaction` → `fundrawtransaction` → `signrawtransactionwithwallet` → `sendrawtransaction`
  - `fetch_finality`: queries `gettransaction` for confirmation count and block height; rejects conflicted (negative confirmation) transactions
  - `is_live()` returns `true`
  - `BitcoinConfig::from_env()` reads `OMNIA_BITCOIN_RPC_URL`, `OMNIA_BITCOIN_RPC_USER`, `OMNIA_BITCOIN_RPC_PASSWORD`, `OMNIA_BITCOIN_MIN_CONFIRMATIONS`
  - Lazy connection via `OnceCell<Arc<Client>>`, blocking RPC calls on `spawn_blocking`
- **What's still missing**:
  - `verify_inclusion`: needs an OP_RETURN history scan to recover the last anchored root — fails closed for now
  - `submit_batch_with_proof`: intentionally not overridden (Bitcoin has no on-chain verifier; trait default fails closed)
- **Legacy stub**: The deprecated `BitcoinAdapter` (legacy `SettlementLayer`) still exists in `mod.rs` for backward compatibility — all methods return `NotImplemented` and it emits deprecation warnings
- **Phase**: ✅ Shipped

### Solana Settlement Adapter — **STUB** ⚠️

- **File**: `omnia-adapters/src/settlement/solana.rs`
- **Status**: Stub — legacy `SettlementLayer` only; all methods return `NotImplemented`; the adapter type is deprecated so new use emits compiler warnings
- **What exists**: Fail-closed legacy implementation returning `SettlementError::NotImplemented` for every operation
- **What's missing**: Solana RPC integration, program deployment, transaction submission
- **Phase**: Planned for Phase 1

### Celestia Settlement Adapter — **PARTIAL / UNVERIFIED** ⚠️

- **File**: `omnia-adapters/src/settlement/celestia.rs`
- **Status**: Partial — HTTP plumbing exists behind the `celestia` feature flag (mock mode when disabled), but it has **never been exercised against a real Celestia node**, and the adapter is **not instantiated anywhere** in the workspace (`CelestiaAdapter` is only re-exported).
- **What exists**: A `reqwest` client and `SettlementAdapter` impls for `submit_root`, `fetch_finality`, and `verify_inclusion`.
- **✅ Resolved (C-2, v0.1.68, commit `0725fc7`)**: `verify_inclusion` now compares the locally computed Merkle root against the root parsed from the RPC response and returns `false` on mismatch. The `CRITICAL TODO` this inventory used to point at no longer exists in the source.
- **⚠️ Remaining gaps — must be closed before this can be called integrated**:
  1. **Unbound root comparison.** `verify_inclusion` issues `GET /share/commitment` with **no namespace, height, or leaf parameter**, so the root it compares against is not bound to the blob being verified. The comparison is real but unanchored, which limits how much assurance it actually provides.
  2. **Fabricated transaction hashes.** `submit_root` discards the Celestia response body and returns `mock_tx_hash()` — a locally derived BLAKE3 value. `fetch_finality` then queries `/blob/commitment/0x<that local hash>`, which cannot correspond to a real blob. The submit → finality → verify chain therefore cannot close against a live node.
  3. **Endpoint paths do not match celestia-node.** `/submit_blob`, `/share/commitment`, and `/blob/commitment/{hash}` are not the celestia-node JSON-RPC 2.0 API (`blob.Submit`, `blob.GetProof`, `header.GetByHeight`, …). These calls would not succeed against a real node as written.
  4. **Synthesized finality metadata.** `confirmation_count` is hardcoded to `3` and `proof_hash` is derived locally rather than taken from the chain.
- **Runtime guard**: In disabled/mock mode, `CelestiaAdapter::new` panics under `OMNIA_ENV=production`; node startup also refuses any non-live settlement adapter in production.
- **Legacy `SettlementLayer` impl**: Returns `NotImplemented` for all methods (Celestia has no proof verification or asset layer)
- **Phase**: Needs a real celestia-node JSON-RPC implementation plus an integration test against a devnet before any mainnet consideration.

### Cosmos Settlement Adapter — **STUB** ⚠️

- **File**: `omnia-adapters/src/settlement/cosmos.rs`
- **Status**: Stub — legacy `SettlementLayer` only; all methods return `NotImplemented`; the adapter type is deprecated so new use emits compiler warnings
- **What exists**: Fail-closed legacy implementation returning `SettlementError::NotImplemented` for every operation
- **What's missing**: IBC integration, Cosmos SDK module, staking-based finality
- **Phase**: Planned for Phase 1

---

## Cross-cutting launch boundary

Launch-critical production settlement is available via live Ethereum (`ethereum-live`) **or live Bitcoin (`bitcoin-live`)** or the FFI settlement path with the native library present. Mock settlement, Solana, Cosmos, disabled Celestia, RF fingerprint simulation, PoUW attestation-only verification, and `node::pipeline` are roadmap/test surfaces. The node refuses startup with `OMNIA_ENV=production` when the selected settlement adapter reports `is_live() == false`.

## Phase 1: Features

### Mobile Wallet — **SHIPPED / FINANCIAL PILOT SURFACE INTEGRATED** ✅⚠️

- **Status**: v1 shipped July 2026 — lives in its own repo: [Willow7737/Omnia-Wallet](https://github.com/Willow7737/Omnia-Wallet)
- **What shipped**: Flutter wallet with dual-mode auth (on-device Ed25519 challenge/signature login **or** Google/GitHub/email via Supabase + `mint-node-jwt` edge function), UBC balance/send/history with per-transaction detail, governance voting, QR-based transfers, address book, biometric app lock, team news feed, in-app notifications, Buy OMNIA quote/status flow, and merchant QR payment submission.
- **Node-side support**: `node/src/api/wallet_auth.rs` (`/auth/challenge`, `/auth/login`, `/auth/register`) plus quote-backed payment-order routes and merchant settlement routes.
- **Pilot boundary**: the wallet surface is integrated against the sandbox/provider contract. Production mobile-money credentials, regulated provider onboarding, webhook operations, reconciliation, refund operations, and merchant operations tooling remain deployment work; the client never decides economic terms or payment success.

### Validator Network — **RUNNING** (5 nodes) ✅⚠️

- **Status**: A standing 5-node geo-distributed mesh runs continuously on
  v0.1.76+, protocol `/omnia/4.0.0`, 4 peers each, 3 regions, 2 continents:
  - **Node A** — Nuremberg, Germany (bootstrap + validator + ingress), 78.47.43.136
  - **Node B** — Ashburn, US (validator), 178.156.163.211
  - **Node C** — Singapore (validator), 5.223.85.30
  - **Node D** — Helsinki, Finland (validator), 46.62.218.24
  - **Node E** — Falkenstein, Germany (validator), 46.224.103.217
- **⚠️ Not yet trust-distributed**: all five nodes are operated by the same
  party. BFT's security argument assumes *independent* operators, so the
  network is currently fault-tolerant against machine and region failure but
  not against operator compromise. Third-party validators are the open item.
- **What's still planned**: staking pool management and auto-scaling validator
  set. The external validator onboarding documentation and sample configs now
  exist, but they are not evidence of independent operators until a third party
  is admitted and monitored in production.
- **Phase**: Phase 1

### Conviction Voting — **NOT STARTED** 🌑

- **Status**: No code exists
- **What's planned**: Time-weighted voting where lock duration multiplies vote weight
- **Related**: `economics/src/governance.rs` has quadratic voting but not conviction
- **Phase**: Planned for Phase 1

### Delegation — **NOT STARTED** 🌑

- **Status**: No code exists
- **What's planned**: Token holders can delegate voting power and UBC quota to trusted validators
- **Phase**: Planned for Phase 1

---

## Financial payment path — **IMPLEMENTED FOR CONTROLLED PILOT** ✅⚠️

- **Files**: `payment-order/src/{auth.rs,ghana_provider.rs,persistence.rs,quote_service.rs,engine.rs}`, `node/src/api/{payment_orders.rs,merchants.rs}`, `node/src/state.rs`
- **What exists**: Ed25519-signed server quotes with integer arithmetic, server-side quote storage and expiry, authenticated JWT wallet identity, HMAC-SHA256 provider callbacks with replay protection, event-sourced order persistence and recovery, a 25-state payment engine, treasury reservation/consume/release integration, merchant onboarding/payment request/history/receipt interfaces, and shared runtime AppState wiring.
- **What is deliberately not claimed**: the sandbox adapter is not a regulated production mobile-money rail; no fixed GHS redemption is promised; OMNIA pilot distribution uses existing treasury inventory rather than automatic minting; merchant confirmation requires the delivery-service role.
- **Remaining launch prerequisites**: production provider adapter and credentials, regulated compliance/KYC/AML controls, durable payment-store deployment, secret rotation, operational reconciliation and refund queues, on-chain delivery worker integration, and independent merchant operations tooling.

---

## ZK Circuit Notes

### Poseidon Hash Parameters — **PRODUCTION** but with caveats ✅

- **File**: `omnia-adapters/src/poseidon.rs`
- **Status**: Production-ready, but parameters use Cauchy MDS + BLAKE3 round constants
- **Caveat**: The original paper specifies Grain LFSR for parameter generation. Our implementation uses a different (mathematically equivalent) approach for parameter generation. This is safe but differs from the paper's exact specification.

---

## Summary Table

| Feature              | Layer | Status                             | File                                        | Phase Planned         |
| -------------------- | ----- | ---------------------------------- | ------------------------------------------- | --------------------- |
| RF Fingerprinting    | 3     | ⚠️ STUB — production fails closed  | `binding/src/rf_fingerprint.rs`             | Phase 1               |
| Proof-of-Useful-Work | 5     | ⚠️ PARTIAL                         | `economics/src/useful_work.rs`              | Phase 2               |
| Bitcoin Settlement   | 0     | ✅ LIVE (OP_RETURN anchoring via `bitcoincore-rpc`) | `omnia-adapters/src/settlement/bitcoin/live.rs` | ✅ Shipped          |
| Solana Settlement    | 0     | ⚠️ STUB — NotImplemented/deprecated | `omnia-adapters/src/settlement/solana.rs`   | Phase 1               |
| Celestia Settlement  | 0     | ✅⚠️ IMPLEMENTED (security caveat) | `omnia-adapters/src/settlement/celestia.rs` | Security fix required |
| Cosmos Settlement    | 0     | ⚠️ STUB — NotImplemented/deprecated | `omnia-adapters/src/settlement/cosmos.rs`   | Phase 1               |
| Mobile Wallet        | [Omnia-Wallet](https://github.com/Willow7737/Omnia-Wallet) | ✅⚠️ SHIPPED + financial pilot surface | Dual-mode auth, Buy OMNIA, merchant QR payment; regulated rail pending | Done / pilot |
| Financial payment path | 8/9 | ✅⚠️ CONTROLLED PILOT | Quote service, authenticated orders, Ghana sandbox, treasury allocation, merchant API | Pilot / production rail pending |
| Validator Network    | —     | ✅⚠️ RUNNING (5 nodes, 4 peers, 3 regions, one operator) | — | Phase 1               |
| Conviction Voting    | 5     | 🌑 NOT STARTED                     | —                                           | Phase 1               |
| Delegation           | 5     | 🌑 NOT STARTED                     | —                                           | Phase 1               |

---

## Deferred v0.1.68 Audit Items

The following items were identified during the v0.1.68 audit cycle but explicitly deferred to a later milestone. They are tracked here so they are not lost.

### Pipeline workers (C-8 deferred) — **DEAD CODE** ⚠️

- **File**: `node/src/pipeline.rs` (276 lines)
- **Status**: Deferred — `node/src/pipeline.rs` is 276 lines of dead code. The module is declared `pub mod pipeline` in `lib.rs` but the pipeline router and hot/warm/cold workers were removed from `main.rs` in the C-14 audit fix (v0.1.68). `main.rs` retains comments explaining the removal. The module is compiled but never instantiated at runtime.
- **What's missing**: Real worker threads that process pipeline stages; current code is unreachable
- **Phase**: Deferred from v0.1.68 audit

### Asymmetric JWT (C-5 deferred) — **UNBLOCKED** ⚠️

- **Status**: Deferred — H-14 dependency now satisfied, unblocked
- **What's missing**: Migration from symmetric to asymmetric (RS256/ES256) JWT signing
- **Phase**: Deferred from v0.1.68 audit (now unblocked)

### VRF rename (C-7 deferred) — **RENAME** ⚠️

- **File**: `substrate/src/vrf.rs` (legacy path)
- **Status**: Deferred — `vrf.rs` → `deterministic_selection.rs` rename
- **What's missing**: Module rename to reflect the actual (non-spec-compliant V1) construction; ECVRF V2 lives in `omnia-crypto/src/vrf.rs`
- **Phase**: Deferred from v0.1.68 audit

### LRU creator buffer (H-4) — **ANTI-SPAM** ✅

- **File**: `omnia-consensus/src/causal_graph.rs` (`SequenceBuffer`)
- **Status**: ✅ Implemented — the out-of-order buffer's creator map is bounded by `MAX_BUFFERED_CREATORS = 1024` with LRU eviction, on top of the existing `MAX_SEQUENCE_BUFFER_PER_CREATOR = 256` and `MAX_SEQUENCE_GAP = 512` bounds. Attacker-minted NodeIds can no longer grow the map without bound; worst-case buffer memory is `1024 × 256` events.
- **Phase**: Deferred from v0.1.68 audit; landed post-ADR-025.

### Chaos test safety checker (H-11 deferred) — **AUTOMATION** ⚠️

- **Status**: Deferred — automated safety property verification
- **What's missing**: Automated verification of safety properties (e.g. no double-spend, causal consistency) at the end of each chaos test run
- **Phase**: Deferred from v0.1.68 audit

### Substrate write lock scope (H-3 deferred) — **RESOLVED** ✅

- **Original file**: `substrate/src/consensus.rs` (now a 2-line re-export to `omnia-consensus`)
- **Status**: Resolved — the original write-lock contention issue was in the monolithic `substrate/src/consensus.rs`. That file was refactored into the `omnia-consensus` crate during the ADR-025 two-lane consensus rewrite. The current `omnia-consensus/src/consensus.rs` (2648 lines) uses a lock-free event pool with slab allocation and vector clock indexing — no `RwLock` or `Mutex` write locks exist in the consensus hot path.
- **Phase**: Deferred from v0.1.68 audit; implicitly resolved by ADR-025 refactor

| Deferred Item                       | Layer | Status       | File                                | Phase         |
| ----------------------------------- | ----- | ------------ | ----------------------------------- | ------------- |
| Pipeline workers (C-8)              | —     | ⚠️ DEFERRED  | `node/src/pipeline.rs`              | v0.1.68 audit |
| Asymmetric JWT (C-5)                | —     | ⚠️ DEFERRED  | —                                   | v0.1.68 audit |
| VRF rename (C-7)                    | 1     | ⚠️ DEFERRED  | `substrate/src/vrf.rs`              | v0.1.68 audit |
| LRU creator buffer (H-4)            | 1     | ✅ DONE      | `omnia-consensus/src/causal_graph.rs` | v0.1.68 audit |
| Chaos test safety checker (H-11)    | —     | ⚠️ DEFERRED  | —                                   | v0.1.68 audit |
| Substrate write lock scope (H-3)    | 1     | ✅ RESOLVED (ADR-025 refactor)  | `omnia-consensus/src/consensus.rs` | v0.1.68 audit |

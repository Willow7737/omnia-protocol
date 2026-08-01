# Stub & Partial Implementation Inventory

This document catalogs all stub, placeholder, and partial implementations in the Omnia Protocol codebase. Items are tracked so they are not mistaken for production-ready features.

> Last updated: 2026-07-18 (post-ADR-025 rollout; live 5-node testnet with measured Lane 0 finality)

---

## Layer 3: Binding Layer

### RF Fingerprinting — **STUB** ⚠️

- **File**: `binding/src/rf_fingerprint.rs`
- **Status**: Stub — requires SDR hardware (HackRF/USRP) for production use
- **What exists**: Trait definition and mock implementation for testing
- **What's missing**: Real SDR integration, signal capture, fingerprint extraction, matching algorithm
- **Blocker**: Hardware dependency — requires physical SDR device
- **Phase**: Planned for Phase 1

---

## Layer 5: Economics

### Proof-of-Useful-Work — **PARTIAL** ⚠️

- **File**: `economics/src/useful_work.rs`
- **Status**: Partial — 3 work types defined, not production-ready
- **What exists**:
  - `UsefulWorkType` enum with 3 variants (ML training, scientific compute, data analysis)
  - `UsefulWorkVerifier` trait with mock implementation
  - Basic reward calculation logic
- **What's missing**:
  - Real work verification (currently trusts prover's claim)
  - Benchmark-based difficulty adjustment
  - Anti-Sybil measures for work submission
  - Integration with external compute providers
- **Phase**: Planned for Phase 2

---

## Phase 0: ZK-Rollup Settlement Adapters

### Bitcoin Settlement Adapter — **STUB** ⚠️

- **File**: `omnia-adapters/src/settlement/bitcoin.rs`
- **Status**: Stub — implements `SettlementAdapter` trait but returns hardcoded values
- **What exists**: Trait implementation with `submit_batch()` and `verify_batch()` returning success
- **What's missing**: Actual Bitcoin script integration, OP_RETURN data embedding, transaction construction
- **Phase**: Planned for Phase 1

### Solana Settlement Adapter — **STUB** ⚠️

- **File**: `omnia-adapters/src/settlement/solana.rs`
- **Status**: Stub — implements `SettlementAdapter` trait but returns hardcoded values
- **What exists**: Trait implementation with no-op methods
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
- **Legacy `SettlementLayer` impl**: Returns `NotImplemented` for all methods (Celestia has no proof verification or asset layer)
- **Phase**: Needs a real celestia-node JSON-RPC implementation plus an integration test against a devnet before any mainnet consideration.

### Cosmos Settlement Adapter — **STUB** ⚠️

- **File**: `omnia-adapters/src/settlement/cosmos.rs`
- **Status**: Stub — implements `SettlementAdapter` trait but returns hardcoded values
- **What exists**: Trait implementation with no-op methods
- **What's missing**: IBC integration, Cosmos SDK module, staking-based finality
- **Phase**: Planned for Phase 1

---

## Phase 1: Planned Features (Not Started)

### Mobile Wallet — **SHIPPED** ✅

- **Status**: v1 shipped July 2026 — lives in its own repo: [Willow7737/Omnia-Wallet](https://github.com/Willow7737/Omnia-Wallet)
- **What shipped**: Flutter wallet with dual-mode auth (on-device Ed25519 challenge/signature login **or** Google/GitHub/email via Supabase + `mint-node-jwt` edge function), UBC balance/send/history with per-transaction detail, governance voting, QR-based transfers, address book, biometric app lock, team news feed, in-app notifications — verified end-to-end against the live testnet node
- **Node-side support**: `node/src/api/wallet_auth.rs` (`/auth/challenge`, `/auth/login`, `/auth/register`)

### Validator Network — **NOT STARTED** 🌑

- **Status**: No code exists; single-node operator for Phase 0
- **What's planned**: Multi-validator coordination, staking pool management, auto-scaling validator set
- **Phase**: Planned for Phase 1

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

## ZK Circuit Notes

### Poseidon Hash Parameters — **PRODUCTION** but with caveats ✅

- **File**: `omnia-adapters/src/poseidon.rs`
- **Status**: Production-ready, but parameters use Cauchy MDS + BLAKE3 round constants
- **Caveat**: The original paper specifies Grain LFSR for parameter generation. Our implementation uses a different (mathematically equivalent) approach for parameter generation. This is safe but differs from the paper's exact specification.

---

## Summary Table

| Feature              | Layer | Status                             | File                                        | Phase Planned         |
| -------------------- | ----- | ---------------------------------- | ------------------------------------------- | --------------------- |
| RF Fingerprinting    | 3     | ⚠️ STUB                            | `binding/src/rf_fingerprint.rs`             | Phase 1               |
| Proof-of-Useful-Work | 5     | ⚠️ PARTIAL                         | `economics/src/useful_work.rs`              | Phase 2               |
| Bitcoin Settlement   | 0     | ⚠️ STUB                            | `omnia-adapters/src/settlement/bitcoin.rs`  | Phase 1               |
| Solana Settlement    | 0     | ⚠️ STUB                            | `omnia-adapters/src/settlement/solana.rs`   | Phase 1               |
| Celestia Settlement  | 0     | ✅⚠️ IMPLEMENTED (security caveat) | `omnia-adapters/src/settlement/celestia.rs` | Security fix required |
| Cosmos Settlement    | 0     | ⚠️ STUB                            | `omnia-adapters/src/settlement/cosmos.rs`   | Phase 1               |
| Mobile Wallet        | [Omnia-Wallet](https://github.com/Willow7737/Omnia-Wallet) | ✅ SHIPPED (v1, July 2026) | Dual-mode auth, live vs. testnet node | Done |
| Validator Network    | —     | 🌑 NOT STARTED                     | —                                           | Phase 1               |
| Conviction Voting    | 5     | 🌑 NOT STARTED                     | —                                           | Phase 1               |
| Delegation           | 5     | 🌑 NOT STARTED                     | —                                           | Phase 1               |

---

## Deferred v0.1.68 Audit Items

The following items were identified during the v0.1.68 audit cycle but explicitly deferred to a later milestone. They are tracked here so they are not lost.

### Pipeline workers (C-8 deferred) — **DEAD CODE** ⚠️

- **File**: `node/src/pipeline.rs` (262 lines)
- **Status**: Deferred — `node/src/pipeline.rs` is 262 lines of dead code
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

### Substrate write lock scope (H-3 deferred) — **CONTENTION** ⚠️

- **File**: `substrate/src/consensus.rs`
- **Status**: Deferred — reduce lock holding time
- **What's missing**: Narrow the substrate write-lock critical section to reduce contention; currently the lock is held across the full consensus round
- **Phase**: Deferred from v0.1.68 audit

| Deferred Item                       | Layer | Status       | File                                | Phase         |
| ----------------------------------- | ----- | ------------ | ----------------------------------- | ------------- |
| Pipeline workers (C-8)              | —     | ⚠️ DEFERRED  | `node/src/pipeline.rs`              | v0.1.68 audit |
| Asymmetric JWT (C-5)                | —     | ⚠️ DEFERRED  | —                                   | v0.1.68 audit |
| VRF rename (C-7)                    | 1     | ⚠️ DEFERRED  | `substrate/src/vrf.rs`              | v0.1.68 audit |
| LRU creator buffer (H-4)            | 1     | ✅ DONE      | `omnia-consensus/src/causal_graph.rs` | v0.1.68 audit |
| Chaos test safety checker (H-11)    | —     | ⚠️ DEFERRED  | —                                   | v0.1.68 audit |
| Substrate write lock scope (H-3)    | 1     | ⚠️ DEFERRED  | `substrate/src/consensus.rs`        | v0.1.68 audit |

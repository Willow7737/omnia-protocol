# Stub & Partial Implementation Inventory

This document catalogs all stub, placeholder, and partial implementations in the Omnia Protocol codebase. Items are tracked so they are not mistaken for production-ready features.

> Last updated: 2026-06-20 (v0.1.68 audit cycle)

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

### Celestia Settlement Adapter — **IMPLEMENTED** (with security caveat) ✅⚠️

- **File**: `omnia-adapters/src/settlement/celestia.rs`
- **Status**: Implemented — `SettlementAdapter` trait has real HTTP RPC integration behind the `celestia` feature flag, with mock mode when disabled
- **What exists**: Real Celestia RPC integration for `submit_root`, `fetch_finality`, and `verify_inclusion` (with `reqwest` client)
- **⚠️ SECURITY CAVEAT**: `verify_inclusion` computes the Merkle root locally but **never compares it against the on-chain data root**. A malicious Celestia node could serve a valid-looking but incorrect commitment. This MUST be fixed before mainnet by fetching the on-chain data root hash and comparing it with the computed root. See the `CRITICAL TODO` comment in the source code.
- **Legacy `SettlementLayer` impl**: Returns `NotImplemented` for all methods (Celestia has no proof verification or asset layer)
- **Phase**: Security fix required before mainnet

### Cosmos Settlement Adapter — **STUB** ⚠️

- **File**: `omnia-adapters/src/settlement/cosmos.rs`
- **Status**: Stub — implements `SettlementAdapter` trait but returns hardcoded values
- **What exists**: Trait implementation with no-op methods
- **What's missing**: IBC integration, Cosmos SDK module, staking-based finality
- **Phase**: Planned for Phase 1

---

## Phase 1: Planned Features (Not Started)

### Mobile Wallet — **NOT STARTED** 🌑

- **Status**: No code exists
- **What's planned**: React Native / Flutter wallet with biometric auth, QR-based transfers, UBC balance tracking
- **Phase**: Planned for Phase 1

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

| Feature | Layer | Status | File | Phase Planned |
|---------|-------|--------|------|---------------|
| RF Fingerprinting | 3 | ⚠️ STUB | `binding/src/rf_fingerprint.rs` | Phase 1 |
| Proof-of-Useful-Work | 5 | ⚠️ PARTIAL | `economics/src/useful_work.rs` | Phase 2 |
| Bitcoin Settlement | 0 | ⚠️ STUB | `omnia-adapters/src/settlement/bitcoin.rs` | Phase 1 |
| Solana Settlement | 0 | ⚠️ STUB | `omnia-adapters/src/settlement/solana.rs` | Phase 1 |
| Celestia Settlement | 0 | ✅⚠️ IMPLEMENTED (security caveat) | `omnia-adapters/src/settlement/celestia.rs` | Security fix required |
| Cosmos Settlement | 0 | ⚠️ STUB | `omnia-adapters/src/settlement/cosmos.rs` | Phase 1 |
| Mobile Wallet | — | 🌑 NOT STARTED | — | Phase 1 |
| Validator Network | — | 🌑 NOT STARTED | — | Phase 1 |
| Conviction Voting | 5 | 🌑 NOT STARTED | — | Phase 1 |
| Delegation | 5 | 🌑 NOT STARTED | — | Phase 1 |

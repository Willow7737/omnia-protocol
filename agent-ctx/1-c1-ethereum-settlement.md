# Task 1 — C-1: Real Ethereum Settlement with Alloy

## Summary

Implemented real Ethereum settlement using the `alloy` crate, gated behind the `ethereum-live` feature flag. The adapter now supports two modes: simulated (BLAKE3-based mock, default) and live (real RPC calls via alloy). All existing tests pass without the feature enabled, and new feature-gated tests validate the live client architecture.

## Changes Made

### 1. `zk/Cargo.toml` — Added alloy dependency

- Added `alloy = { version = "1", features = [...], optional = true }` with features: `provider-ws`, `provider-http`, `signers`, `signer-local`, `contract`, `network`, `primitives`
- Changed `ethereum-live = []` to `ethereum-live = ["dep:alloy"]` to properly gate the dependency
- Updated comments to reflect alloy instead of the previously-planned ethers-rs

### 2. `zk/src/settlement/mod.rs` — New error variants

Added three new `SettlementError` variants for live mode:
- `TxFailed(String)` — Transaction was rejected or reverted on-chain
- `TxTimedOut(u64)` — Transaction timed out waiting for confirmations
- `ContractError(String)` — Smart contract call returned unexpected data

### 3. `zk/src/settlement/ethereum.rs` — Complete rewrite with live mode

**New types (feature-gated):**
- `EthereumLiveClient` — Holds RPC URL, operator key, contract address, and gas config. Creates alloy providers lazily per-call for `Send + Sync` compatibility without type erasure.
- `OmniaRollup` — Contract bindings generated via `alloy::sol!` macro from the ABI JSON matching `zk/contracts/ethereum/OmniaRollup.sol`

**New methods on `EthereumLiveClient`:**
- `connect(config)` — Validates config and creates the client (lazy, no network connection)
- `build_provider()` — Constructs an alloy provider with recommended fillers + wallet signing
- `submit_batch_live(bundle)` — Decomposes Groth16 proof into `proofA`, `proofB`, `proofC`, calls `submitBatch` on the contract, waits for confirmations
- `latest_state_root_live()` — Calls `stateRoot()` view function
- `deposit_live(l2_did, amount)` — Calls `deposit(bytes32)` with BLAKE3-derived DID mapping
- `request_withdrawal_live(l2_did, amount)` — Calls `requestWithdrawal(bytes32, uint256)`
- `verify_proof_live(old_root, new_root, proof)` — Simulates `submitBatch` via `eth_call` for read-only proof verification
- `slice_to_array()` — Helper to convert `&[u8]` to `[u8; 32]` with proper error handling

**Updated `EthereumAdapter`:**
- Added `live_client: Option<EthereumLiveClient>` field (feature-gated)
- `with_mode(Live)` now creates `EthereumLiveClient` when feature is enabled, returns `ConfigError` otherwise
- `EthereumConfig::validate()` now also checks for non-empty, `0x`-prefixed operator private key
- All `SettlementLayer` trait methods delegate to `EthereumLiveClient` in Live mode
- No `unwrap()` in production code — all errors use typed `SettlementError` variants
- BLAKE3 domain separation used for all hash operations

### 4. `zk/tests/settlement_agnostic.rs` — Updated integration tests

- Replaced `test_ethereum_adapter_with_mode_live_not_implemented` with `test_ethereum_adapter_with_mode_live_requires_feature` which correctly tests the new behavior (ConfigError without feature, success with feature)
- Updated `test_ethereum_config_validation_valid` to provide a valid operator private key

### 5. `.github/workflows/ethereum-settlement.yml` — New CI workflow

Three jobs:
- **simulated**: Tests without `ethereum-live` feature (simulated mode only)
- **live**: Installs Foundry/Anvil, deploys OmniaRollup contract, tests with `--features ethereum-live`
- **forge-test**: Runs Solidity contract tests via Forge

## Design Decisions

1. **Lazy provider creation**: `EthereumLiveClient` creates a new alloy provider per-call. This avoids complex type erasure for alloy's filler stack and keeps the client trivially `Send + Sync`.

2. **Proof decomposition**: The 256-byte Groth16 proof is decomposed into `proofA` (uint256[2]), `proofB` (uint256[2][2]), `proofC` (uint256[2]) matching the Solidity contract's calldata layout.

3. **BLAKE3 domain separation**: All hash operations use `blake3::derive_key` with domain-specific contexts (e.g., "OMNIA-ETH-BATCH-DATA", "OMNIA-DID-MAP", "OMNIA-PROOF-VERIFY") to prevent cross-domain attacks.

4. **No `unwrap()`**: All error paths use typed `SettlementError` variants, complying with `#![deny(clippy::unwrap_used)]`.

5. **Feature flag isolation**: All alloy-dependent code is behind `#[cfg(feature = "ethereum-live")]`. Without the feature, the code compiles with zero alloy dependencies.

## Files Modified

- `zk/Cargo.toml` — alloy dependency + feature flag
- `zk/src/settlement/mod.rs` — new error variants
- `zk/src/settlement/ethereum.rs` — complete rewrite with live mode
- `zk/tests/settlement_agnostic.rs` — updated integration tests
- `.github/workflows/ethereum-settlement.yml` — new CI workflow

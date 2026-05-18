# H-6: Consensus State Persistence Across Restarts

**Task ID**: 10
**Status**: COMPLETED
**Date**: 2025-03-05

## Summary

Implemented consensus state persistence for crash recovery, enabling nodes to resume from the last persisted round without replaying all events from genesis.

## Changes Made

### 1. New file: `substrate/src/consensus_store.rs`
- `ConsensusStoreError` enum with Database, Serialization, and InvalidVersion variants
- `ConsensusState` struct (serializable snapshot of engine state):
  - `current_round`, `round_seed`, `committed_events`, `last_finalized_round`
  - `active_validators`, `equivocation_tracking`, `version`
- `ConsensusStore` trait with `save_state`, `load_state`, `save_round`, `load_round`
- `RedbConsensusStore` implementation with `open()` (file-based) and `in_memory()` (testing)
- Uses `postcard` for binary serialization (avoids JSON key issues with `[u8; 32]` HashMap keys)
- 6 unit tests for the store

### 2. Updated: `substrate/src/lib.rs`
- Added `pub mod consensus_store;`
- Added re-export: `ConsensusState as PersistedConsensusState`, `ConsensusStore`, `ConsensusStoreError`, `RedbConsensusStore`
- Added `consensus_data_dir: Option<PathBuf>` to `SubstrateConfig`
- Updated `SubstrateConfig::new()` and `with_network_size()` to include `consensus_data_dir: None`
- Added `consensus_store: Option<Arc<dyn ConsensusStore>>` to `Substrate` struct
- Updated `Substrate::new()` to:
  - Open `RedbConsensusStore` if `consensus_data_dir` is set
  - Use `ConsensusEngine::load_or_new()` to restore from persisted state
  - Store the `Arc<dyn ConsensusStore>` for later persistence
- Updated `process_consensus()` to persist state after committed events

### 3. Updated: `substrate/src/consensus.rs`
- Added imports for `ConsensusStore`, `ConsensusStoreError`, `PersistedConsensusState`, `Arc`
- Added `ConsensusError::Config(String)` variant
- Added `ConsensusEngine::load_or_new()` — creates engine, restoring from persisted state if available
- Added `ConsensusEngine::restore_state()` — restores round_seed, committed_count, node_info, round_timer
- Added `ConsensusEngine::persist_state()` — saves snapshot to store
- 7 new tests for persistence functionality

### 4. Updated: `node/src/config.rs`
- Added `consensus_data_dir: Option<PathBuf>` to `NodeConfig`
- Added `consensus_data_dir: Option<String>` to `NodeConfigFile`
- Added `consensus_dir()` helper method (defaults to `<data_dir>/consensus.redb`)
- Wired `consensus_data_dir` in `from_cli()`
- Updated all test configs to include `consensus_data_dir: None`

### 5. Updated: `node/src/main.rs`
- Added `substrate_config.consensus_data_dir = Some(config.consensus_dir());`

### 6. Updated: `node/tests/integration.rs` and `node/tests/api_integration.rs`
- Added `consensus_data_dir: None` to test configs

## Test Results
- 386 substrate lib tests pass
- 26 node lib tests pass
- All builds successful (omnia-substrate, omnia-node, omnia-shards)

## Design Decisions
1. **postcard over serde_json**: Used binary serialization to avoid JSON's requirement that object keys be strings. `NodeId = [u8; 32]` cannot be a JSON key, so postcard was chosen (already a project dependency, used by RedbSlashingStore).

2. **Version field**: Added `version: u32` to `ConsensusState` for forward-compatible format migrations. Version 1 is the initial format.

3. **Equivocation tracking**: Derived as `HashMap<NodeId, u64>` (NodeId → max sequence seen) from `first_event_for_sequence`. On restore, only `node_info` and `last_witness_round` are set to prevent double-witnessing; the full `first_event_for_sequence` map cannot be reconstructed from the summary.

4. **Round timer**: Re-initialized on restore via `round_timer.start_round(state.current_round)`, since `Instant` is not serializable.

5. **Persistence trigger**: State is persisted in `process_consensus()` only when events are committed (indicating round progression), not on every event processing.

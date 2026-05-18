# Task 5 — H-1: Fix ZK Circuit Trusted Setup Dummy Values (HIGH)

**Status**: Completed

## Work Done

- Added `for_setup()` method to `ExpandedRollupCircuit` with non-zero witnesses
- Updated `generate_trusted_setup_expanded()` in prover.rs to use `for_setup()`
- Added warning doc comment on `empty()` recommending `for_setup()`
- Added test: `test_for_setup_produces_non_zero_witnesses`
- All 83 omnia-zk tests pass

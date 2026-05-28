# Task: Fix Event::new() and Event::genesis() Result Return Type Callers

## Summary
Fixed all callers across the omnia-protocol workspace where `Event::new()` and `Event::genesis()` now return `Result<Event, EventValidationError>` instead of `Event` directly.

## Root Cause
`Event::new()` and `Event::genesis()` in `omnia-primitives/src/event.rs` changed their return types from `Event` to `Result<Event, EventValidationError>`. All callers that treated the return as a plain `Event` needed to handle the `Result`.

## Files Modified

### Fuzz Targets
1. **`fuzz/fuzz_targets/causal_graph_insert.rs`** - Added `.unwrap()` to `Event::genesis()` (line 15) and `Event::new()` (line 20)
2. **`fuzz/fuzz_targets/event_validate.rs`** - Added `.unwrap()` to `Event::genesis()` (line 14) and `Event::new()` (line 19)
3. **`fuzz/fuzz_targets/shard_route.rs`** - Added `.unwrap()` to `Event::genesis()` (line 15)

### Chaos Tests Library
4. **`chaos-tests/src/lib.rs`** - Used `.expect("genesis event creation should not fail")` for genesis (line 251) and `.map_err(|e| anyhow::anyhow!("Event creation failed: {e}"))?` for submit_event (lines 467-472) to respect `#![deny(clippy::unwrap_used)]`
5. **`chaos-tests/src/stability_test.rs`** - Used `.expect("genesis event creation should not fail")` for warmup (line 325) and `.expect("event creation should not fail")` for submit_round (lines 370-375)
6. **`chaos-tests/src/safety_monitoring.rs`** - Used `.expect("genesis event creation should not fail")` for warmup (line 341) and `.expect("event creation should not fail")` for submit_and_propagate (lines 391-396)
7. **`chaos-tests/src/gossip_chaos.rs`** - Used `.expect("genesis event creation")` (lines 241, 642) and `.expect("event creation")` (line 456)
8. **`chaos-tests/src/load_test.rs`** - Used `.expect("event creation should not fail")` for the if/else expression (lines 207-219)
9. **`chaos-tests/src/full_chaos_suite.rs`** - Used `.expect("event creation should not fail")` for both equivocating events (lines 394-402, 406-414)

### Chaos Tests Integration Tests
10. **`chaos-tests/tests/integration_test.rs`** - Used `.expect("genesis event creation")` (lines 33, 357, 712) and `.expect("event creation")` (line 411)
11. **`chaos-tests/tests/byzantine.rs`** - Used `.unwrap()` (lines 35-43, 48-56, 124, 129) since the file has `#![allow(clippy::unwrap_used)]`

## Strategy Used
- **Fuzz code**: Used `.unwrap()` since failures in fuzz targets are acceptable
- **Test code without clippy deny**: Used `.unwrap()` where `#![allow(clippy::unwrap_used)]` is in effect
- **Test code with clippy deny**: Used `.expect("descriptive message")`
- **Production code returning Result**: Used `?` with `.map_err()` for error conversion
- **Production code not returning Result**: Used `.expect("descriptive message")`

## Verification
`cargo check --workspace` passes cleanly with no errors.

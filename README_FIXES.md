# Omnia Protocol — 6 Critical Fixes

This package contains all 6 critical fixes for the Omnia Protocol Layer 1 substrate.

## Files Changed

| Fix | File | What Changed |
|-----|------|-------------|
| 1 | `substrate/Cargo.toml` | Added `bincode = "1.3"` dependency |
| 1 | `substrate/src/event.rs` | Replaced fake `serde_json` bincode module with real `bincode` crate |
| 2 | `substrate/Cargo.toml` | Added `ed25519-dalek = "2.1"`, `rand = "0.8"` |
| 2 | `substrate/src/crypto.rs` | **NEW** — Ed25519 key types and generation |
| 2 | `substrate/src/event.rs` | Real signature verification; `creator_pubkey` + `signature` fields |
| 3 | `substrate/src/consensus.rs` | Fixed `is_witness()` to track per-round; fixed `check_commitments()` for small networks |
| 4 | `substrate/Cargo.toml` | Added `tokio`, `libp2p`, `futures`, `async-trait`, `blake3` |
| 4 | `substrate/src/network.rs` | **NEW** — Real libp2p gossipsub + mDNS + request-response |
| 4 | `substrate/src/gossip.rs` | Refactored to async with `tokio::sync::RwLock` |
| 4 | `substrate/src/lib.rs` | Updated `Substrate` to use `tokio::sync::RwLock`; async `start()`/`submit_event()` |
| 5 | `substrate/benches/throughput.rs` | **NEW** — Criterion benchmarks for event creation, graph insertion, vector clock merge |
| 6 | `substrate/tests/property_tests.rs` | **NEW** — proptest for GCounter commutativity, associativity, vector clock partial order |
| 6 | `substrate/Cargo.toml` | Added `proptest = "1.4"` dev-dependency |

## How to Apply

1. Backup your existing `substrate/` directory.
2. Copy all files from this package into your repo's `substrate/` directory.
3. Run `cargo check` in `substrate/` — Fix 1 must compile before proceeding.
4. Run `cargo test` — all tests including new ones should pass.
5. Run `cargo clippy` — zero warnings expected.
6. Run `cargo bench` — benchmarks should execute without panic.

## Verification Checklist

- [ ] `cargo check` passes with zero warnings
- [ ] `cargo test` passes — all tests including new ones
- [ ] `cargo clippy` passes — no lints
- [ ] `cargo bench` runs — benchmarks execute without panic
- [ ] No `unwrap()` in production code paths (tests OK)
- [ ] All crypto uses constant-time implementations
- [ ] Async code uses `tokio::sync` primitives, never `std::sync::Mutex` across await points
- [ ] Binary serialization produces deterministic output

## Git Commit Format

```bash
git add substrate/
git commit -m "fix(layer1): real bincode serialization + Ed25519 signatures + consensus witness fix + async libp2p gossip + benchmarks + property tests"
```

## Notes

- `vector_clock.rs`, `causal_graph.rs`, and `crdt.rs` core logic were NOT rewritten per constraints.
- The 5-layer architecture was preserved.
- No markdown documentation was added — only code comments and rustdoc.

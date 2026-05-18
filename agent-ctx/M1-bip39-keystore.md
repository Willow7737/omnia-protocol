# M-1: BIP-39 Mnemonic Support for Keystore

## Summary
Added BIP-39 mnemonic support to the `omnia-substrate` keystore module as specified in M-1 of the Omnia Protocol Phase 2.

## Files Modified
- `substrate/Cargo.toml` — Added `bip39 = { version = "2", features = ["zeroize", "rand"] }`
- `substrate/src/keystore.rs` — Added `KeyPurpose` enum, `from_mnemonic`, `generate_with_mnemonic`, `derive_child_key`, `derive_key_from_seed`, `initialize_with_key` methods, `aes_gcm_encrypt_with_key` helper, and 3 new tests
- `substrate/src/lib.rs` — Added `KeyPurpose` to re-exports

## Key Decisions
- Added `rand` feature to bip39 dependency (required for `Mnemonic::generate()`)
- Replaced deprecated `word_iter()` with `words()` in test code
- Skipped adding private `keypair()` helper (already exists as public method)
- Restored sled dependency lines that were removed by a prior agent's commit

## Test Results
All 342 tests pass (342 unit + 9 integration + 3 property + 13 slashing + 8 persistence + 21 doctests).

# H-2: Migrate pqc_kyber → ml-kem to Fix KyberSlash

**Task ID:** 2
**Date:** 2026-03-04
**Status:** ✅ Complete

## Summary

Migrated from `pqc_kyber` 0.7.1 to `ml-kem` 0.2 to resolve the KyberSlash timing side-channel vulnerability (RUSTSEC-2023-0079). The `ml-kem` crate provides the FIPS 203 standard ML-KEM-768 implementation with constant-time operations, eliminating the division-timing vulnerability present in `pqc_kyber`.

## Changes Made

### 1. `binding/Cargo.toml`
- Replaced `pqc_kyber = { version = "0.7.1", features = ["rand", "zeroize"] }` with `ml-kem = { version = "0.2", features = ["zeroize"] }`

### 2. `binding/src/quantum_commit.rs`
- **Imports**: Replaced `pqc_kyber` imports with `ml_kem::{MlKem768, EncapsulationKey, DecapsulationKey}` and `ml_kem::kem::{Encapsulate, Decapsulate}`, `rand::rngs::OsRng`
- **Constants**: Added `ML_KEM_768_ENCAPSULATION_KEY_SIZE` (1184), `ML_KEM_768_DECAPSULATION_KEY_SIZE` (2400), `ML_KEM_768_CIPHERTEXT_SIZE` (1088), `ML_KEM_768_SHARED_SECRET_SIZE` (32) to replace `pqc_kyber::KYBER_*` constants
- **`generate_kyber_keypair()`**: Now uses `MlKem768::generate(&mut rng)` returning `(dk, ek)`, extracts bytes via `ek.as_bytes()` and `dk.as_bytes()`
- **`kyber_encapsulate()`**: Now uses `MlKemEncapKey::from_bytes()` + `ek.encapsulate(&mut rng)` returning `(ss, ct)`, extracts bytes via `ss.as_bytes()` and `ct.as_bytes()`
- **`kyber_decapsulate()`**: Now uses `MlKemDecapKey::from_bytes()` + `ml_kem::Ciphertext::from_bytes()` + `dk.decapsulate(&ct)`, with `map_err` for KEM errors
- **Removed**: `impl From<pqc_kyber::KyberError> for KyberError` — errors now handled via `map_err(|e| KyberError::KemFailed(...))`
- **Doc comments**: Updated references from "Kyber768" to "ML-KEM-768", from "Kyber" to "ML-KEM" where appropriate
- **Tests**: All test assertions updated to use `ML_KEM_768_*` constants instead of `pqc_kyber::KYBER_*`

### 3. `deny.toml`
- Removed `RUSTSEC-2023-0079` from the `ignore` list (6 lines including comments)

### 4. `supply-chain/config.toml`
- Removed `[[exemptions.pqc_kyber]]` entry (version 0.7.1)
- Added `[[exemptions.ml-kem]]` entry (version 0.2) with appropriate notes about FIPS 203 and KyberSlash fix

### 5. `supply-chain/audits.toml`
- Added `[[audits.ml-kem]]` entry (version 0.2, criteria "safe-to-deploy") documenting the migration rationale

### 6. `binding/src/lib.rs`
- Updated doc comment from `pqc_kyber` to `ml-kem` in the Constraints section

### 7. `PHASE_3_SUMMARY.md`
- Updated M-1 resolution to note the migration from `pqc_kyber` to `ml-kem`

## API Compatibility

The public API is **unchanged**:
- `KyberKeyPair` struct (same field names and types)
- `KyberError` enum (same variants, removed `From<pqc_kyber::KyberError>` impl)
- `generate_kyber_keypair()`, `kyber_encapsulate()`, `kyber_decapsulate()` signatures unchanged

The key size constants are identical between Kyber768 and ML-KEM-768 (1184/2400/1088/32 bytes), so wire format is fully compatible.

## Notes

- `Cargo.lock` still references `pqc_kyber`; it will be updated automatically on next `cargo build`
- `agent-ctx/12-m1-kyber-kem.md` intentionally left unchanged (historical record)
- No `.github/workflows/` files exist in this project; no CI updates needed

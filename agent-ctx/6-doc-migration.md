# Task 6: Documentation Migration (sled→redb, bincode→postcard)

**Date:** 2026-15-05
**Status:** ✅ Completed

## Summary

Completed two documentation migration tasks (28–29) for the Omnia Protocol, systematically replacing all references to the old `sled` database with `redb` and all references to `bincode` serialization with `postcard` across the documentation.

## Task 28: Replace All sled References with redb

- Updated 14 files with sled→redb replacements
- Key changes: `SledSlashingStore` → `RedbSlashingStore`, `SledNonceStore` → `RedbNonceStore`, `sled` → `redb`
- Rewrote docs/OPERATIONS.md database section entirely
- Replaced sled alpha-quality warnings with redb production-quality descriptions
- Updated all audit documents (ATTACK_SURFACE, AUDIT_README, SELF_ASSESSMENT, AUDIT_SCOPE)

**Commit:** `docs: replace all sled references with redb in documentation`

## Task 29: Replace bincode References with postcard

- Updated 8 files with bincode→postcard replacements
- Key changes: `bincode::serialize()` → `postcard::to_allocvec()`, `bincode::deserialize()` → `postcard::from_bytes()`
- Updated ADR-005, ADR-006, ADR-008 with migration justification (no_std, deterministic encoding)
- Documented bincode 1.x retention only for v0 backward compat
- Updated SECURITY.md version label and README.md test badge

**Commit:** `docs: replace bincode references with postcard in documentation`

## Verification

- Remaining `sled` references are only in CHANGELOG.md (historical), migration notes, and historical discrepancy reports
- Remaining `bincode` references are only about "legacy" or "backward compat" retention
- No .adoc, .rst, or .txt files found in the repository

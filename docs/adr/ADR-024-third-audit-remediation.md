# ADR-024: Third-Pass Audit Remediation (NEW-C1 through NEW-L2)

**Date:** 2026-06-30
**Status:** Proposed
**Audit:** Omnia Protocol Architecture Audit Report, Third Pass (Super Z · Z.ai, 30 June 2026)
**Audited commit:** v0.1.79 (post-ADR-023)

## Context

A third independent audit identified 18 new findings (2 Critical, 7 High, 7 Medium, 2 Low) that the prior audits did not surface. Two of the new Critical findings invalidate claimed security fixes in ADR-023.

## Verification Summary

All 18 new findings were verified against the live code. All are CONFIRMED TRUE.

### Critical (2 findings)

| ID | Claim | Status |
|----|-------|--------|
| NEW-C1 | F-13 fix is security theater — verify_source_signature never called | **CONFIRMED + FIXED** |
| NEW-C2 | ZK VerifyingKey deserialized from attacker-supplied proof bytes | CONFIRMED — deferred (needs VK registry) |

### High (7 findings)

| ID | Claim | Status |
|----|-------|--------|
| NEW-H1 | production feature off by default — all fail-closed gates bypassed | **CONFIRMED + FIXED** |
| NEW-H2 | node_key.bin written unencrypted | Deferred |
| NEW-H3 | SlashingEngine Clone race causes lost updates | Deferred (same root as F-10) |
| NEW-H4 | thread_pool skips signature verification | **CONFIRMED + FIXED** |
| NEW-H5 | Silent event drops under burst load | Deferred |
| NEW-H6 | Fast-sync Sybil-vulnerable | Deferred |
| NEW-H7 | PeerScoreTracker + VersionHandshake dead code | Deferred |

### Medium (7 findings)

| ID | Claim | Status |
|----|-------|--------|
| NEW-M1 | parse_consensus_seed silent fallback in default constructors | **CONFIRMED + FIXED** |
| NEW-M2 | Quadratic voting Sybil attack | Deferred |
| NEW-M3 | Biological consent expiry disabled | Deferred |
| NEW-M4 | TimeLockVoting dead code | Deferred |
| NEW-M5 | JWT secret + AuthorizedCallers no rotation | Deferred |
| NEW-M6 | QuantumCommitment phase caller-controlled | Deferred (latent — binding dead code) |
| NEW-M7 | Replay protection diverges on save_incremental failure | **CONFIRMED + FIXED** |

### Low (2 findings)

| ID | Claim | Status |
|----|-------|--------|
| NEW-L1 | Stale omnia-zk audit entry | **CONFIRMED + FIXED** |
| NEW-L2 | O(epochs) decay loop | Deferred |

## Fixes Applied

### NEW-C1: F-13 Fix Is Security Theater

**Problem:** The F-13 fix checked `source_signature.is_none()` (presence) but never called `verify_source_signature()` (validity). Any `Some(random_64_bytes)` passed. Additionally, `verify_source_signature` itself only signed `self.payload`, omitting `causal_proof` from the signed digest.

**Fix:**
1. `route_cross_shard` now calls `msg.verify_source_signature(&event.creator_pubkey)` after the presence check
2. Rejects on verification failure (all builds, not just `#[cfg(feature = "production")]`)
3. `verify_source_signature` now includes `causal_proof.to_bytes()` in the signed data

### NEW-H1: Production Feature Off By Default

**Problem:** The F-6 (PoUW) and F-13 (cross-shard) fail-closed gates used `#[cfg(feature = "production")]`, but `production` was not in the default feature set. The default binary build bypassed all fail-closed behavior.

**Fix:** Made PoUW verification fail-closed unconditionally — empty `verifier_signature` always returns `false`, regardless of feature flags. Cross-shard signature verification (NEW-C1) is also unconditional.

### NEW-H4: Thread Pool Skips Signature Verification

**Problem:** `validate_and_insert` only called `verify_hash()` — no `verify_signature()`. The module docstring claimed signature verification was performed.

**Fix:** Added `event.verify_signature()` check after the hash check, before inserting into sharded state.

### NEW-M1: parse_consensus_seed Silent Fallback

**Problem:** `SubstrateConfig::new()` and `with_network_size()` used the unsafe legacy `parse_consensus_seed()` that silently fell back to a random seed on invalid input. `try_new()` also used `.unwrap_or(4)` for `OMNIA_TOTAL_NODES`, silently falling back on a typo.

**Fix:**
1. Deprecated `new()` and `with_network_size()` — they now delegate to `try_new()` / `try_with_network_size()` and panic on error
2. `try_new()` now validates `OMNIA_TOTAL_NODES` and returns `Err` on parse failure instead of silently falling back to 4

### NEW-M7: Replay Protection Diverges on save_incremental Failure

**Problem:** On `save_incremental` failure, the in-memory nonce map had the new nonce but the persistent store didn't. The code only logged a warning and continued. On restart, the stale persisted nonce allowed replay.

**Fix:** On `save_incremental` failure, the node now panics with a clear error message. This is fail-closed — silent divergence is worse than downtime.

### NEW-L1: Stale omnia-zk Audit Entry

**Problem:** `supply-chain/audits.toml` had `[[audits.omnia-zk]]` but the crate was renamed to `omnia-adapters`.

**Fix:** Renamed the audit entry to `[[audits.omnia-adapters]]`.

## Deferred (13 findings)

The following require architectural work and are tracked for future sprints:

- **NEW-C2** (VK registry): 1-2 weeks — needs `VkRegistry` mapping `CircuitId → VerifyingKey`
- **NEW-H2** (encrypted node key): 2-3 days — needs `EncryptedKeyStore` integration
- **NEW-H3** (slashing clone race): 2-3 days — needs write lock held across persist
- **NEW-H5** (event drops): 3-5 days — needs backpressure or unbounded channel + fast-sync trigger
- **NEW-H6** (fast-sync Sybil): 1-2 weeks — needs stake-weighted threshold + checkpoint signatures
- **NEW-H7** (dead code peer scoring): 3-5 days — needs wiring into gossip validation
- **NEW-M2** (voting Sybil): 1-2 weeks — needs proof-of-personhood or DID clustering
- **NEW-M3** (consent expiry): 2-3 days — needs epoch threading
- **NEW-M4** (TimeLockVoting): 3-5 days — needs wiring into governance
- **NEW-M5** (JWT rotation): 3-5 days — needs admin endpoint + file-watching
- **NEW-M6** (quantum phase): 3-5 days — latent (binding dead code)
- **NEW-L2** (decay loop): 2 hours — needs exponentiation by squaring
- Plus 15 deferred findings from prior audits (F-1, F-2, F-3, F-4, F-7, F-8, F-9, F-10, F-11, F-12, F-18, F-19, F-24, F-25, H1)

## Decision

Apply the 6 immediate fixes (NEW-C1, NEW-H1, NEW-H4, NEW-M1, NEW-M7, NEW-L1). Document the 13 deferred findings. The fixes address Sprint 1 of the audit's recommended 6-sprint roadmap.

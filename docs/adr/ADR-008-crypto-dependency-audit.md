# ADR-008: Cryptographic Dependency Audit

**Status**: Accepted  
**Date**: 2026-03-04  
**Decider**: Cipher (Agent 02 — ZK/Crypto Layer)  
**Sprint**: Sprint 1  

## Context

The Omnia Protocol relies on several cryptographic crates for core security guarantees: event signing, Merkle tree computation, state root hashing, and networking. As part of Sprint 1 hardening, we need to audit all cryptographic dependencies for version appropriateness and known vulnerabilities.

The spec mandates:
- ed25519-dalek should be 2.1+
- blake3 should be 1.5+

## Audit Results

### 1. ed25519-dalek

| Field | Value |
|-------|-------|
| **Specified** | ≥ 2.1 |
| **Cargo.toml** | `"2.1"` |
| **Resolved (Cargo.lock)** | **2.2.0** |
| **Assessment** | ✅ **Safe** |

ed25519-dalek 2.2.0 supersedes the 1.x line which had side-channel vulnerabilities (CVE-2020-12973 class). Version 2.x uses `curve25519-dalek` 4.x with constant-time operations by default. No known vulnerabilities in 2.2.0.

**Recommendation**: Keep at current version. Monitor for any future advisories on the 2.x line.

### 2. blake3

| Field | Value |
|-------|-------|
| **Specified** | ≥ 1.5 |
| **Cargo.toml** | `"1.5"` |
| **Resolved (Cargo.lock)** | **1.8.5** |
| **Assessment** | ✅ **Safe** |

BLAKE3 is a relatively new hash function with no known cryptographic vulnerabilities. The 1.8.5 release includes performance improvements and SIMD optimizations. Used for Merkle tree computation in `CausalGraph::state_root()` and `CausalGraph::merkle_proof()`, as well as batch commitment hashing in the ZK module.

**Recommendation**: Keep at current version. The 1.5+ minimum ensures AVX-512 support and the optimized SIMD backend.

### 3. sha2

| Field | Value |
|-------|-------|
| **Specified** | N/A (not specified in spec) |
| **Cargo.toml** | `"0.10"` |
| **Resolved (Cargo.lock)** | **0.10.9** |
| **Assessment** | ✅ **Safe** |

SHA-256 (via sha2 0.10.x) is used for computing event IDs in `Event::compute_hash()`. Version 0.10.x uses the `digest` 0.10 trait system and includes hardware-accelerated SHA extensions via `cpufeatures`. No known vulnerabilities. This is the current recommended version.

**Recommendation**: Keep at 0.10.x. Do not downgrade to 0.9.x (different `digest` trait, no longer maintained).

### 4. rand

| Field | Value |
|-------|-------|
| **Specified** | N/A |
| **Cargo.toml** | `"0.8"` |
| **Resolved (Cargo.lock)** | **0.8.6** (also 0.9.4 via transitive deps) |
| **Assessment** | ⚠️ **Acceptable, with note** |

rand 0.8.x is used directly in the substrate crate for `OsRng` in key generation. The rand 0.9.x entry in Cargo.lock comes from transitive dependencies (libp2p ecosystem). This dual-version situation is harmless but adds to compile time.

No known security vulnerabilities in rand 0.8.6. The 0.9.x line changed the trait system significantly; migration is non-trivial but not urgent.

**Recommendation**: Remain on 0.8.x for direct usage. Plan migration to 0.9.x in a future sprint to reduce dependency duplication.

### 5. libp2p

| Field | Value |
|-------|-------|
| **Specified** | N/A |
| **Cargo.toml** | `"0.56"` |
| **Resolved (Cargo.lock)** | **0.56.0** |
| **Assessment** | ⚠️ **Acceptable, consider upgrade** |

libp2p 0.56.0 is used for the gossip protocol (QUIC, Kademlia DHT, Gossipsub). This is not a cryptographic primitive per se, but it transports cryptographic operations (Noise protocol for encryption, multiplexing via Yamux).

No critical CVEs known for 0.56.0. The libp2p team frequently releases updates; 0.56 is a few versions behind the latest.

**Recommendation**: Consider upgrading to the latest 0.5x line in a future sprint. Ensure Noise protocol configuration uses the `XX` handshake pattern (currently default).

## Summary Table

| Crate | Cargo.toml | Resolved | Spec Met | Status |
|-------|-----------|----------|----------|--------|
| ed25519-dalek | 2.1 | 2.2.0 | ✅ ≥2.1 | ✅ Safe |
| blake3 | 1.5 | 1.8.5 | ✅ ≥1.5 | ✅ Safe |
| sha2 | 0.10 | 0.10.9 | N/A | ✅ Safe |
| rand | 0.8 | 0.8.6 | N/A | ⚠️ Acceptable |
| libp2p | 0.56 | 0.56.0 | N/A | ⚠️ Acceptable |

## Action Items

1. **No immediate changes required** — all spec requirements are met and no known vulnerabilities exist.
2. **Future sprint**: Migrate rand from 0.8.x to 0.9.x to eliminate dependency duplication.
3. **Future sprint**: Upgrade libp2p to latest 0.5x line for bug fixes and performance improvements.
4. **Ongoing**: Subscribe to RustSec advisories for all crypto crates; integrate `cargo audit` into CI when available.

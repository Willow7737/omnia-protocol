# Omnia Project Dashboard

This dashboard provides a real-time overview of the Omnia Protocol's development, contributor health, and strategic alignment. We believe in **radical transparency**: every step, from conceptualization to mainnet, is documented here.

## Current Project Status: Phase 0 — Complete | Phase 1 — Complete | Phase 2 — In Progress

| Metric | Value | Status |
| :--- | :--- | :--- |
| **Phase** | Phase 2: Hardening & Security Audit | 🔄 In Progress |
| **Overall Completion** | **92%** | Phase 2 pre-work findings identified, ADRs 010-014 added |
| **Active Contributors** | 1 (Lead) + AI Agent assistance | Needs Growth |
| **Code Health** | 295+ tests passing, 0 production `unwrap()`, 34 typed error enums | Verified |
| **Network Status** | Single-node operator | Phase 0 |
| **Security Posture** | JWT auth, ACL, rate limiting, AES-256-GCM encrypted keys | Phase 1 resolved |

### Completion Bar

```
[████████████████████████████░] 92%
```

---

## Contributor Overview

Omnia is a community-driven protocol. We track our human capital as transparently as our code.

| Role | Individual / Entity | Status |
| :--- | :--- | :--- |
| **Project Lead** | [@Willow7737](https://github.com/Willow7737) | Active |
| **Distributed Systems** | *Vacant* | Recruiting |
| **Applied Cryptography** | [@10299614-lgtm](https://github.com/10299614-lgtm) | Active but Recruiting |
| **Community & Gov** | [@Spidroidtech](https://github.com/Spidroidtech) | Active but Recruiting |
| **Protocol Engineering** | *Vacant* | Recruiting |

---

## Technical Readiness

| Component | Status | Risk Level | Notes |
| :--- | :--- | :--- | :--- |
| **Causal Consensus** | Implemented | Low | Rust, 120+ tests passing, O(new_events) processing |
| **Identity Shard** | Implemented | Low | DIDs (`did:omnia:` method), Shamir's Secret Sharing (GF(256)), BLAKE3 biometric anchors, AI agents (5 capability types), social recovery |
| **Financial Shard** | Implemented | Low | Balances with causal tracking, transfers/mint/burn, replay protection (nonce tracking with redb persistence) |
| **Computational Shard** | Implemented | Low | Task lifecycle (Submitted → Proved → Verified/Failed), proof verification stub |
| **Physical Shard** | Implemented | Low | Append-only provenance log (CRDT-friendly), ownership tracking, chain verification |
| **Biological Shard** | Implemented | Low | Consent registry (grant/revoke), ZK query stub, expiration tracking |
| **Economics Shard** | Implemented | Medium | UBC quota (1,000/month, 30-day epochs), quadratic voting with PPM decay, time-locked voting (flash loan prevention), useful-work proof (3 types: AI training, scientific simulation, distributed storage) |
| **Physical Binding** | Implemented | Medium | Provenance log full; RF/QC are stubs (need hardware) |
| **Fee Enforcement** | Implemented | Medium | `FeeSchedule` with per-domain flat fees, enforced by `ShardRouter` before dispatch |
| **UBC Economic Model** | Implemented | Medium | `QuotaSystem` with epoch advancement, `UbcToken` (soulbound, monthly reset), `GovernanceState` with fixed-point decay |
| **ZK-Rollup Settlement** | Architecture | **High** | Ethereum adapter done; Poseidon hash in ZK circuit (BLAKE3-derived round constants, **non-standard params** — see ADR-014); dummy circuit fields (see Phase 2 findings); trusted setup hash initialized to zero (stub) |
| **Encrypted Keystore** | Implemented | Low | AES-256-GCM + HKDF-SHA256 + BLAKE3 domain separation; legacy XOR migration supported (see ADR-010) |
| **VRF Leader Election** | Implemented | **Medium** | Non-standard VRF construction (Ed25519 + BLAKE3, not ECVRF — see ADR-012); functional for internal use but not spec-compliant |
| **DKG / Threshold Signatures** | Implemented | **Medium** | Feldman VSS-based DKG (see ADR-013); XOR share encryption (should be AES-256-GCM); simplified polynomial (BLAKE3-derived shares) |
| **Slashing** | Implemented | Medium | Binary slash-point model; gradual slashing designed (ADR-011) but not yet implemented |

### Shard Architecture Summary

The shards crate (`omnia-shards`) implements 6 domain shards, all conforming to the `Shard` trait defined in `shards/src/shard.rs`:

```rust
pub trait Shard: Send + Sync {
    fn shard_id(&self) -> ShardId;
    fn process_event(&mut self, event: &Event, op: ShardOp) -> Result<(), ShardError>;
    fn state_snapshot(&self) -> Vec<u8>;
    fn validate(&self, op: &ShardOp) -> Result<(), ShardError>;
}
```

| Shard | ShardId Prefix | Operations | State Type |
|-------|---------------|------------|------------|
| Financial | `"FINANCE0"` | Transfer, Mint, Burn, BalanceQuery | `FinancialState` |
| Computational | `"COMP___0"` | SubmitTask, SubmitProof, VerifyProof | `ComputationalState` |
| Physical | `"PHYS___0"` | AnchorItem, TransferOwnership, VerifyChain | `PhysicalState` |
| Biological | `"BIO____0"` | GrantAccess, RevokeAccess, QueryWithZkProof | `BiologicalState` |
| Identity | `"IDENT__0"` | CreateDid, UpdateDid, RecoverDid, VerifyDid, AddAgent, EnrollBiometric, VerifyBiometric, RevokeAgent, ConfigureRecovery | `IdentityState` |
| Economics | `"ECONOM00"` | MintUbc, SpendUbc, SubmitWork, CreateProposal, Vote, RegisterDid, AdvanceEpoch | `EconomicsShardState` |

### Economics Layer Summary

The economics crate (`omnia-economics`) implements Layer 5 with these core modules:

| Module | Key Types | File |
|--------|-----------|------|
| UBC Token | `UbcToken` (soulbound, monthly reset) | `economics/src/ubc.rs` |
| Quota System | `QuotaSystem` (1,000 UBC/month, 30-day epochs) | `economics/src/quota.rs` |
| Governance | `GovernanceState`, `Proposal`, `VoteChoice` (For/Against/Abstain) | `economics/src/governance.rs` |
| Fixed-Point | `DecayRate`, `BasisPpmExt`, `isqrt()` (no f64) | `economics/src/fixed_point.rs` |
| Useful Work | `UsefulWorkProof`, `UsefulWorkType` (3 variants) | `economics/src/useful_work.rs` |
| Time Lock | `TimeLockVoting`, `LockedStake`, `TimeLockConfig` | `economics/src/time_lock.rs` |
| Economics Shard | `EconomicsState`, `EconomicsOp` | `economics/src/economics_shard.rs` |

---

## Phase 2 Work Items

| # | Item | Status | ADR | Notes |
|---|------|--------|-----|-------|
| M-4 | Architecture Decision Records 010-014 | ✅ Complete | ADR-010–014 | Encrypted keystore, gradual slashing, VRF construction, DKG selection, Poseidon migration |
| M-5 | Project dashboard & findings update | ✅ Complete | — | Dashboard updated, PHASE_2_FINDINGS.md created |
| — | Gradual slashing implementation (ADR-011) | 🔄 Planned | ADR-011 | 3-tier: Warning → Jail → Ejection; replace binary slash-point model |
| — | DKG share encryption upgrade | 🔄 Planned | ADR-013 | Replace XOR with AES-256-GCM for DKG share packages |
| — | VRF spec compliance (ADR-012) | 📋 Deferred | ADR-012 | Migrate to ECVRF (RFC 9381) or formally justify current construction |
| — | Poseidon parameter migration (ADR-014) | 📋 Deferred | ADR-014 | Dual-hash transition to Filecoin/Neptune reference constants |
| — | ZK circuit dummy field remediation | 🔄 Planned | — | Replace `Fr::zero()` placeholder witnesses in `ExpandedRollupCircuit::empty()` |
| — | Trusted setup transcript hash initialization | 🔄 Planned | — | Replace `[0u8; 32]` initial transcript hash with proper initial state hash |
| — | Identity recovery TODO in state.rs | 🔄 Planned | — | `TODO: Recovery should add the new recovered key to authentication` |
| — | Sybil resistance / stake-weighted validator registry | 📋 Deferred | — | Phase 2 scope from PHASE_1_SUMMARY.md |
| — | Causal graph GC with pruning | 📋 Deferred | — | Phase 2 scope from PHASE_1_SUMMARY.md |
| — | Multi-party trusted setup ceremony | 📋 Deferred | — | Phase 2 scope from PHASE_1_SUMMARY.md |
| — | RF fingerprint hardware integration | 📋 Deferred | — | Requires SDR hardware |
| — | Full rustdoc coverage (`#![deny(missing_docs)]`) | 📋 Deferred | — | Phase 2 scope from PHASE_1_SUMMARY.md |

---

## Roadmap and Milestones

### Phase 0: The Seed (Months 0-6) — ✅ Complete
- [x] Architecture Whitepaper (v1.0)
- [x] Causal Graph Consensus Prototype (Rust, 120+ tests)
- [x] DID Registry Implementation (`did:omnia:` method with hex-encoded 32-byte public keys)
- [x] Domain Shard Implementation (6 shards, extensive test suite including adversarial tests)
- [x] Binding Layer Implementation (provenance log, RF/QC stubs)
- [x] Economics Layer (UBC, quadratic voting with PPM decay, time-locked voting, useful-work proofs)
- [x] ZK-Rollup Settlement Architecture (Ethereum adapter)
- [x] Fee enforcement (FeeSchedule + ShardRouter integration)
- [x] Replay protection (nonce tracking with redb persistence)
- [x] AI agent identity (5 capability types including governance)
- [x] Shamir's Secret Sharing social recovery (GF(256) with Lagrange interpolation)
- [x] Biometric anchors (BLAKE3 salted commitments, raw template never stored)
- [x] Full ZK Circuit (arkworks R1CS + Groth16 + Poseidon hash)
- [x] Real PQC Signatures (Dilithium integration with real verification)
- [x] Local Testnet (5 Nodes via Docker Compose)
- [x] REST API (9 endpoints with JWT auth, ACL, rate limiting, CORS)
- [x] Encrypted key storage (AES-256-GCM + HKDF-SHA256)
- [x] Governance quorum + time-lock (flash-loan prevention)
- [x] Gossip payload limits (MAX_PAYLOAD_SIZE enforcement)
- [x] BLAKE3 domain separation (context-specific hashing)
- [x] Creator-pubkey binding (constant-time validation via subtle crate)

### Phase 1: The Sprout (Months 6-12) — ✅ Complete
- [x] Typed error migration (34 `thiserror` enums, zero `Result<_, String>`)
- [x] `unwrap()` replacement (`#![deny(clippy::unwrap_used)]` on all 7 crates)
- [x] E2E REST API Integration Tests (19 test functions, 4 auth states)
- [x] Code Coverage Integration (`cargo llvm-cov`, 70% target)
- [x] RUSTSEC Advisory Review (removed stale RUSTSEC-2025-0055, added review dates)
- [x] Documentation Sprint (50+ discrepancy fixes across 12 files)
- [x] Solidity Groth16 Verifier (production-quality, BN254 precompiles)
- [x] Rustdoc Coverage (35 items added to 7 security-critical modules)

### Phase 2: Hardening & Security Audit (Months 12-18) — 🔄 In Progress
- [ ] Gradual slashing implementation (ADR-011)
- [ ] DKG share encryption upgrade (XOR → AES-256-GCM)
- [ ] ZK circuit dummy field remediation
- [ ] Trusted setup transcript hash initialization
- [ ] Identity recovery TODO resolution
- [ ] VRF spec compliance evaluation (ADR-012)
- [ ] Poseidon parameter migration planning (ADR-014)
- [x] Architecture Decision Records 010-014 (M-4)
- [x] Project dashboard & findings documentation (M-5)
- [ ] Full ZK-Rollup Integration
- [ ] Fee Mechanism Design (dynamic fees, EIP-1559-style)
- [ ] Validator Network (multi-node)
- [ ] Public Testnet Launch
- [ ] Community Governance Genesis

---

## Architecture Decision Records

| ADR | Title | Date | Status |
|-----|-------|------|--------|
| ADR-001 | Event Processor Trait | 2026-05-10 | Implemented |
| ADR-002 | Settlement Layer Trait | 2026-05-10 | Implemented |
| ADR-003 | Gossip Substrate Interface | 2026-05-10 | Implemented |
| ADR-005 | ProofBundle Universal Proof Format | 2026-05-14 | Implemented |
| ADR-006 | Shard Trait Contract | 2026-05-10 | Implemented |
| ADR-007 | Binding Shard Interface | 2026-05-10 | Implemented |
| ADR-008 | Crypto Dependency Audit | 2026-05-10 | Accepted |
| ADR-009 | Poseidon Parameter Justification | 2026-05-14 | Accepted |
| ADR-010 | Encrypted Keystore Design | 2026-05-18 | Accepted |
| ADR-011 | Gradual Slashing Model | 2026-05-18 | Accepted |
| ADR-012 | VRF Construction Choice | 2026-05-18 | Accepted |
| ADR-013 | DKG Protocol Selection | 2026-05-18 | Accepted |
| ADR-014 | Poseidon Parameter Migration Strategy | 2026-05-18 | Accepted |

---

## Transparency Log
*Every major decision and status change is logged here.*

- **2026-05-18:** Phase 2 pre-work: Created ADRs 010-014 (encrypted keystore design, gradual slashing model, VRF construction choice, DKG protocol selection, Poseidon parameter migration strategy). Updated project dashboard. Documented Phase 2 findings (PHASE_2_FINDINGS.md).
- **2026-05-18:** Phase 1 complete. All 8 deliverables verified: typed errors (34 enums), `unwrap()` elimination, E2E API tests (19 functions), code coverage integration, RUSTSEC review, documentation sprint (50+ fixes), Groth16 verifier, rustdoc coverage (35 items).
- **2026-03-05:** Phase 0 complete. All critical and high-severity security findings resolved (JWT auth FIND-001, ACL FIND-002, creator binding FIND-003, encrypted keys FIND-010, slashing rollback FIND-011, governance quorum FIND-020, gossip limits FIND-021, BLAKE3 domain FIND-022). 295+ tests, 7 fuzz targets, TLA+ verified. Documentation sprint (FIND-034) resolved 42 discrepancies across 10 documentation files.
- **2026-05-14:** Documentation audit (Task 3-b): All docs updated to match actual codebase. Corrected fee schedule descriptions, epoch duration (30 days not 24 hours), ShardError variant count (7 not 6), time-locked voting status (implemented not planned), DID format (hex not multi-base), fee mechanism status (implemented not "not started"). Added code references and type signatures.
- **2026-05-13:** Documentation overhaul — all docs updated to match actual codebase. Honest labeling of stubs, planned features, and aspirational content. Visual design preserved.
- **2026-05-10:** Project Dashboard initialized. Radical transparency policy enacted.
- **2026-05-10:** Genesis Architecture documents finalized and uploaded.
- **2026-05-10:** Project ownership confirmed under CC0 License.

---
*Last Updated: 2025-05-18*
*Version: 5.0.0*

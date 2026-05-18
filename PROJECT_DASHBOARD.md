# Omnia Project Dashboard

This dashboard provides a real-time overview of the Omnia Protocol's development, contributor health, and strategic alignment. We believe in **radical transparency**: every step, from conceptualization to mainnet, is documented here.

## Current Project Status: Phase 0 — Active Development

| Metric | Value | Status |
| :--- | :--- | :--- |
| **Phase** | Phase 0: The Seed | In Progress |
| **Overall Completion** | **85%** | All layers scaffolded |
| **Active Contributors** | 1 (Lead) + AI Agent assistance | Needs Growth |
| **Code Health** | 200+ tests passing | Verified |
| **Network Status** | Single-node operator | Phase 0 |

### Completion Bar

```
[████████████████████████░░░░] 85%
```

---

## Contributor Overview

Omnia is a community-driven protocol. We track our human capital as transparently as our code.

| Role | Individual / Entity | Status |
| :--- | :--- | :--- |
| **Project Lead** | [@Willow7737](https://github.com/Willow7737) | Active |
| **Distributed Systems** | *Vacant* | Recruiting |
| **Applied Cryptography** | [@10299614-lgtm](https://github.com/10299614-lgtm)10299614-lgtm | Recruiting |
| **Protocol Engineering** | *Vacant* | Recruiting |
| **Community & Gov** | *Vacant* | Recruiting |

---

## Technical Readiness

| Component | Status | Risk Level | Notes |
| :--- | :--- | :--- | :--- |
| **Causal Consensus** | Implemented | Low | Rust, 75+ tests passing, O(new_events) processing |
| **Identity Shard** | Implemented | Low | DIDs (`did:omnia:` method), Shamir's Secret Sharing (GF(256)), BLAKE3 biometric anchors, AI agents (5 capability types), social recovery |
| **Financial Shard** | Implemented | Low | Balances with causal tracking, transfers/mint/burn, replay protection (nonce tracking with redb persistence) |
| **Computational Shard** | Implemented | Low | Task lifecycle (Submitted → Proved → Verified/Failed), proof verification stub |
| **Physical Shard** | Implemented | Low | Append-only provenance log (CRDT-friendly), ownership tracking, chain verification |
| **Biological Shard** | Implemented | Low | Consent registry (grant/revoke), ZK query stub, expiration tracking |
| **Economics Shard** | Implemented | Medium | UBC quota (1,000/month, 30-day epochs), quadratic voting with PPM decay, time-locked voting (flash loan prevention), useful-work proof (3 types: AI training, scientific simulation, distributed storage) |
| **Physical Binding** | Implemented | Medium | Provenance log full; RF/QC are stubs (need hardware) |
| **Fee Enforcement** | Implemented | Medium | `FeeSchedule` with per-domain flat fees, enforced by `ShardRouter` before dispatch |
| **UBC Economic Model** | Implemented | Medium | `QuotaSystem` with epoch advancement, `UbcToken` (soulbound, monthly reset), `GovernanceState` with fixed-point decay |
| **ZK-Rollup Settlement** | Architecture | Medium | Ethereum adapter done; expanded ZK circuit with Merkle path verification (hash gadget placeholder) |

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

## Roadmap and Milestones

### Phase 0: The Seed (Months 0-6)
- [x] Architecture Whitepaper (v1.0)
- [x] Causal Graph Consensus Prototype (Rust, 75+ tests)
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
- [ ] Full ZK Circuit (arkworks R1CS)
- [ ] Real PQC Signatures (Dilithium integration)
- [ ] Local Testnet (5 Nodes)

### Phase 1: The Sprout (Months 6-12)
- [ ] Full ZK-Rollup Integration
- [ ] Real RF Fingerprinting (SDR hardware)
- [ ] Fee Mechanism Design (dynamic fees, EIP-1559-style)
- [ ] Mobile Wallet
- [ ] Validator Network (multi-node)
- [ ] Public Testnet Launch
- [ ] Community Governance Genesis

---

## Transparency Log
*Every major decision and status change is logged here.*

- **2026-05-14:** Documentation audit (Task 3-b): All docs updated to match actual codebase. Corrected fee schedule descriptions, epoch duration (30 days not 24 hours), ShardError variant count (7 not 6), time-locked voting status (implemented not planned), DID format (hex not multi-base), fee mechanism status (implemented not "not started"). Added code references and type signatures.
- **2026-05-13:** Documentation overhaul — all docs updated to match actual codebase. Honest labeling of stubs, planned features, and aspirational content. Visual design preserved.
- **2026-05-10:** Project Dashboard initialized. Radical transparency policy enacted.
- **2026-05-10:** Genesis Architecture documents finalized and uploaded.
- **2026-05-10:** Project ownership confirmed under CC0 License.

---
*Last Updated: 2026-05-14*
*Version: 4.0.0*

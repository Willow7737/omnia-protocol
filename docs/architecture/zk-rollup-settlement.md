# ZK-Rollup Settlement Layer (Phase 0)
> 🎯 Audience: Developers
> 🔗 Context: Phase 0 implements settlement-agnostic ZK-rollup architecture for bridging to L1 chains
> 📅 Last Updated: 2026-05-20

## Overview

Phase 0 implements the ZK-rollup settlement layer, providing a settlement-agnostic architecture that can bridge to any L1 chain with data availability and proof verification capabilities.

## Settlement-Agnostic Architecture

The `SettlementLayer` trait (`omnia-adapters/src/settlement/mod.rs`) defines the interface for all settlement adapters:

```rust
pub trait SettlementLayer {
    fn post_batch(&self, batch: &RollupBatch) -> Result<SettlementReceipt, SettlementError>;
    fn verify_proof(&self, receipt: &SettlementReceipt) -> Result<bool, SettlementError>;
    fn latest_state_root(&self) -> Result<[u8; 32], SettlementError>;
    fn deposit(&self, deposit: &Deposit) -> Result<DepositReceipt, SettlementError>;
    fn request_withdrawal(&self, withdrawal: &Withdrawal) -> Result<WithdrawalReceipt, SettlementError>;
}
```

## Ethereum Adapter — ✅ Implemented

- **Simulated mode** (default): Full architecture, in-memory state transitions
- **Live mode** (feature flag `ethereum-live`): Real RPC via `alloy` v1 with Anvil/Hardhat integration tests
- Solidity contract: `OmniaRollup.sol` with Groth16 verification using EIP-196/197 BN254 precompiles
- ABI: `omnia-adapters/contracts/ethereum/OmniaRollup.json`
- Config validation: URL scheme, contract address format, operator key format
- BLAKE3 domain-separated batch data hashing (`OMNIA-ETH-BATCH-DATA`)
- Gas estimation and confirmation waiting with configurable `confirmation_blocks`

Located in: `omnia-adapters/src/settlement/ethereum/`

## Other Adapters — Stubs

Bitcoin, Solana, and Celestia adapters are stubs that return `SettlementError::NotImplemented`.

Located in: `omnia-adapters/src/settlement/`

## ZK Circuit — ✅ Implemented

### Groth16 Proof System

- arkworks R1CS + Groth16 on BN254 curve
- `RollupCircuit` — basic rollup circuit
- `ExpandedRollupCircuit` — Merkle path verification + per-event state transition constraints
- `ExpandedRollupCircuit::for_setup()` — uses non-zero witness fields for trusted setup key generation

Located in: `omnia-adapters/src/circuit.rs`, `omnia-adapters/src/proof.rs`

### Poseidon Hash

- SNARK-friendly hash (BN254, t=3, R_F=8, R_P=57)
- Dual-hash infrastructure: `PoseidonVersion::Custom` (default, deprecated) and `PoseidonVersion::Reference` (target)
- Custom: Cauchy MDS + BLAKE3-derived round constants
- Reference: Deterministic parameters with distinct Cauchy MDS construction and BLAKE3 domain `"Poseidon-Ref-BN254-t3-RF8-RP57"`
- ⚠️ Parameters use Cauchy MDS matrix and BLAKE3-derived round constants (not Grain LFSR from Filecoin/Neptune paper)

Located in: `omnia-adapters/src/poseidon.rs`

See [ADR-014](../reference/adr-index.md#adr-014-poseidon-parameter-migration) for migration strategy.

### Merkle Tree

- Sparse Merkle tree with BLAKE3 off-circuit leaf hashing
- `build_merkle_tree()`, `compute_root_from_proof()`, `merkle_proof()`
- On-circuit compatible: `poseidon_hash_to_fr()` for ZK-friendly alternative

Located in: `omnia-adapters/src/merkle.rs`

### Operation Type Constraints

- `OperationType` enum: Transfer, Mint, Burn, AnchorItem, QueryWithZkProof, SubmitTask, Govern, IdentityUpdate (8 values)
- Bit decomposition constraints enforcing `operation_type ∈ [0, 7]`
- `payload_hash` constraint: `payload_hash == Poseidon(event_hash, operation_type)`

### Batch Verification

- `verify_proofs_batch()` for efficient multi-proof verification
- BLAKE3 domain separation (`OMNIA-BATCH-VRFY-V1`) for random scalar derivation
- Handles empty batches, single proofs, and multi-proof batches

## Trusted Setup Ceremony — ✅ Implemented

### Powers of Tau

- `PowersOfTau` SRS initialized with generator points (not identity)
- Transcript hash initialized with BLAKE3 domain separation (`OMNIA-SETUP-TRANSCRIPT-V1`)
- Real EC operations with BN254 G1 scalar multiplication
- Fiat-Shamir Proof of Knowledge on BN254 G1

### Ceremony Server/Client

- `CeremonyServer` coordinator with lifecycle: `NotStarted` → `AcceptingContributions` → `Finalized`
- `CeremonyClient` for generating and verifying contributions
- CLI subcommands: `setup-contribute`, `setup-verify`
- HTTP API stubs: `/ceremony/state`, `/ceremony/contribute`, `/ceremony/transcript`, `/ceremony/finalize`
- `export_transcript()` for independent third-party verification

Located in: `omnia-adapters/src/setup/`

## L2 Operator

The L2 operator with batch builder processes events into settlement batches.

Located in: `omnia-adapters/src/operator.rs`

## Settlement Architecture Diagram

```
┌───────────────────────────────────────────────┐
│            Settlement-Agnostic ZK-Rollup      │
├───────────────┬───────────────┬───────────────┤
│  Ethereum ✅  │ Bitcoin 🔄    │  Solana 🔄    │
│  (OmniaRollup │  (stub)       │  (stub)       │
│   .sol)       │               │               │
├───────────────┴───────────────┴───────────────┤
│           SettlementLayer Trait                │
├───────────────────────────────────────────────┤
│         L2 Operator + Batch Builder           │
├───────────────────────────────────────────────┤
│      ZK Circuit (arkworks R1CS + Groth16) ✅  │
├───────────────────────────────────────────────┤
│    Merkle State Root + Inclusion Proofs        │
├───────────────────────────────────────────────┤
│         Event Pruning (sustainability)         │
└───────────────────────────────────────────────┘
```

---
🔙 **Back**: [architecture/](./) | 🔄 **Related**: [pipeline-design.md](./pipeline-design.md)
🚀 **Next**: [trait-boundaries.md](./trait-boundaries.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

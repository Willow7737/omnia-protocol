# ZK-Rollup Settlement Layer (Phase 0)

> 🎯 Audience: Developers
> 🔗 Context: Phase 0 implements settlement-agnostic ZK-rollup architecture for bridging to L1 chains
> 📅 Last Updated: 2026-05-20

## Overview

Phase 0 implements the ZK-rollup settlement layer, providing a settlement-agnostic architecture that can bridge to any L1 chain with data availability and proof verification capabilities.

## Settlement-Agnostic Architecture

The protocol provides **two** settlement trait hierarchies:

### New: `SettlementAdapter` (hybrid architecture)

The `SettlementAdapter` trait (`omnia-adapters/src/settlement/mod.rs`) is the primary trait for the hybrid settlement architecture. Core protocol depends ONLY on this trait — zero alloy, zero MSRV conflict:

```rust
#[async_trait]
pub trait SettlementAdapter: Send + Sync {
    async fn submit_root(&self, root: [u8; 32]) -> Result<TxHash, SettlementError>;
    async fn fetch_finality(&self, tx: TxHash) -> Result<FinalityProof, SettlementError>;
    async fn verify_inclusion(&self, proof: &MerkleProof, leaf: &[u8; 32]) -> Result<bool, SettlementError>;
    fn is_live(&self) -> bool { false }
}
```

Implementations:

- **MockSettlementAdapter** — always available, deterministic BLAKE3-based responses
- **EthereumSettlementAdapter** — feature-gated behind `ethereum-live`, real RPC via Alloy
- **FfiSettlementAdapter** — feature-gated behind `settlement-ffi`, C library integration

### Legacy: `SettlementLayer` (backward-compatible)

The original `SettlementLayer` trait with full adapter methods (post_batch, verify_proof, deposit, withdrawal, etc.). Existing code using this trait continues to work unchanged.

```rust
#[async_trait]
pub trait SettlementLayer: Send + Sync {
    fn chain_id(&self) -> &'static str;
    async fn post_batch(&self, batch_data: &[u8]) -> Result<String, SettlementError>;
    async fn verify_proof(&self, old_root: &[u8; 32], new_root: &[u8; 32], proof: &[u8]) -> Result<bool, SettlementError>;
    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError>;
    async fn deposit(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError>;
    async fn request_withdrawal(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError>;
    async fn submit_batch(&self, bundle: &ProofBundle) -> Result<String, SettlementError>;
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

### v0.1.69 Security Fix: verify_proof_with_root

The `SettlementAdapter::verify_proof` trait method now fails-closed in Ethereum live mode. It cannot safely derive `batch_merkle_root` from the prover's own proof bytes (a malicious prover could craft proof bytes whose offset 192..224 matched the on-chain root). Use `EthereumAdapter::verify_proof_with_root()` instead, which requires the `batch_merkle_root` as a trusted parameter fetched from the on-chain event log. See `omnia-adapters/src/settlement/ethereum/mod.rs`.

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

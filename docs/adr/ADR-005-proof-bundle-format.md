# ADR-005: ProofBundle Universal Proof Format

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-08-11

**Status**: Implemented
**Date**: 2026-05-14
**Updated**: 2026-05-16
**Decision**: Define a universal `ProofBundle` format that encapsulates all data needed for cross-chain state transition verification, with forward-compatible versioning and structured L1 anchoring.

## Context

The Omnia ZK-rollup must post state transition proofs to L1 settlement layers. Each proof must contain enough information for an L1 verifier (smart contract, program, or script) to independently verify that a state transition is valid, without trusting the operator.

The proof bundle must satisfy several requirements:

1. **Chain-agnostic structure.** The same proof bundle must be usable on Ethereum, Bitcoin, Solana, Celestia, or any future L1. The bundle's logical structure is defined once; only the encoding changes per chain.
2. **Forward compatibility.** As the protocol evolves (Phase 0 stubs → Groth16 proofs → future proof systems), the bundle format must support versioned upgrades without breaking existing verifiers.
3. **State transition verification.** The bundle must contain both the previous and new state roots so that the verifier can confirm the transition.
4. **Data availability.** The bundle must commit to the batch data via a Merkle root, enabling light clients to verify inclusion without downloading the full batch.
5. **Cross-chain anchor.** The bundle must reference an L1 anchor (chain ID, block height, timestamp) for cross-chain verification.

The proof generation infrastructure is in `zk/src/prover.rs`, the settlement adapters in `zk/src/settlement/`, and the ZK circuit in `zk/src/circuit.rs`.

## Decision

### ProofBundle Structure

We define the following structure for a proof bundle (as implemented in `zk/src/proof_bundle.rs`):

```rust
/// Current format version for ProofBundle.
pub const PROOF_BUNDLE_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofBundle {
    /// Format version for forward compatibility (currently 1).
    pub version: u16,

    /// BLAKE3 hash of the state after applying this batch.
    pub state_root: [u8; 32],

    /// BLAKE3 hash of the state before this batch.
    pub prev_state_root: [u8; 32],

    /// Serialized ZK proof bytes (Groth16 proof in production).
    pub transition_proof: Vec<u8>,

    /// BLAKE3 Merkle root of all events in this batch.
    pub batch_merkle_root: [u8; 32],

    /// L1 anchor data for cross-chain verification.
    pub l1_anchor: L1Anchor,
}
```

### L1Anchor Structure

The L1 anchor is a structured type rather than a raw 32-byte array, enabling
chain-specific identification and verification:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L1Anchor {
    /// EIP-155 style chain ID (1 = Ethereum mainnet, etc.)
    pub chain_id: u64,

    /// L1 block height at time of anchoring.
    pub block_height: u64,

    /// L1 block timestamp (milliseconds since epoch).
    pub timestamp: u64,
}
```

The `L1Anchor` type provides convenience methods:

- `is_ethereum()` — returns `true` if `chain_id == 1` (Ethereum mainnet)
- `is_bitcoin()` — returns `true` if `chain_id` matches known Bitcoin chain IDs
  (`0x80000000`, `0x02000000`, `0x01000000`)

### Field Justifications

**Version field.** The `version: u16` field enables forward compatibility. When a new proof system is introduced, the version is incremented. L1 verifiers check the version and reject bundles with unsupported versions. This prevents ambiguity: a verifier encountering an unsupported version can cleanly reject it rather than attempting to parse incompatible bytes. The current version is 1 (`PROOF_BUNDLE_VERSION`), corresponding to the Groth16 proof era.

**State roots (prev + new).** The `prev_state_root` must match the last confirmed root on L1 (obtained via `SettlementLayer::latest_state_root()`). The `state_root` is computed by `CausalGraph::state_root()` after inserting all events in the batch. The L1 verifier checks that `prev_state_root` matches its stored root, then verifies the transition proof to confirm that `state_root` is correct. Note that `verify_integrity()` rejects bundles where `state_root == prev_state_root` (no-op batches are not allowed).

**Transition proof bytes.** The current production proof system is Groth16 on Bn254. The `RollupOperator` (`zk/src/operator.rs`) generates real Groth16 proofs:

```rust
// In operator.rs::generate_proof():
let circuit = RollupCircuit::from_state_roots(*old_root, *new_root, event_count as u64);
let proof_obj = prover::create_proof(circuit, &cache.proving_key)?;
let proof_bytes = prover::serialize_proof(&proof_obj)?;
```

The `ExpandedRollupCircuit` (`zk/src/circuit.rs`) also supports Groth16 proofs with Merkle path verification and Poseidon hash constraints, using `prover::create_expanded_proof()`.

Legacy stub functions for Phase 0 testing are retained:

- `proof::generate_dummy_proof()` returns `vec![0xBB; 192]` (test-only)
- `proof::verify_stub_proof()` checks non-empty and >= 32 bytes (test-only)
- `proof::compute_batch_commitment()` produces a BLAKE3 hash for batch commitment

**Batch Merkle root.** This is the root of a Merkle tree built over the batch's events. It enables data availability proofs: a light client can verify that a specific event is included in the batch by checking a Merkle proof against this root. The tree is built in `zk/src/merkle.rs::build_merkle_tree()` using BLAKE3 for off-circuit hashing and `poseidon_hash_to_fr()` for on-circuit-compatible trees.

**L1 anchor.** The `L1Anchor` struct ties the L2 proof to a specific L1 block, preventing replay attacks. A proof created at L1 block N cannot be replayed at L1 block M > N because the anchor won't match. The anchor includes:

- `chain_id: u64` — EIP-155 style chain identifier. Ethereum mainnet is 1. Bitcoin chain IDs use SLIP-44 mapped values (`0x80000000`, `0x02000000`, `0x01000000`).
- `block_height: u64` — The L1 block number at which the proof was anchored.
- `timestamp: u64` — L1 block timestamp in milliseconds since epoch.

### Integrity Verification

The `ProofBundle::verify_integrity()` method performs three checks:

1. **Version check**: `version == PROOF_BUNDLE_VERSION` (currently 1)
2. **Non-empty proof**: `!transition_proof.is_empty()`
3. **State transition**: `state_root != prev_state_root` (rejects no-op batches)

```rust
pub fn verify_integrity(&self) -> Result<(), ProofBundleError> {
    if self.version != PROOF_BUNDLE_VERSION {
        return Err(ProofBundleError::InvalidVersion(self.version));
    }
    if self.transition_proof.is_empty() {
        return Err(ProofBundleError::EmptyProof);
    }
    if self.state_root == self.prev_state_root {
        return Err(ProofBundleError::SameStateRoots);
    }
    Ok(())
}
```

### Serialization Strategy

**Internal serialization: postcard.** Within the Omnia L2 node, `ProofBundle` is serialized using `postcard` for efficiency and determinism. Postcard is a `no_std`-compatible, compact binary serialization format that produces deterministic output — the same data always produces the same byte sequence, which is critical for consensus reproducibility and cross-node state root agreement. Serialization and deserialization are provided via `to_allocvec()` and `from_bytes()` methods.

**Per-chain encoder for L1.** Each `SettlementLayer` adapter is responsible for encoding the `ProofBundle` into a format that its L1 can process:

- **Ethereum**: ABI-encoded calldata for the `OmniaRollup.sol` contract's `submitBatch()` function.
- **Bitcoin**: OP_RETURN script data (limited to 80 bytes, so only the state roots and Merkle root are included).
- **Solana**: Borsh-serialized account data.
- **Celestia**: Namespace-prefixed blob data.

This dual serialization strategy means the `ProofBundle` struct itself is chain-agnostic, while each adapter handles the L1-specific encoding. No chain-specific logic leaks into the core proof generation code.

### Error Types

`ProofBundleError` covers the failure modes:

- `InvalidVersion(u16)` — Unsupported format version
- `EmptyProof` — The transition proof is empty
- `SameStateRoots` — `prev_state_root` and `state_root` are identical
- `SerializationError(String)` — postcard serialization/deserialization failure
- `IntegrityError(String)` — General integrity check failure

### How This Format Enables Multi-Chain Settlement

The `ProofBundle` contains all the information needed to verify a state transition on any chain:

1. **What was the old state?** → `prev_state_root`
2. **What is the new state?** → `state_root`
3. **Prove the transition is valid** → `transition_proof`
4. **Prove the data is available** → `batch_merkle_root`
5. **When and where did this happen on L1?** → `l1_anchor` (chain_id + block_height + timestamp)

Any L1 verifier that can check these five things can settle Omnia state transitions. Adding a new L1 chain only requires implementing the `SettlementLayer` trait and an encoder for the `ProofBundle` — the proof generation logic remains unchanged.

## Consequences

- **Positive**: One proof format serves all chains. Adding a new L1 adapter requires no changes to proof generation.
- **Positive**: Version field enables smooth upgrades from stubs to Groth16 proofs and beyond.
- **Positive**: `batch_merkle_root` enables light client support and data availability verification without full batch download.
- **Positive**: Structured `L1Anchor` with chain ID, block height, and timestamp provides richer cross-chain verification than a raw 32-byte hash.
- **Positive**: `verify_integrity()` method allows pre-submission validation of bundles.
- **Negative**: The `transition_proof: Vec<u8>` is untyped — the verifier must know the proof system from the `version` field. Misaligned version/proof-type pairs would cause verification failures.
- **Negative**: The `L1Anchor` uses a fixed EIP-155 chain ID model. Chains without EIP-155 chain IDs need a mapping convention.
- **Trade-off**: The `L1Anchor` is a structured type rather than a fixed 32 bytes, which is more flexible but requires each adapter to understand the structure.

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

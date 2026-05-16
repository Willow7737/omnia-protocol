# Task 1: Implement Groth16 Verification in OmniaRollup.sol

## Agent: Groth16 Verifier Implementation
## Status: COMPLETED

## Summary

Replaced the stub `verifyProof` function in `zk/contracts/ethereum/OmniaRollup.sol` with a full Groth16 verification implementation using BN254 (alt_bn128) pairing precompiles.

## Changes Made

### 1. Full Groth16 Verification Implementation

- **Replaced** the Phase 0 stub `verifyProof(bytes calldata _proof) -> bool` that only checked `_proof.length > 0`
- **Implemented** the complete Groth16 pairing check: `e(-A, B) * e(alpha, beta) * e(accum, gamma) * e(C, delta) = 1`
- **Added** accumulator computation: `accum = IC[0] + sum(publicInput[i] * IC[i+1])`
- **Uses** EIP-197 pairing precompile (0x08) for the 4-pair pairing check
- **Uses** EIP-196 ecMul precompile (0x07) for scalar multiplication on G1
- **Uses** EIP-196 ecAdd precompile (0x06) for point addition on G1
- **Correctly encodes** G2 points for EIP-197 (imaginary part first, then real part)

### 2. Structs Added

- `G1Point` — Point on G1 subgroup (x, y as uint256)
- `G2Point` — Point on G2 subgroup (x, y as Fp2 elements: [c0, c1] where c0=real, c1=imaginary)

### 3. Verifying Key Storage

- `vkAlpha` (G1Point) — Alpha point from verifying key
- `vkBeta` (G2Point) — Beta point from verifying key
- `vkGamma` (G2Point) — Gamma point from verifying key
- `vkDelta` (G2Point) — Delta point from verifying key
- `vkIC` (G1Point[]) — IC commitments (length = numPublicInputs + 1)
- All set in constructor, immutable afterwards
- `vkICLength()` view helper added

### 4. Constructor Updated

New constructor signature:
```solidity
constructor(
    address _operator,
    bytes32 _initialStateRoot,
    uint256[2] memory _alpha,       // G1: [x, y]
    uint256[2][2] memory _beta,     // G2: [[x.c0, x.c1], [y.c0, y.c1]]
    uint256[2][2] memory _gamma,    // G2
    uint256[2][2] memory _delta,    // G2
    uint256[] memory _ic            // Flat: [IC0.x, IC0.y, IC1.x, IC1.y, ...]
)
```

### 5. submitBatch Updated

New signature accepts structured proof parameters:
```solidity
function submitBatch(
    bytes32 _newStateRoot,
    uint256[2] calldata _proofA,
    uint256[2][2] calldata _proofB,
    uint256[2] calldata _proofC,
    uint256[] calldata _publicInputs,
    bytes calldata _batchData
) external onlyOperator
```

### 6. Reentrancy Fix in finalizeWithdrawal

**Before (vulnerable):**
```solidity
deposits[w.l2Did] -= w.amount;
payable(msg.sender).transfer(w.amount);  // External call before state clear
emit WithdrawalFinalized(msg.sender, w.amount);
```

**After (Checks-Effects-Interactions pattern):**
```solidity
// Save to memory
bytes32 l2Did = w.l2Did;
uint256 amount = w.amount;
// Clear state BEFORE external call
w.l2Did = bytes32(0);
w.amount = 0;
w.requestedAt = 0;
deposits[l2Did] -= amount;
// External call LAST
payable(msg.sender).transfer(amount);
emit WithdrawalFinalized(msg.sender, amount);
```

### 7. Comprehensive Documentation

- Contract-level NatSpec documenting proof format, VK format, verification equation, and public inputs
- Proof byte layout table matching `ark_groth16::Proof<Bn254>` uncompressed serialization
- VK byte layout table matching `ark_groth16::VerifyingKey<Bn254>` uncompressed serialization
- G2 encoding convention documentation (c0=real, c1=imaginary; EIP-197 swap order)
- Public input documentation for both `RollupCircuit` (1 input) and `ExpandedRollupCircuit` (3 inputs)

## Rust-Side Format Compatibility

The Rust prover (`zk/src/prover.rs`) uses:
- `ark_groth16::Proof<Bn254>` with `serialize_uncompressed` → 256 bytes
- `ark_groth16::VerifyingKey<Bn254>` with `serialize_uncompressed`
- Fp2 elements serialized as c0 || c1 (real || imaginary)

The Solidity contract's parameter convention matches:
- `_proofA = [A.x, A.y]`
- `_proofB = [[B.x.c0, B.x.c1], [B.y.c0, B.y.c1]]`
- `_proofC = [C.x, C.y]`

When encoding for the EIP-197 pairing precompile, G2 elements are reordered to (c1, c0) — imaginary part first, per the specification.

## Preserved Functionality

- `deposit()` — unchanged
- `requestWithdrawal()` — unchanged
- All 4 events — unchanged
- `onlyOperator` modifier — unchanged
- State variables (stateRoot, operator, batchIndex, deposits, withdrawals) — unchanged

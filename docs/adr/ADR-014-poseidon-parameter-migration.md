# ADR-014: Poseidon Parameter Migration Strategy

## Status

Accepted

## Date

2025-05-18

## Version

1.0.0

## Decision

Maintain current non-standard Poseidon hash parameters with a documented migration plan to Filecoin/Neptune reference constants, requiring a hard fork to implement.

## Context

The Omnia protocol uses Poseidon as the SNARK-friendly hash function for Merkle path verification and state transition constraints in the ZK rollup circuit. The current implementation in `zk/src/poseidon.rs` uses:

- MDS matrix: Cauchy construction (not Filecoin/Neptune reference)
- Round constants: BLAKE3-derived (not Grain LFSR as in the reference specification)
- Parameters: R_F=8, R_P=57, alpha=5, t=3

The Filecoin and Neptune projects have established reference Poseidon parameters that are widely audited and interoperable. Using non-standard parameters means:
- Omnia proofs are not compatible with other Poseidon-based systems
- The security of the non-standard parameters has not been independently audited
- Any cross-chain verification or proof aggregation with other protocols would be impossible

## Alternatives Considered

### Immediate Migration
Immediately switch to Filecoin/Neptune reference parameters. This would:
- Break all existing proofs (every proof generated under current params becomes invalid)
- Require all circuits to be re-generated with new parameters
- Require all operators to update simultaneously
- Be equivalent to a hard fork

### Dual-Hash Transition Period
Run both hash functions in parallel for a transition period:
- Old proofs use current parameters
- New proofs use reference parameters
- Validators accept both during transition
- After transition, only reference parameters accepted
This is the safest but most complex approach.

### Keep Current Parameters (Chosen)
Document the deviation and plan migration for a future hard fork:
- Current parameters are mathematically sound (Cauchy MDS, proper round count)
- Migration is deferred to avoid breaking existing deployments
- ADR documents the risk and migration path

## Consequences

### Positive
- No immediate disruption to existing proofs and deployments
- Current parameters are mathematically valid (just non-standard)
- Migration can be planned and coordinated with operators

### Negative
- Existing proofs are not interoperable with Filecoin/Neptune
- No independent security audit of the Cauchy MDS matrix construction
- BLAKE3-derived round constants may not have the same security margin as Grain LFSR-derived constants
- A future hard fork will be required to migrate, which is disruptive

### Trade-offs
- Chose deployment stability over spec compliance
- The deviation is documented and tracked for future resolution
- Phase 3 should prioritize Poseidon parameter migration with a dual-hash transition

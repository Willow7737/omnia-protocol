# ADR-014: Poseidon Parameter Migration Strategy

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-08-11

## Status

Accepted — Updated in Phase 5 (dual-hash foundation added)

## Date

2025-05-18 (original), 2026-05-19 (Phase 5 update)

## Version

2.0.0

## Decision

Implement a dual-hash transition from custom BLAKE3-derived Poseidon parameters to Filecoin/Neptune reference parameters, with both versions available during the migration period.

## Context

The Omnia protocol uses Poseidon as the SNARK-friendly hash function for Merkle path verification and state transition constraints in the ZK rollup circuit. The current implementation in `omnia-adapters/src/poseidon.rs` uses:

- MDS matrix: Cauchy construction (not Filecoin/Neptune reference)
- Round constants: BLAKE3-derived (not Grain LFSR as in the reference specification)
- Parameters: R_F=8, R_P=57, alpha=5, t=3

Phase 5 adds the `PoseidonVersion` enum with `Custom` (current, default) and `Reference` (target) options, enabling the dual-hash transition.

## Migration Timeline

### Phase A: Both Versions Available, Custom is Default (Phase 5 — Current)

- `PoseidonVersion::Custom` is the default for all hash operations
- `PoseidonVersion::Reference` available via `poseidon_hash_with_version(_, Reference)`
- Reference parameters are placeholder (zero-filled) until populated from Filecoin/Neptune
- All existing proofs use Custom parameters and continue to work

### Phase B: Both Versions Available, Reference is Default for New Proofs (Phase 6 — Testnet)

- Reference constants populated from Filecoin/Neptune repository
- New proofs default to `PoseidonVersion::Reference`
- Custom parameters still accepted for verification of existing proofs
- ZK circuit updated to support both versions
- Trusted setup keys regenerated for Reference parameters

### Phase C: Custom Deprecated, Only Reference Accepted (Phase 7 — Mainnet)

- Only `PoseidonVersion::Reference` accepted for new proofs
- Custom parameters kept for historical verification only
- All existing Custom proofs must be regenerated with Reference parameters
- Migration tooling provided for batch proof regeneration

## Alternatives Considered

### Immediate Migration

Breaks all existing proofs immediately. Rejected due to deployment disruption.

### Dual-Hash Transition (Chosen)

Safest approach: both versions coexist during transition. Allows gradual migration with no network downtime.

### Keep Current Parameters (Previous Decision)

Retained as Phase A of the dual-hash transition. Now has a concrete migration timeline.

## Consequences

### Positive

- Concrete migration plan with defined phases
- No immediate disruption to existing proofs
- Reference parameters will provide interoperability with Filecoin/Neptune
- Phase 5 infrastructure (`PoseidonVersion`, `poseidon_hash_with_version`) ready for Phase B

### Negative

- Three-phase migration requires coordination across multiple releases
- Trusted setup keys must be regenerated for Reference parameters
- All existing ZK proofs must eventually be regenerated
- Dual-hash period increases code complexity

### Trade-offs

- Migration complexity traded for long-term interoperability and auditability
- Phased approach reduces risk compared to immediate migration
- Phase 5 lays the foundation; Phases 6-7 complete the transition

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

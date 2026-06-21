# ADR-009: Poseidon Hash Parameter Generation Justification

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-05-20

## Status

Accepted

## Context

The Poseidon hash implementation in `zk/src/poseidon.rs` uses Cauchy MDS matrix generation with BLAKE3 round constants instead of the Grain LFSR method specified in the original Poseidon paper. This deviation creates an interoperability risk and could affect formal security guarantees.

## Decision

We justify this deviation on the following grounds:

1. **Algebraic Equivalence**: The Cauchy method for MDS matrix generation produces matrices with the same algebraic properties (maximum distance separable) as those produced by the Grain LFSR method. Both methods ensure the MDS property, which is the security-relevant property.

2. **Cryptographic Strength of BLAKE3**: BLAKE3 is a cryptographically secure hash function. Using BLAKE3 for round constant generation provides the same randomness guarantees as Grain LFSR - both produce uniformly distributed constants with no exploitable structure.

3. **No Known Attacks**: There are no known attacks that exploit the specific parameter generation method in Poseidon. All known attacks target the round function structure, not the parameter generation.

4. **Test Validation**: The generated parameters pass the same algebraic tests (MDS property, round constant uniformity) that Grain LFSR-generated parameters pass.

## Consequences

- **Positive**: Simpler implementation, no need for a Grain LFSR implementation
- **Negative**: Incompatible with other Poseidon implementations that strictly follow the paper
- **Mitigation**: Full documentation of the deviation allows other implementations to replicate our parameters if interoperability is needed

## References

- Grassi, L., Khovratovich, D., Rechberger, C., Roy, A., & Schofnegger, M. (2021). Poseidon: A New Hash Function for Zero-Knowledge Proof Systems. USENIX Security.
- Cauchy MDS matrix generation: https://github.com/arkworks-rs/algebra

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

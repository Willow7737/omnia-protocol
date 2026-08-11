# Audit Findings Template

> 🎯 Audience: Security Researchers
> 🔗 Context: Part of the audit documentation section
> 📅 Last Updated: 2026-08-11

**Version**: 1.0
**Date**: 2026-05-19

## Instructions

Use this template for reporting all audit findings. Each finding should be a separate entry with all fields completed. Assign severity using the criteria defined in `AUDIT_PACKAGE.md`.

---

## Finding [F-XXX]: [Title]

### Severity

- [ ] Critical
- [ ] High
- [ ] Medium
- [ ] Low
- [ ] Informational

### Status

- [ ] Open
- [ ] Acknowledged
- [ ] Fix in Progress
- [ ] Fixed
- [ ] Won't Fix (with rationale)

### Component

- [ ] `substrate/src/consensus.rs` — Consensus engine
- [ ] `substrate/src/vrf.rs` — VRF leader selection
- [ ] `substrate/src/slashing.rs` — Slashing engine
- [ ] `substrate/src/causal_graph.rs` — Causal graph
- [ ] `substrate/src/event.rs` — Event processing
- [ ] `substrate/src/gossip.rs` — Gossip protocol
- [ ] `substrate/src/network.rs` — P2P networking
- [ ] `substrate/src/keystore.rs` — Key storage
- [ ] `zk/src/circuit.rs` — ZK rollup circuit
- [ ] `zk/src/poseidon.rs` — Poseidon hash
- [ ] `zk/src/prover.rs` — Proof generation
- [ ] `zk/src/setup/` — Trusted setup
- [ ] `binding/src/quantum_commit.rs` — Quantum commitments
- [ ] `binding/src/key_rotation.rs` — PQC key rotation
- [ ] `node/src/api/` — REST API
- [ ] Other: **\*\***\_\_\_**\*\***

### Description

[Provide a clear, detailed description of the finding. Include what the vulnerability is, where it exists, and why it matters.]

### Impact

[Describe the potential impact if this vulnerability is exploited. Be specific about what an attacker could achieve.]

### Reproduction Steps

1. [Step 1]
2. [Step 2]
3. [Step 3]

### Proof of Concept

```rust
// Include proof-of-concept code here if applicable
```

### Recommended Fix

[Describe how to fix the vulnerability. Be specific about which file(s) and function(s) need to change and what the correct behavior should be.]

### References

- [Link to relevant ADR, threat model, or external reference]

---

## Findings Summary Table

| ID    | Title | Severity | Component | Status |
| ----- | ----- | -------- | --------- | ------ |
| F-001 |       |          |           |        |
| F-002 |       |          |           |        |
| F-003 |       |          |           |        |

---

## Severity Definitions

| Severity          | Criteria                                                                                                                      |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| **Critical**      | Direct fund loss, consensus break, key theft, ZK proof forgery exploitable on mainnet. Immediate action required.             |
| **High**          | State corruption, signature bypass, slashing circumvention, significant DoS vector. Fix before mainnet.                       |
| **Medium**        | Information leak, authentication bypass on non-critical endpoints, degraded performance. Fix before or shortly after mainnet. |
| **Low**           | Minor bugs, code quality issues, documentation errors with security implications. Fix in next release.                        |
| **Informational** | Best practice suggestions, style issues, non-security observations. Consider for future improvements.                         |

---

🔙 **Back**: [Audit](./) | 🔄 **Related**: [Attack Surface](./ATTACK_SURFACE.md)
🚀 **Next**: [Self Assessment](./SELF_ASSESSMENT.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

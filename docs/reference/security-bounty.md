# Security Bounty Program
> 🎯 Audience: All
> 🔗 Context: Vulnerability disclosure and bounty reward structure
> 📅 Last Updated: 2026-05-20

**Version**: 1.0
**Effective Date**: 2026-05-21
**Status**: Active

## Overview

The Omnia Protocol bug bounty program rewards security researchers who discover and responsibly disclose vulnerabilities in the Omnia Protocol codebase. This program is essential for ensuring the security of a protocol that handles real economic value through its consensus mechanism, ZK proof system, and quantum-resistant cryptographic commitments.

## Scope

### In Scope

The following components are within the scope of the bug bounty program:

- **All Rust code in the `omnia-protocol` repository**, including:
  - `substrate/` — Causal graph consensus, vector clocks, CRDTs, gossip protocol, slashing
  - `omnia-adapters/` — Groth16 ZK circuits, Poseidon hash, proof generation and verification, trusted setup
  - `binding/` — Quantum commitments, PQC key rotation, RF fingerprinting, provenance chains
  - `shards/` — Domain shard state machines and operation handlers
  - `economics/` — UBC token logic, decay model, governance
  - `node/` — REST API, authentication, HTTP server
  - `chaos-tests/` — Load testing and chaos engineering framework
- **Solidity contracts** in `omnia-adapters/contracts/ethereum/`
- **Cryptographic implementations** specifically:
  - VRF (Ed25519-based and ECVRF constructions)
  - Poseidon hash function (custom and reference parameters)
  - Groth16 proof generation and verification
  - CRYSTALS-Dilithium signature operations
  - ML-KEM (Kyber) key encapsulation
  - BLS12-381 signature aggregation
- **Consensus mechanism**: BFT finality, leader selection, slashing, equivocation detection
- **Network protocol**: libp2p gossip, Kademlia DHT, message compression

### Out of Scope

The following are explicitly out of scope:

- **Third-party dependencies** — Report vulnerabilities to the upstream maintainer
- **Social engineering attacks** — Phishing, impersonation, etc.
- **Denial of service** — Already mitigated by rate limiting; DoS findings are low priority
- **Issues in test-only code** — Test helpers, mock implementations, documentation
- **Issues requiring physical access** — Hardware attacks, side-channel via physical proximity
- **Spam or information disclosure without security impact** — Verbose error messages without data leak

## Reward Tiers

| Severity | Description | Reward Range |
|----------|-------------|--------------|
| **Critical** | Consensus break, key theft, fund loss, ZK proof forgery, state corruption exploitable on mainnet | $10,000 – $50,000 |
| **High** | State corruption (non-exploitable on mainnet), signature bypass, DoS bypass, slashing circumvention | $5,000 – $10,000 |
| **Medium** | Information leak, degraded performance, nonce reuse, authentication bypass on non-critical endpoints | $1,000 – $5,000 |
| **Low** | Minor bugs, UX issues, documentation errors with security implications | $100 – $1,000 |

### Severity Assessment

Severity is assessed based on the following factors:

1. **Impact**: What can an attacker achieve? (e.g., steal funds, forge proofs, disrupt consensus)
2. **Likelihood**: How feasible is the attack? (e.g., requires local access vs. remote, requires specific conditions)
3. **Scope**: How many users or components are affected?
4. **Exploitability**: How complex is the exploit? (e.g., trivial vs. requires deep cryptographic knowledge)

The Omnia security team reserves the right to adjust severity classifications based on these factors.

## Reporting

### How to Report

1. **Email**: Send reports to **security@omnia-protocol.org**
2. **Encryption**: Use the PGP key published in `SECURITY.md` for encrypted reports
3. **Format**: Include the following in your report:
   - Affected component(s) and file(s)
   - Steps to reproduce
   - Potential impact and severity assessment
   - Proof-of-concept code (if applicable)
   - Your preferred contact method for follow-up

### Response Timeline

| Milestone | Target |
|-----------|--------|
| Acknowledgment of report | Within 24 hours |
| Initial assessment and severity classification | Within 72 hours |
| Fix development begins | Within 5 business days (critical/high) |
| Patch release | Critical: 7 days, High: 14 days, Medium: 30 days, Low: next release |
| Bounty payment | Within 30 days of patch release |

### Responsible Disclosure

We follow a **90-day embargo** policy:

1. Reports are kept confidential until a patch is available
2. After 90 days from the initial report, the researcher may disclose publicly if no patch has been released
3. We will make every effort to ship fixes well before this deadline
4. Coordinated disclosure is preferred — we work with the researcher on the disclosure timeline

## Eligibility

### Requirements

To be eligible for a bounty reward, you must:

1. Be the **first reporter** of a previously unknown vulnerability
2. **Not exploit** the vulnerability beyond what is necessary for a proof-of-concept
3. Follow the **responsible disclosure timeline** (90-day embargo)
4. **Not access or modify** other users' data
5. **Not use automated scanners** that generate excessive traffic or cause service disruption
6. Comply with all applicable laws

### Exclusions

The following are not eligible for rewards:

- Vulnerabilities already known to the Omnia security team
- Issues reported in components explicitly out of scope
- Vulnerabilities discovered through violations of law
- Issues found in third-party dependencies (report to upstream)
- Theoretical vulnerabilities without practical demonstration

## Payment

Bounty rewards are paid in USDC or USDT on Ethereum mainnet. Alternative payment methods may be arranged by mutual agreement. Payment is processed within 30 days of the patch release.

## Program Updates

This bug bounty program may be updated periodically. Changes to scope, reward tiers, or rules will be announced via the Omnia Protocol GitHub repository and official communication channels.

---

*This bug bounty program is administered by the Omnia Protocol security team. For questions, contact security@omnia-protocol.org.*

---
🔙 **Back**: [Reference Index](../) | 🔄 **Related**: [Roadmap](./roadmap.md)
🚀 **Next**: [Blueprint Reference](./blueprint-reference.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

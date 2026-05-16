# Omnia Protocol — Cryptographic Migration Playbook

**Phase D2 Deliverable**  
**Sprint 6 — Security Hardening**

---

## 1. Overview

This document defines the procedures for migrating cryptographic primitives in the Omnia Protocol when a vulnerability, compromise, or obsolescence is discovered. Cryptographic agility is essential for a long-lived protocol — the primitives we trust today (Ed25519, BLAKE3, BN254) may be broken tomorrow by advances in mathematics or quantum computing.

### 1.1 Four-Phase Migration Process

Every cryptographic migration follows a strict four-phase process:

| Phase | Name | Duration | Description |
|-------|------|----------|-------------|
| 1 | **Disclosure** | 0–72 hours | Vulnerability is disclosed privately to the security team. Impact assessment begins. Affected components are identified. |
| 2 | **Deprecation** | 1–4 weeks | The vulnerable primitive is marked as deprecated. New code must not use it. A migration flag is added to the protocol configuration. |
| 3 | **Migration** | 4–12 weeks | The new primitive is implemented alongside the old one. A dual-signature/dual-hash period ensures backward compatibility. Events may carry both old and new signatures. |
| 4 | **Sunset** | 2–4 weeks | The old primitive is removed from the codebase. Events with only old signatures are rejected. The migration is complete. |

### 1.2 General Migration Principles

1. **No hard forks** — Migrations must be backward-compatible during the deprecation and migration phases.
2. **Dual-signature period** — During migration, events should carry both old and new signatures to ensure nodes at different migration stages can verify them.
3. **Configuration-driven** — The active primitive set is controlled by protocol configuration, not hardcoded.
4. **Testnet first** — All migrations are deployed to testnet for a minimum of 2 weeks before mainnet.
5. **Communication** — All migrations are announced via governance proposals with a minimum 4-week notice period.

---

## 2. Migration Paths

### 2.1 Ed25519 Compromise

**Scenario**: A practical attack on Ed25519 is discovered (e.g., discrete logarithm on Curve25519 becomes feasible, or a critical implementation vulnerability in ed25519-dalek).

| Step | Action | Timeline | Responsible |
|------|--------|----------|-------------|
| 1 | Security team acknowledges disclosure | T+0h | Security Lead |
| 2 | Assess impact — identify all Ed25519 usage (event signing, DID authentication, gossip) | T+24h | Cipher (Agent 02) |
| 3 | Select replacement primitive (e.g., Ed448, CRYSTALS-Dilithium, or SPHINCS+) | T+72h | Crypto Team |
| 4 | Publish migration proposal via governance | T+1w | Core Team |
| 5 | Implement new signature crate with trait-based abstraction (`SignatureScheme` trait) | T+2w | Cipher (Agent 02) |
| 6 | Add dual-signature support to Event struct — events carry both Ed25519 and new signature | T+4w | Cipher (Agent 02) |
| 7 | Deploy to testnet with dual-signature verification | T+5w | DevOps |
| 8 | Update `Event::verify_signature()` to accept either signature | T+5w | Cipher (Agent 02) |
| 9 | Node operators generate new keypairs and register public keys on-chain | T+6w | Node Operators |
| 10 | Mainnet deployment with dual-signature period (both signatures accepted) | T+8w | DevOps |
| 11 | Governance vote to begin sunset phase (reject Ed25519-only events) | T+10w | Governance |
| 12 | Sunset — Ed25519 signatures no longer accepted | T+12w | Cipher (Agent 02) |
| 13 | Remove ed25519-dalek dependency from Cargo.toml | T+14w | Cipher (Agent 02) |

**Key Considerations:**

- The `Event` struct must be extended with an optional `alt_signature` field and `alt_pubkey` field.
- The `EventProcessor` trait must remain unchanged — signature verification happens before event processing.
- Existing events with only Ed25519 signatures must remain verifiable for historical queries (archive nodes).
- Key rotation must be atomic — a node must not accept events signed with the old key after the rotation epoch.

### 2.2 BLAKE3 Collision

**Scenario**: A collision attack on BLAKE3 is discovered, making it possible to create two different data sets with the same hash.

| Step | Action | Timeline | Responsible |
|------|--------|----------|-------------|
| 1 | Security team acknowledges disclosure | T+0h | Security Lead |
| 2 | Assess impact — BLAKE3 is used for state root, Merkle proofs, batch commitments | T+24h | Cipher (Agent 02) |
| 3 | Select replacement hash (e.g., SHA3-256, KangarooTwelve, or BLAKE3b) | T+48h | Crypto Team |
| 4 | Implement `HashFunction` trait in substrate/src/crypto.rs | T+1w | Cipher (Agent 02) |
| 5 | Add dual-hash computation to CausalGraph — compute both BLAKE3 and new hash | T+2w | Cipher (Agent 02) |
| 6 | Update `CausalGraph::state_root()` to return new hash; BLAKE3 root available as `legacy_root()` | T+3w | Cipher (Agent 02) |
| 7 | Deploy to testnet | T+4w | DevOps |
| 8 | Update ZK proof circuit to use new hash for state commitments | T+5w | Cipher (Agent 02) |
| 9 | Mainnet deployment with dual-hash period | T+8w | DevOps |
| 10 | Governance vote to begin sunset phase | T+10w | Governance |
| 11 | Sunset — BLAKE3 no longer used for new state roots | T+12w | Cipher (Agent 02) |
| 12 | Remove BLAKE3 from active hash computation (keep for legacy verification) | T+14w | Cipher (Agent 02) |

**Key Considerations:**

- State root migration is particularly sensitive — the L1 settlement contract must be updated to accept the new state root format.
- Merkle proofs must be regenerated for all active state using the new hash function.
- Historical events can retain their BLAKE3-based hashes for verification; only new events use the new hash.
- The ZK circuit must be recompiled with the new hash function — this requires a new trusted setup if using Groth16.

### 2.3 BN254 Curve Compromise

**Scenario**: The BN254 pairing-friendly curve used for ZK proofs is compromised (e.g., discrete logarithm on the curve becomes feasible due to improved Number Field Sieve).

| Step | Action | Timeline | Responsible |
|------|--------|----------|-------------|
| 1 | Security team acknowledges disclosure | T+0h | Security Lead |
| 2 | Assess impact — BN254 is used in ZK proof generation and verification (zk/ module) | T+24h | Cipher (Agent 02) |
| 3 | Select replacement curve (e.g., BLS12-381, BN254 with scalar field extension) | T+1w | Crypto Team |
| 4 | Implement new curve backend in zk/ crate using arkworks | T+3w | Cipher (Agent 02) |
| 5 | Re-implement ZK circuits for the new curve | T+5w | Cipher (Agent 02) |
| 6 | Run new trusted setup ceremony (Powers of Tau) for the new curve | T+7w | Crypto Team + Community |
| 7 | Deploy new verifier contract to L1 | T+8w | DevOps |
| 8 | Testnet deployment with new proof system | T+9w | DevOps |
| 9 | Mainnet deployment — both old and new proofs accepted during transition | T+12w | DevOps |
| 10 | Governance vote to sunset old proof system | T+14w | Governance |
| 11 | Sunset — BN254 proofs no longer accepted | T+16w | Cipher (Agent 02) |

**Key Considerations:**

- This is the most complex migration — it requires a new trusted setup, new verifier contract, and new proof generation.
- BLS12-381 is the most likely replacement — it provides ~128-bit security (vs. BN254's degraded ~100-bit security).
- The `ProofBundle` struct must support both curve types during migration.
- L1 settlement contracts need upgrade paths (proxy patterns or governance-controlled contract migration).
- This migration may take 4–6 months due to the complexity of the trusted setup ceremony.

### 2.4 Q-Day Scenario (Post-Quantum Cryptography)

**Scenario**: A large-scale quantum computer capable of running Shor's algorithm becomes operational, breaking all elliptic curve cryptography (Ed25519, BN254).

**Severity**: Existential — all current cryptographic guarantees are void.

| Step | Action | Timeline | Responsible |
|------|--------|----------|-------------|
| 1 | Emergency security council convenes | T+0h | Security Council |
| 2 | Announce emergency protocol halt (governance emergency vote) | T+6h | Governance |
| 3 | Suspend event processing and ZK proof submission | T+12h | DevOps |
| 4 | Deploy hybrid classical/PQC signature verification (Ed25519 + CRYSTALS-Dilithium) | T+2w | Cipher (Agent 02) |
| 5 | Deploy PQC hash-based state commitments (SHA3-256 or SHAKE256) | T+2w | Cipher (Agent 02) |
| 6 | Replace BN254 ZK proofs with lattice-based proof system (e.g., Aurora, Ligero++) | T+8w | Cipher (Agent 02) |
| 7 | Run PQC trusted setup (if required) | T+10w | Crypto Team |
| 8 | Deploy PQC verifier contracts to all L1 settlement layers | T+12w | DevOps |
| 9 | Testnet deployment with full PQC stack | T+14w | DevOps |
| 10 | Mainnet deployment with emergency governance vote | T+16w | Governance |
| 11 | Resume normal operations with PQC primitives | T+18w | All Teams |

**Key Considerations:**

- Q-Day is a catastrophic scenario requiring emergency response. The protocol should NOT wait for Q-Day to prepare.
- Proactive mitigation: implement hybrid signatures (Ed25519 + Dilithium) in Phase 2, well before quantum computers are practical.
- The binding module already has a CRYSTALS-Dilithium stub (`binding/src/quantum_commit.rs`) — this must be upgraded to a production implementation.
- PQC signatures are significantly larger than classical signatures (Dilithium3: ~2.4 KB vs. Ed25519: 64 bytes). This impacts event size, gossip bandwidth, and storage.
- Lattice-based ZK proofs are less mature than pairing-based proofs — performance and proof size will be worse.
- The economic model must account for increased transaction sizes (higher bandwidth costs, larger L1 calldata).

---

## 3. Migration Infrastructure Requirements

### 3.1 Trait-Based Crypto Abstraction

To enable smooth migrations, all cryptographic operations must be abstracted behind traits:

```rust
/// Abstract signature scheme — enables migration from Ed25519 to Dilithium.
trait SignatureScheme {
    type PublicKey;
    type PrivateKey;
    type Signature;
    fn sign(key: &Self::PrivateKey, message: &[u8]) -> Self::Signature;
    fn verify(pubkey: &Self::PublicKey, message: &[u8], sig: &Self::Signature) -> bool;
}

/// Abstract hash function — enables migration from BLAKE3 to SHA3-256.
trait HashFunction {
    fn hash(data: &[u8]) -> [u8; 32];
    fn hash_multi(chunks: &[&[u8]]) -> [u8; 32];
}
```

### 3.2 Configuration-Driven Primitive Selection

```toml
# omnia-node.toml
[crypto]
signature_scheme = "ed25519"  # "ed25519" | "dilithium3" | "hybrid-ed25519-dilithium"
hash_function = "blake3"      # "blake3" | "sha3-256" | "kangarootwelve"
zk_curve = "bn254"            # "bn254" | "bls12-381"
```

### 3.3 Migration Testing Checklist

- [ ] Dual-signature verification works correctly
- [ ] Events with old-only signatures are accepted during migration phase
- [ ] Events with new-only signatures are accepted during migration phase
- [ ] State root computation produces identical results with new hash
- [ ] ZK proofs verify on L1 with new curve
- [ ] Gossip protocol handles larger event sizes (PQC signatures)
- [ ] Performance benchmarks: new vs. old primitive
- [ ] Chaos tests pass with mixed primitive nodes
- [ ] Upgrade path from old to new configuration is documented
- [ ] Rollback plan exists if migration encounters issues

---

## 4. Pre-Migration Checklist

Before initiating any cryptographic migration, the following must be completed:

1. **Impact Assessment**: Identify all code paths that use the affected primitive.
2. **Replacement Selection**: Evaluate candidates based on security, performance, and ecosystem maturity.
3. **Backward Compatibility Plan**: Define how the protocol handles events created with the old primitive.
4. **Governance Proposal**: Submit a formal proposal with timeline, justification, and risk assessment.
5. **Testnet Deployment**: Deploy to testnet for a minimum of 2 weeks with monitoring.
6. **Node Operator Communication**: Publish migration guide with step-by-step instructions.
7. **Emergency Rollback Plan**: Define criteria for aborting the migration and reverting to the old primitive.
8. **Audit**: Commission external audit of the new primitive integration before mainnet deployment.

---

## 5. Emergency Procedures

If a zero-day vulnerability is discovered in a cryptographic primitive:

1. **T+0h**: Security team is notified via security@omnia-protocol.org
2. **T+1h**: Security council convenes (encrypted channel)
3. **T+4h**: Initial impact assessment complete
4. **T+12h**: If Critical (active exploitation likely): emergency governance vote to enter Deprecation phase immediately
5. **T+24h**: Patch released to testnet with temporary mitigation
6. **T+48h**: If patch is stable: emergency mainnet deployment
7. **T+72h**: Post-incident review and long-term migration plan published

**Emergency Contacts:**

| Role | Contact |
|------|---------|
| Security Lead | security@omnia-protocol.org |
| Cipher (Agent 02) | cipher@omnia-protocol.org |
| DevOps Lead | devops@omnia-protocol.org |

---

## Appendix: Primitive Risk Assessment

| Primitive | Risk Level | Reason | Monitoring |
|-----------|-----------|--------|-----------|
| Ed25519 | Medium | Classical — vulnerable to quantum computing | IACR ePrint, NIST PQC updates |
| BLAKE3 | Low | No known attacks; relatively new (less analysis than SHA-2) | BLAKE3 GitHub security advisories |
| BN254 | Medium | Security level degraded to ~100 bits; new attacks on pairing-friendly curves | IACR ePrint, ECC updates |
| SHA-256 | Low | Extremely well-analyzed; no practical attacks | NIST, IACR |
| libp2p Noise | Low | Well-audiated protocol; XX handshake is standard | libp2p security advisories |

---

*This document should be reviewed quarterly and updated when new cryptographic research is published.*

# Architecture Decision Record Index
> 🎯 Audience: Developers
> 🔗 Context: Index and summaries of all Architecture Decision Records (ADRs)
> 📅 Last Updated: 2026-05-20

## ADR Summary

| ADR | Title | Status | Key Decision |
|-----|-------|--------|-------------|
| ADR-001 | Event Processor Trait | ✅ Adopted | `validate(&self)`, `process_event(&mut self)`, `state_snapshot(&self)` — no interior mutability |
| ADR-002 | Settlement Layer Trait | ✅ Adopted | Settlement-agnostic interface for L1 adapters |
| ADR-003 | Gossip Substrate Interface | ✅ Adopted | Boundary between gossip propagation and substrate ordering |
| ADR-005 | Proof Bundle Format | ✅ Adopted | Serialized format for ZK proof bundles |
| ADR-006 | Shard Trait Contract | ✅ Adopted | `Shard` trait with fee enforcement and nonce tracking |
| ADR-007 | Binding Shard Interface | ✅ Adopted | Provenance and quantum commitment integration |
| ADR-008 | Crypto Dependency Audit | ✅ Adopted | Audit of cryptographic dependency versions |
| ADR-009 | Poseidon Parameter Justification | ✅ Adopted | Cauchy MDS + BLAKE3-derived round constants |
| ADR-010 | Encrypted Keystore Design | ✅ Adopted | AES-256-GCM + HKDF-SHA256 for key storage |
| ADR-011 | Gradual Slashing Model | ✅ Adopted | 3-tier: Warning → Jail → Ejection |
| ADR-012 | VRF Construction Choice | ✅ Adopted | V1 (legacy Ed25519+BLAKE3) + V2 (ECVRF per RFC 9381) |
| ADR-013 | DKG Protocol Selection | ✅ Adopted | Feldman VSS-based DKG |
| ADR-014 | Poseidon Parameter Migration | ✅ Adopted | Dual-hash transition: Custom → Reference (Filecoin/Neptune) |
| ADR-015 | Leader Selection in Consensus Loop | ✅ Adopted | VRF-based, stake-weighted leader selection |
| ADR-016 | Kademlia DHT Configuration | ✅ Adopted | `/omnia/kad/1.0.0`, AutoNAT/Relay/DCutr |
| ADR-017 | GossipSub Peer Scoring Thresholds | ✅ Adopted | Graylist at -100, 1-min decay |
| ADR-018 | Consensus State Persistence | ✅ Adopted | `RedbConsensusStore`, `load_or_new` |
| ADR-019 | Fast-Sync Protocol | ✅ Adopted | BLAKE3 checkpoints, supermajority selection, P2P download |
| ADR-020 | Kyber KEM / ML-KEM Integration | ✅ Adopted | FIPS-203 ML-KEM-768 (replaces pqc_kyber after KyberSlash) |
| ADR-021 | Gossip Message Compression | ✅ Adopted | Snappy compression for messages >256 bytes |

## Source Files

All ADRs are located in `docs/adr/`:

| File | Title |
|------|-------|
| `ADR-001-event-processor-trait.md` | Event Processor Trait |
| `ADR-002-settlement-layer-trait.md` | Settlement Layer Trait |
| `ADR-003-gossip-substrate-interface.md` | Gossip Substrate Interface |
| `ADR-005-proof-bundle-format.md` | Proof Bundle Format |
| `ADR-006-shard-trait-contract.md` | Shard Trait Contract |
| `ADR-007-binding-shard-interface.md` | Binding Shard Interface |
| `ADR-008-crypto-dependency-audit.md` | Crypto Dependency Audit |
| `009-poseidon-parameter-justification.md` | Poseidon Parameter Justification |
| `ADR-010-encrypted-keystore-design.md` | Encrypted Keystore Design |
| `ADR-011-gradual-slashing-model.md` | Gradual Slashing Model |
| `ADR-012-vrf-construction-choice.md` | VRF Construction Choice |
| `ADR-013-dkg-protocol-selection.md` | DKG Protocol Selection |
| `ADR-014-poseidon-parameter-migration.md` | Poseidon Parameter Migration |
| `ADR-015-leader-selection-consensus-loop.md` | Leader Selection in Consensus Loop |
| `ADR-016-kademlia-dht-configuration.md` | Kademlia DHT Configuration |
| `ADR-017-gossipsub-peer-scoring-thresholds.md` | GossipSub Peer Scoring Thresholds |
| `ADR-018-consensus-state-persistence.md` | Consensus State Persistence |
| `ADR-019-fast-sync-protocol.md` | Fast-Sync Protocol |
| `ADR-020-kyber-kem-ml-kem-integration.md` | Kyber KEM / ML-KEM Integration |
| `ADR-021-gossip-message-compression.md` | Gossip Message Compression |

---
🔙 **Back**: [reference/](./) | 🔄 **Related**: [../architecture/trait-boundaries.md](../architecture/trait-boundaries.md)
🚀 **Next**: [security-audit.md](./security-audit.md) | 📜 **Source of Truth**: [Restructuring Blueprint](./blueprint-reference.md)

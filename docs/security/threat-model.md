# STRIDE Threat Model for Omnia Protocol

**Task**: 6.1 — STRIDE threat model
**Date**: 2026-05-14
**Updated**: 2026-05-16
**Version**: 4.0.0

## Overview

This document presents a STRIDE threat model for the Omnia Protocol. STRIDE is a threat classification framework developed by Microsoft that categorizes threats into six types: **S**poofing, **T**ampering, **R**epudiation, **I**nformation Disclosure, **D**enial of Service, and **E**levation of Privilege.

For each threat category, we identify the attack vectors, assess their impact, describe current mitigations, note remaining gaps, and assign a priority level.

---

## 1. Spoofing

Spoofing attacks involve an adversary pretending to be another entity — a node, a validator, or an event creator.

### 1.1 Fake Events (Event Spoofing)

**Attack Vector**: An adversary creates events with a forged `creator` field, claiming they originated from a different node. For example, creating a `FinancialOp::Mint` event that appears to come from the treasury node.

**Impact**: HIGH. If accepted, a spoofed mint event would inflate the money supply. A spoofed transfer event would drain a victim's account.

**Current Mitigation**: Every event carries an Ed25519 signature (`event.signature`) over the event hash (`event.id`), which includes the `creator_pubkey`. The `Event::verify_signature()` method (in `substrate/src/event.rs`) checks that the signature is valid for the claimed public key. The `Event::validate()` method enforces both hash integrity and signature validity before any event enters the causal graph.

**Gap**: The system does not currently verify that the `creator` field matches the `creator_pubkey`. An attacker could set `creator = victim_node_id` but sign with their own keypair. This mismatch would pass `verify_signature()` but break the assumption that `creator` identifies the signer.

**Priority**: HIGH. Fix by adding a check in `Event::validate()` that `creator == hash(creator_pubkey)` or by removing the `creator` field and deriving it from `creator_pubkey`.

### 1.2 Fake Identities (Identity Spoofing)

**Attack Vector**: An adversary creates a DID that impersonates a real-world entity (e.g., `did:omnia:bank-of-america`).

**Impact**: MEDIUM. Could enable social engineering or unauthorized access to identity-bound resources.

**Current Mitigation**: The Identity shard (`shards/src/identity/`) uses Ed25519 keypairs bound to DIDs. A DID document includes the controller's public key, which must sign any updates.

**Gap**: No DID verification registry or certificate authority. Any node can create any DID string. This is a Phase 1+ concern (decentralized identity verification).

**Priority**: MEDIUM. Add DID namespace reservations or a verification registry in Phase 1.

### 1.3 Fake Validators (Validator Impersonation)

**Attack Vector**: An adversary joins the network claiming to be a validator node and participates in consensus, potentially influencing finality decisions.

**Impact**: HIGH. A fake validator could prevent events from reaching the >2/3 supermajority threshold, effectively blocking finality.

**Current Mitigation**: The `ConsensusConfig::total_nodes` parameter defines the validator set size. The `supermajority()` function computes the >2/3 threshold. Unknown nodes are not counted as validators.

**Gap**: No validator authentication or on-chain validator registry. Any node that joins the gossip network can create events and participate in the gossip protocol. True validator authentication requires a staking mechanism (planned for Phase 1).

**Priority**: MEDIUM. The BFT consensus tolerates up to f Byzantine nodes out of 3f+1. With 4 nodes, at most 1 can be Byzantine. Validator authentication is needed when the network grows beyond trusted participants.

### 1.4 RF Fingerprint Spoofing (Binding Layer)

**Attack Vector**: An adversary clones or forges an RF fingerprint to impersonate a physical device in the binding layer.

**Impact**: HIGH. A spoofed RF fingerprint could allow an adversary to create fraudulent provenance events, claiming physical custody of an item they do not possess.

**Current Mitigation**: The `RfFingerprint::verify()` method (`binding/src/rf_fingerprint.rs`) uses Hamming distance comparison against a stored `spectral_hash` with a confidence threshold. Two measurements from the same device should have small Hamming distance; different devices should have large distance.

**Gap**: The RF fingerprinting implementation is a **stub** — it uses raw byte arrays instead of real RF spectral features captured by SDR hardware. The `RfFingerprint::stub()` constructor simply uses provided bytes directly. Real RF-DNA fingerprinting requires hardware access and feature extraction algorithms. An attacker can trivially forge a stub fingerprint by providing the exact same 32 bytes.

**Priority**: HIGH. Real RF fingerprint capture and verification require hardware integration. The stub is suitable for testing but provides no physical security guarantee.

---

## 2. Tampering

Tampering attacks involve unauthorized modification of data — event payloads, state roots, or shard state.

### 2.1 Event Payload Modification

**Attack Vector**: An adversary intercepts a gossip message, modifies the event payload (e.g., changing a transfer amount), and re-broadcasts it.

**Impact**: HIGH. A modified transfer amount could redirect funds or create tokens.

**Current Mitigation**: The `Event::id` field is a SHA-256 hash of all event fields (including payload and `creator_pubkey`). The `Event::compute_hash()` method (in `substrate/src/event.rs`) computes this hash, and `Event::verify_hash()` checks that it matches. The `CausalGraph::insert()` method also verifies the hash before insertion. Any modification to the payload changes the hash, which invalidates the signature and the hash check.

**Gap**: None significant. The hash-then-sign pattern provides strong tamper protection. However, the hash computation includes `creator_pubkey` but not the signature itself, which is correct (the signature covers the hash, not the other way around).

**Priority**: LOW. Current mitigation is strong.

### 2.2 State Root Manipulation

**Attack Vector**: An adversary (who controls the ZK operator) posts a fraudulent state root to L1 that does not correspond to the actual L2 state.

**Impact**: CRITICAL. If accepted by L1, a fraudulent state root would allow the operator to steal all bridged assets.

**Current Mitigation**: The protocol now uses real Groth16 proofs (not just hash-chain stubs). The `RollupOperator` (`zk/src/operator.rs`) generates Groth16 proofs using `prover::create_proof()` with the `RollupCircuit`, and self-verifies before posting. The `ExpandedRollupCircuit` (`zk/src/circuit.rs`) adds Merkle path verification and Poseidon-based state transition constraints, providing stronger guarantees. The `ProofBundle` (`zk/src/proof_bundle.rs`) carries the transition proof along with state roots and batch Merkle root, and `verify_integrity()` rejects bundles with missing or malformed data.

The `SettlementLayer::submit_batch()` method submits the complete `ProofBundle` for L1 verification. The Ethereum adapter (`zk/src/settlement/ethereum.rs`) references the `OmniaRollup.sol` contract which has a `submitBatch()` function that verifies the proof and updates the committed state root.

**Gap**: The Ethereum adapter's `verify_proof()` implementation is still a stub in Phase 0 — it only checks `!proof.is_empty() && proof.len() >= 32`. The Solidity contract's `verifyProof()` is also a stub that checks `_proof.length > 0`. Production requires implementing a real Groth16 verifier in the Solidity contract using the `ark-groth16` verifying key.

**Priority**: CRITICAL. Implement real Groth16 verification in `OmniaRollup.sol` before mainnet.

### 2.3 Provenance Chain Tampering (Binding Layer)

**Attack Vector**: An adversary attempts to modify or remove events from a provenance chain to hide transfers or insert false ownership claims.

**Impact**: HIGH. Tampered provenance could obscure the chain of custody for physical items.

**Current Mitigation**: The `ProvenanceLog` (`binding/src/provenance.rs`) is append-only — events can only be added via `transfer()`, `verify()`, or `destroy()`. The `verify_chain()` method checks that every consecutive pair of events has a valid `links_to()` relationship via their `QuantumCommitment` data hashes. The `from_bytes()` method validates the version byte, preventing format downgrades. The `ProvenanceTracker` (`binding/src/physical_shard.rs`) prevents operations on destroyed items.

**Gap**: The `links_to()` check only verifies that consecutive commitments have different, non-zero data hashes. It does NOT verify that the previous commitment's hash is embedded in the current commitment's signed data. A sophisticated attacker who controls both commitments could forge a chain that passes `links_to()` without real linkage.

**Priority**: MEDIUM. Strengthen `links_to()` to verify cryptographic embedding of previous commitment hash in the current commitment's signed data.

### 2.4 Shard State Mutation Outside `process_event()`

**Attack Vector**: A bug in a shard implementation allows state mutation through `validate()` or `state_snapshot()`, violating the purity contracts.

**Impact**: MEDIUM. Would break determinism guarantees and cause consensus divergence.

**Current Mitigation**: The Rust type system enforces `&self` on `validate()` and `state_snapshot()`, preventing direct mutation. However, interior mutability (via `Cell`, `RefCell`, `AtomicU64`) could bypass this.

**Gap**: No runtime enforcement of the purity contract. Relies on code review and testing.

**Priority**: MEDIUM. Add property-based tests that verify `validate()` and `state_snapshot()` do not mutate state.

---

## 3. Repudiation

Repudiation attacks involve an entity denying that it performed an action — creating an event, casting a vote, or authorizing a transfer.

### 3.1 Denial of Event Creation

**Attack Vector**: A node creates an event, submits it to the network, and later denies having created it (e.g., to avoid accountability for a fraudulent transfer).

**Impact**: MEDIUM. Could undermine auditability and legal accountability.

**Current Mitigation**: Every event is signed with Ed25519 (`Event::sign_with_keypair()`). The signature is non-repudiable — only the holder of the private key could have produced it. The `Event::verify_signature()` method provides cryptographic proof of authorship. In the binding layer, quantum commitments provide additional non-repudiation via `QuantumCommitment::sign_classical()` and `QuantumCommitment::sign_hybrid()`, which create Ed25519 and/or Dilithium signatures over the committed data.

**Gap**: No mechanism for key revocation or rotation in the substrate. The binding layer has `PqcKeyRotationManager` for PQC key rotation, but there is no integration with substrate event signing.

**Priority**: LOW. Key rotation is a Phase 1+ feature for substrate events. The binding layer already supports PQC key rotation.

### 3.2 Denial of Provenance Event

**Attack Vector**: A participant in a supply chain denies having transferred or verified an item, despite their quantum commitment being in the provenance log.

**Impact**: MEDIUM. Could undermine supply chain accountability.

**Current Mitigation**: Each `ProvenanceEvent` contains a `QuantumCommitment` signed by the participant's key. The commitment's `data_hash` is a BLAKE3 hash of the event data, and the signature (Ed25519 and/or Dilithium) proves the participant authorized the event.

**Gap**: No integration between provenance event signatures and the causal graph. The provenance log operates at the binding layer (Layer 3) and is not directly anchored in the L2 causal graph, though each `ProvenanceLog` has a `latest_anchor: EventId` field.

**Priority**: LOW. Current design provides strong non-repudiation within the binding layer.

---

## 4. Information Disclosure

Information disclosure attacks involve unauthorized access to sensitive data — ZK proofs, shard state, or private keys.

### 4.1 ZK Proof Data Visibility

**Attack Vector**: An adversary observes ZK proof data posted to L1 and extracts information about L2 transactions.

**Impact**: MEDIUM. Could reveal transaction amounts, sender/receiver identities, or shard state.

**Current Mitigation**: The `ExpandedRollupCircuit` (`zk/src/circuit.rs`) uses Groth16 proofs which are zero-knowledge by construction — they reveal nothing about the witness (event data, Merkle paths, intermediate roots) beyond the validity of the public statement (old_root, new_root, event_commitment). The `ProofBundle::transition_proof` field contains only the proof, not the transaction data. However, the `ProofBundle::batch_merkle_root` commits to the batch data for data availability.

**Gap**: The `post_batch()` method in settlement adapters posts the full `batch_data` bytes to L1 for data availability. This means transaction data may be visible on L1 even though the ZK proof is zero-knowledge. This is a design choice for transparency in Phase 0.

**Priority**: MEDIUM. For full privacy, the batch data should be committed only via the Merkle root (not posted in full), with individual event data available off-chain via data availability sampling.

### 4.2 Trusted Setup Secret Leakage

**Attack Vector**: An adversary compromises the secret randomness (`tau`) from the Powers of Tau ceremony, enabling creation of fake proofs.

**Impact**: CRITICAL. A compromised trusted setup allows the attacker to generate valid proofs for arbitrary state transitions, breaking the entire ZK rollup security model.

**Current Mitigation**: The trusted setup ceremony (`zk/src/setup/`) uses a multi-party protocol with Proof of Knowledge (PoK). Each participant contributes randomness, and the ceremony is secure as long as at least one participant is honest and destroys their secret. The `ContributionProof` (`zk/src/setup/contribution.rs`) uses Fiat-Shamir on BN254 G1:

```
1. Commit: R = G1 * r (for random r)
2. Challenge: c = H("OMNIA-POK-V1" || R || old_hash || new_hash)
3. Response: t = r + c * s (mod q)
```

The `SetupCeremony` (`zk/src/setup/mod.rs`) enforces a minimum number of participants before key derivation.

**Gap**: The current ceremony is simulated (deterministic seeds in `run_ceremony()`). A production ceremony requires real participant interaction with secure randomness generation, air-gapped machines, and participant attestation.

**Priority**: HIGH. Implement a production-grade ceremony with real multi-party participation before mainnet.

### 4.3 Shard State Exposure

**Attack Vector**: An adversary queries `state_snapshot()` on a shard and extracts sensitive information (e.g., account balances in the Financial shard).

**Impact**: MEDIUM. Financial privacy is a user expectation; exposure could enable targeted attacks.

**Current Mitigation**: The `Shard::state_snapshot()` method is only available to the local node — it is not exposed via RPC or gossip. The state root (a 32-byte hash) is posted to L1, but the full state is not.

**Gap**: No encryption of state snapshots. If a node is compromised, the attacker has full access to shard state.

**Priority**: MEDIUM. Encrypted state snapshots are a Phase 2+ feature. Node operators should use standard OS-level security (disk encryption, access controls) in the interim.

### 4.4 Private Key Compromise

**Attack Vector**: An adversary extracts the Ed25519 or Dilithium private key from a node's memory or storage.

**Impact**: CRITICAL. A compromised Ed25519 key allows the attacker to sign arbitrary events. A compromised Dilithium key allows forging quantum commitments.

**Current Mitigation**: Keys are generated using `ed25519_dalek::SigningKey::generate(&mut OsRng)`, which uses the operating system's secure random number generator. Dilithium keys are generated via `pqc_dilithium::Keypair::generate()`. Keys are stored in memory only (not persisted to disk by default).

**Gap**: No hardware security module (HSM) integration. No key encryption at rest. No multi-signature support for high-value operations.

**Priority**: HIGH. Add HSM support and key encryption before mainnet.

---

## 5. Denial of Service

Denial of service attacks aim to make the system unavailable or unusable.

### 5.1 Gossip Flooding

**Attack Vector**: An adversary floods the gossip network with events, consuming bandwidth and processing capacity.

**Impact**: HIGH. Could prevent legitimate events from being gossiped in a timely manner, degrading finality latency.

**Current Mitigation**: The `GossipConfig::max_pending` parameter (default: 100,000) limits the pending event queue size. The `GossipConfig::max_events_per_message` parameter (default: 100) limits the number of events per gossip message. The `seen_events` HashSet deduplicates incoming events. The rate limiter (`substrate/src/rate_limiter.rs`) provides token-bucket rate limiting (200 burst/100s).

**Gap**: No peer reputation or blacklisting system. A malicious peer that respects rate limits but sends crafted events could still consume resources.

**Priority**: MEDIUM. Rate limiting is implemented. Add peer reputation scoring for further hardening.

### 5.2 Consensus Stall

**Attack Vector**: A Byzantine validator refuses to create witness events, preventing the network from reaching the >2/3 supermajority threshold.

**Impact**: HIGH. Events would never achieve finality, effectively freezing the protocol.

**Current Mitigation**: The BFT consensus engine tolerates up to f Byzantine nodes out of 3f+1 total. View-change and round timeout mechanisms were implemented in Sprint 6, allowing the network to make progress even when some validators are unresponsive.

**Gap**: If >1/3 of validators are offline or Byzantine, the network cannot make progress. This is a fundamental BFT limitation, not a gap.

**Priority**: LOW. BFT bounds are by design. View-change is implemented.

### 5.3 ZK Proving DoS

**Attack Vector**: An adversary submits many events requiring expensive ZK proof generation, overloading the operator's proving capacity.

**Impact**: MEDIUM. Could delay batch finalization and L1 settlement.

**Current Mitigation**: The `RollupOperator` (`zk/src/operator.rs`) batches events with a configurable `batch_size` limit. Proof generation uses cached trusted setup keys. The `ExpandedRollupCircuit` generates constraints proportional to `num_events * merkle_depth`, which bounds the proving time per batch.

**Gap**: No per-event proving cost limit. No mechanism to reject events that would cause excessively large circuits. No timeout on proof generation.

**Priority**: MEDIUM. Add a maximum circuit size and proving timeout in the operator.

---

## 6. Elevation of Privilege

Elevation of privilege attacks involve an adversary gaining higher privileges than they are authorized for.

### 6.1 Validator Takeover

**Attack Vector**: An adversary gains control of a validator node (e.g., via key compromise or social engineering) and uses it to influence consensus.

**Impact**: CRITICAL. A compromised validator can create arbitrary events, vote against valid events, and influence finality decisions.

**Current Mitigation**: The BFT consensus engine tolerates up to f Byzantine validators out of 3f+1. A single compromised validator cannot independently finalize invalid events (needs >2/3 agreement). Slashing is implemented for equivocation and other misbehavior.

**Gap**: No validator rotation or ejection mechanism. A compromised validator remains in the set indefinitely.

**Priority**: HIGH. Implement validator rotation before mainnet.

### 6.2 Governance Manipulation

**Attack Vector**: An adversary accumulates governance tokens and uses them to pass proposals that benefit themselves (e.g., increasing their validator stake, modifying protocol parameters).

**Impact**: MEDIUM. Could centralize control of the protocol.

**Current Mitigation**: The Economics shard (`economics/src/governance.rs`) implements quadratic voting, which reduces the influence of large token holders. Time-locked voting was added in Sprint 6 to prevent flash loan attacks.

**Gap**: No delegation mechanism. No minimum quorum for proposals.

**Priority**: MEDIUM. Strengthen governance mechanisms in Phase 1.

### 6.3 PQC Key Rotation Downgrade

**Attack Vector**: An adversary attempts to downgrade the commitment phase from Hybrid to ClassicalOnly, weakening the security of quantum commitments.

**Impact**: MEDIUM. Downgrading from Hybrid to ClassicalOnly would make commitments vulnerable to quantum attacks if a quantum computer exists.

**Current Mitigation**: The `PqcKeyRotationManager` (`binding/src/key_rotation.rs`) rejects phase downgrades:

```rust
if (request.new_phase as u8) < (self.current_phase as u8) {
    return Err("Cannot downgrade from ... to ...");
}
```

The `CommitmentPhase` enum ordering (ClassicalOnly=0, Hybrid=1, PostQuantum=2) ensures that phases only advance. Rotation requests also require an `authorization_sig` from the old key.

**Gap**: No cryptographic verification of the `authorization_sig` — only emptiness check. A non-empty but invalid signature would be accepted.

**Priority**: MEDIUM. Add proper signature verification for rotation authorization.

### 6.4 Shard-Level Privilege Escalation

**Attack Vector**: An adversary exploits a bug in a shard implementation to escalate privileges within the shard (e.g., minting tokens without authorization in the Financial shard).

**Impact**: HIGH. Could create unlimited tokens or modify account balances.

**Current Mitigation**: The `FinancialState::apply()` method enforces business rules (e.g., insufficient balance check). The `Shard::validate()` method provides pre-flight validation. The `ShardError::InvalidOperation` and `ShardError::ValidationFailed` variants catch unauthorized operations.

**Gap**: No formal access control list (ACL) for shard operations. Any event can trigger any operation — there is no check that the event creator is authorized to perform the operation (e.g., only the treasury can mint).

**Priority**: HIGH. Add ACL checks in `Shard::process_event()` that verify the event creator is authorized for the operation type.

---

## Threat Summary

| Category | Threat | Impact | Priority | Current Status |
|----------|--------|--------|----------|----------------|
| Spoofing | Fake Events | HIGH | HIGH | Mitigated by Ed25519 signatures; gap in `creator` ↔ `creator_pubkey` binding |
| Spoofing | Fake Identities | MEDIUM | MEDIUM | Mitigated by key-bound DIDs; gap in DID verification |
| Spoofing | Fake Validators | HIGH | MEDIUM | Mitigated by BFT threshold; gap in validator authentication |
| Spoofing | RF Fingerprint Spoofing | HIGH | HIGH | Stub implementation provides no real physical security |
| Tampering | Event Payload Mod. | HIGH | LOW | Strongly mitigated by hash-then-sign |
| Tampering | State Root Manip. | CRITICAL | CRITICAL | Groth16 proofs implemented; Solidity verifier still stub |
| Tampering | Provenance Chain Tampering | HIGH | MEDIUM | Append-only log with links_to(); gap in cryptographic embedding |
| Tampering | Shard State Mutation | MEDIUM | MEDIUM | Mitigated by Rust type system; gap in runtime enforcement |
| Repudiation | Denial of Event Creation | MEDIUM | LOW | Non-repudiable Ed25519 + Dilithium signatures |
| Repudiation | Denial of Provenance Event | MEDIUM | LOW | Quantum commitments provide non-repudiation |
| Info Disclosure | ZK Proof Data Visibility | MEDIUM | MEDIUM | ZK proofs are zero-knowledge; batch data may be posted to L1 |
| Info Disclosure | Trusted Setup Compromise | CRITICAL | HIGH | Multi-party PoK ceremony; production ceremony needed |
| Info Disclosure | Shard State Exposure | MEDIUM | MEDIUM | No external API; gap in encryption at rest |
| Info Disclosure | Private Key Compromise | CRITICAL | HIGH | Keys in memory only; no HSM |
| DoS | Gossip Flooding | HIGH | MEDIUM | Rate limiting implemented |
| DoS | Consensus Stall | HIGH | LOW | View-change implemented |
| DoS | ZK Proving DoS | MEDIUM | MEDIUM | No circuit size limit or proving timeout |
| Elevation | Validator Takeover | CRITICAL | HIGH | BFT + slashing; no validator rotation |
| Elevation | Governance Manip. | MEDIUM | MEDIUM | Quadratic voting + time-locks |
| Elevation | PQC Key Rotation Downgrade | MEDIUM | MEDIUM | Downgrade rejected; authorization sig not verified |
| Elevation | Shard Privilege Esc. | HIGH | HIGH | No ACL for shard operations |

## Priority Action Items

1. **CRITICAL — Implement Solidity Groth16 verifier**: Replace stub `verifyProof()` in `OmniaRollup.sol` with a real Groth16 verifier contract.
2. **HIGH — Fix `creator` ↔ `creator_pubkey` binding**: Add a validation check in `Event::validate()`.
3. **HIGH — Production trusted setup ceremony**: Implement real multi-party ceremony with secure randomness before mainnet.
4. **HIGH — RF fingerprint hardware integration**: Replace stub with real RF-DNA feature extraction.
5. **HIGH — Shard ACL**: Add authorization checks for privileged shard operations (minting, burning).
6. **HIGH — HSM support**: Add hardware security module integration for key protection.
7. **MEDIUM — Strengthen `links_to()` verification**: Verify cryptographic embedding of previous commitment hash.
8. **MEDIUM — Verify rotation authorization signatures**: Add cryptographic verification in `PqcKeyRotationManager`.
9. **MEDIUM — ZK proving DoS protection**: Add circuit size limits and proving timeouts.

# Frequently Asked Questions

> 🎯 Audience: All
> 🔗 Context: Common questions about the Omnia Protocol, its economics, security model, and practical usage
> 📅 Last Updated: 2026-05-20

## General Questions

### Q: What is Omnia?

**A:** Omnia is a universal coordination layer for reality — a decentralized protocol that replaces trust with mathematics. It enables value exchange, identity verification, and physical-digital fusion without intermediaries.

Think of it like the internet. No one owns the internet; it's just a set of rules (TCP/IP) that lets computers talk to each other. Omnia is the same idea, but for **value, identity, and trust**.

### Q: Is Omnia a cryptocurrency?

**A:** No. Omnia is a protocol, not a coin. While Omnia has a token model (UBC — Universal Basic Compute), the protocol itself is much broader. It's a framework for:

- Identity management (DIDs, social recovery, biometric anchors, AI agents)
- Supply chain tracking (append-only provenance logs)
- AI agent governance (5 capability types with bounded permissions)
- Economic coordination (quadratic voting, time-locked voting, useful-work rewards)
- Physical-digital binding (RF fingerprints, quantum commitments)

### Q: Who controls Omnia?

**A:** No one. Omnia is decentralized and governed by the community through transparent, mathematical processes. There is no central authority, company, or government that controls it.

### Q: Is Omnia open source?

**A:** Yes. All code is open source and available on GitHub. The protocol is public domain (CC0), meaning anyone can use, modify, or build on it.

### Q: What is the current state of the project?

**A:** All 5 core layers are implemented and tested (938+ tests passing). The protocol has causal graph consensus with VRF-based leader selection, 6 domain shards (Financial, Computational, Physical, Biological, Identity, Economics), a binding layer with provenance tracking, identity hardening with DIDs and Shamir's Secret Sharing, and an economics layer with UBC and quadratic voting. The ZK-rollup settlement layer uses real arkworks R1CS + Groth16 + Poseidon proofs on BN254 with Merkle path verification. ML-KEM-768 (FIPS-203) post-quantum cryptography and fee enforcement are implemented. Phase 3 added Kademlia DHT peer discovery, GossipSub peer scoring, consensus state persistence, fast-sync protocol, message compression, and gradual slashing. Some features like real RF fingerprinting remain stubs awaiting hardware. Bitcoin/Solana/Celestia settlement adapters are stubs. There is no mobile wallet and no validator network yet.

### Q: How do I run the tests?

**A:** Clone the repository and run:

```bash
git clone https://github.com/Willow7737/omnia-protocol.git
cd omnia-protocol
cargo test --workspace
```

You should see 938+ tests passing.

---

## Technical Questions

### Q: How does causal graph consensus work?

**A:** Instead of organizing events into sequential blocks, Omnia maintains a directed acyclic graph (DAG) where:

- Each event (transaction) is a node
- Edges represent causal relationships (event A must happen before event B)
- Unrelated events can be processed in parallel
- The graph naturally captures causality without artificial ordering

**Example:**

```
If Alice pays Bob, and Carol pays Dave:
- These are independent events
- They can happen simultaneously
- Only when Bob pays Carol do we need to establish order
```

This is much faster than traditional blockchains where every transaction waits in a single queue.

### Q: What are zero-knowledge proofs?

**A:** Zero-knowledge proofs let you prove something is true without revealing the underlying information.

**Analogy:** You and a friend are looking for Wally in a crowded picture. You found him. How do you prove it without showing where he is?

You take a massive piece of paper with a small hole in it. You place it over the picture so only Wally is visible through the hole. Your friend sees Wally and knows you found him — but learns nothing about where he is in the full picture.

**In Omnia:**

- Prove you're over 18 without revealing your birth date
- Prove you have enough money without revealing your balance
- Prove a medicine is authentic without revealing supply chain details

**Current status:** The ZK circuit uses arkworks R1CS + Groth16 + Poseidon hash for proof generation and verification. The Biological shard's `QueryWithZkProof` operation uses a stub verifier that checks consent but does not perform real ZK proof verification.

### Q: How does physical anchoring work?

**A:** The provenance log is fully implemented — it provides an append-only CRDT log for tracking the lifecycle of physical items (create, transfer, verify). This gives every tracked item a cryptographic birth certificate and ownership history. The `PhysicalState` (in `shards/src/physical/state.rs`) maintains a `HashMap<ItemId, Vec<ProvenanceEvent>>` where each entry records the owner, event type, vector clock, and optional metadata.

RF fingerprinting remains a stub requiring real SDR hardware (HackRF/USRP) for production use. The quantum commitment implementation is no longer a stub — it now uses real ML-KEM-768 (FIPS-203 standardized Kyber768) for post-quantum key encapsulation, with constant-time comparisons via `subtle::ConstantTimeEq` (see ADR-020). The hybrid mode combines classical X25519 ECDH with ML-KEM-768 for defense-in-depth.

Physical time anchors (previously described as "Gravitational Timestamps") are not implemented. The protocol currently relies on logical time via vector clocks rather than physical time anchors.

### Q: What's a Decentralized Identifier (DID)?

**A:** A DID is a permanent, self-created digital address that you own forever.

**Format:** `did:omnia:<hex_public_key>` where `<hex_public_key>` is a 64-character hex string representing a 32-byte Ed25519 public key.

**Example:** `did:omnia:ab01cdef0123456789abcdef0123456789abcdef0123456789abcdef01234567`

The validation rules (implemented in `shards/src/identity/did.rs`) are:

- Must start with `did:omnia:` (the `DID_PREFIX` constant)
- The method-specific identifier must be exactly 64 hex characters (32 bytes)
- The hex must be valid (no non-hex characters)

**Properties:**

- You create it yourself (no authority issues it)
- It's cryptographically verifiable
- It cannot be revoked or censored
- It's portable across platforms

The `format_did()` function constructs DIDs from 32-byte public keys:

```rust
// shards/src/identity/did.rs
pub fn format_did(public_key: &[u8; 32]) -> String {
    format!("{}{}", DID_PREFIX, hex::encode(public_key))
}
```

### Q: How does social recovery work?

**A:** Social recovery uses **Shamir's Secret Sharing over GF(256)** (implemented in `shards/src/identity/recovery.rs`). Your private key is split into shares using threshold cryptography, and each share is given to a trusted guardian.

1. Your key is split into N shares using `ShamirRecovery::split(secret, threshold, total)`
2. Any threshold number of shares (e.g., 3 of 5) can reconstruct the key via `ShamirRecovery::reconstruct(shares)`
3. If you lose your key, guardians provide their shares
4. The key is reconstructed from the threshold number of shares using Lagrange interpolation
5. No single guardian has your full key (K-1 shares reveal nothing)

The GF(256) arithmetic uses the AES irreducible polynomial (0x11B) for reduction, ensuring byte-level operations without big-integer arithmetic. The threshold must be at least 2.

### Q: What's Universal Basic Compute (UBC)?

**A:** Every identity on Omnia receives a free monthly quota via the UBC token (implemented in `economics/src/ubc.rs`). The UBC token is soulbound (non-transferable) and provides a baseline of compute and transaction capacity. This ensures participation doesn't require money.

Key parameters (from `economics/src/quota.rs`):

- **Default quota**: 1,000 UBC/month (`DEFAULT_UBC_QUOTA`)
- **Epoch duration**: 30 days (2,592,000,000 ms, `DEFAULT_EPOCH_DURATION_MS`)
- **Monthly reset**: Balances are reset to the monthly quota at each epoch boundary; unspent balance is forfeited (anti-hoarding)
- **Useful-work rewards**: Extra UBC can be earned via `UbcToken::reward()` (additive, not reset)

### Q: How does governance work?

**A:** Quadratic voting with multiplicative reputation decay is currently implemented in `economics/src/governance.rs` and `economics/src/fixed_point.rs`. This means:

- Voting power scales as the integer square root of stake (preventing whale dominance)
- Reputation decays per epoch of inactivity using fixed-point PPM arithmetic (no floating-point)
- Default decay rate: 10% per epoch (`DecayRate::ten_percent()` = 100,000 PPM)
- All decay calculations are bit-for-bit identical across platforms (x86, ARM, etc.)
- After voting, the voter's `last_active` is updated, resetting the decay clock

**Time-locked voting** is also implemented (in `economics/src/time_lock.rs`), preventing flash loan attacks:

- Stake must be locked for a minimum duration (default: 100 blocks) before it grants voting power
- Freshly-locked stake has zero voting power until the lock matures
- Supports multiple concurrent locks per node

**Planned for Phase 1 (not yet implemented):**

- Conviction voting (graduated multipliers based on lock duration)
- Delegation (delegating your vote to a trusted representative)

**AI agent governance:** AI agents with `AgentCapability::GovernanceVote { max_weight }` can participate in governance with a bounded maximum voting weight.

---

## Economic Questions

### Q: How is Omnia currency created?

**A:** Omnia uses the UBC (Universal Basic Compute) token model. UBC tokens are soulbound — they are issued monthly to each identity and cannot be transferred. The `UbcToken::mint_monthly()` method resets the balance to the monthly quota at epoch boundaries. The `UbcToken::spend()` method consumes UBC for transactions (destroyed, not transferred). The `UbcToken::reward()` method adds UBC for useful-work contributions (additive, not reset at epoch boundaries).

Proof-of-useful-work is implemented with 3 work types defined in `economics/src/useful_work.rs`:

- `AiTraining { model_hash, training_data_hash }` — AI model training
- `ScientificSimulation { simulation_id, params_hash }` — Distributed computation
- `DistributedStorage { data_hash, storage_duration }` — Data hosting

Verification is currently a stub (`UsefulWorkProof::verify_stub()`) that checks for non-zero result hash and positive compute units. Reward amount equals compute units consumed (1:1 ratio).

There is no validator reward mechanism or staking system yet. Gradual slashing is implemented (ADR-011) with a 3-tier model: Warning → Jail → Ejection.

### Q: What's the fee structure?

**A:** Fee enforcement is **implemented** via the `FeeSchedule` struct (in `shards/src/fee_schedule.rs`) and the `ShardRouter` (in `shards/src/router.rs`). The standard fee schedule has flat per-operation-type fees:

| Domain            | Fee (UBC) |
| ----------------- | --------- |
| Financial         | 10        |
| Computational     | 5         |
| Physical          | 3         |
| Identity          | 2         |
| Biological        | 3         |
| Cross-Shard       | 15        |
| Economics/Default | 3         |

When `ShardRouter::route_event()` processes an event, it:

1. Deserializes the payload into a `ShardPayload`
2. Checks the nonce for replay protection
3. Looks up the fee via `FeeSchedule::fee_for_op()`
4. Deducts the fee from the caller's UBC quota via `QuotaSystem::spend()`
5. Routes the operation to the target shard

If the caller has insufficient UBC, the operation is rejected with `ShardError::InsufficientFee`.

A `ShardRouter::new_without_fees()` constructor is available for testing.

### Q: Can I convert Omnia to other currencies?

**A:** No DEX integration exists yet. There is currently no way to exchange Omnia tokens for other currencies.

---

## Security Questions

### Q: Is Omnia secure?

**A:** Omnia uses multiple layers of security that are implemented and tested:

| Security Layer                                           | Status                                           |
| -------------------------------------------------------- | ------------------------------------------------ |
| Ed25519 signatures                                       | Implemented                                      |
| BLAKE3 hashing                                           | Implemented                                      |
| BFT consensus (<1/3 faulty nodes)                        | Implemented                                      |
| Replay protection (nonce tracking with redb persistence) | Implemented                                      |
| State commitments (Merkle root)                          | Implemented                                      |
| Event pruning (sustainability)                           | Implemented                                      |
| Fee enforcement (FeeSchedule + QuotaSystem)              | Implemented                                      |
| Time-locked voting (flash loan prevention)               | Implemented                                      |
| Shamir's Secret Sharing social recovery (GF(256))        | Implemented                                      |
| Biometric anchors (BLAKE3 salted commitments)            | Implemented                                      |
| Post-quantum cryptography (ML-KEM-768 / FIPS-203)        | Implemented                                      |
| Gradual slashing (3-tier: Warning → Jail → Ejection)     | Implemented                                      |
| Economic security (staking rewards)                      | Not started                                      |
| Real ZK proofs                                           | Implemented (arkworks R1CS + Groth16 + Poseidon) |

### Q: What if my private key is compromised?

**A:** You can use social recovery via Shamir's Secret Sharing to reconstruct your key from guardian shares. The implementation (in `shards/src/identity/recovery.rs`) supports configurable thresholds (minimum K=2, with configurable N total shares). The `IdentityOp::ConfigureRecovery` operation splits the secret and stores the threshold/total configuration. The `IdentityOp::RecoverDid` operation reconstructs the secret from K+ shares.

The reconstructed secret is used to rotate the DID's public key and authentication methods via `complete_recovery()`, which adds the recovered key to DID authentication (rotation, not replacement). A `recovery_count` is incremented to prevent replay attacks.

---

## Practical Questions

### Q: How do I get started?

**A:** You can interact with Omnia via the Rust library or the `omnia-node` binary with REST API (Sprint 3). There is no wallet and no mobile app yet. To experiment:

1. Clone the repository
2. Run `cargo test --workspace` to see all tests passing
3. Run `cargo run -p omnia-node` to start a node with HTTP health/metrics/Swagger UI
4. Explore the crate APIs in `substrate/`, `shards/`, `binding/`, `economics/`, `zk/`

### Q: Which wallet should I use?

**A:** No wallet exists yet. All interaction is via the Rust library API. A mobile wallet is planned for Phase 1.

### Q: Can I use Omnia on my phone?

**A:** No mobile app exists yet. A mobile wallet is planned for Phase 1.

### Q: How long does a transaction take?

**A:** Performance has not been benchmarked at scale yet. The consensus engine processes only new events each round (O(new_events)), which is designed for low latency, but specific TPS and finality numbers have not been measured.

### Q: How does fast-sync work for new nodes?

**A:** Fast-sync (implemented in `substrate/src/fast_sync.rs`) allows new nodes to skip full genesis replay by downloading a verified state snapshot from peers:

1. **Query peers** for their latest checkpoint (round, state root, event count)
2. **Select a target** checkpoint via supermajority agreement (2/3+ stake must agree)
3. **Download the snapshot** from a peer via P2P request-response
4. **Verify integrity** using BLAKE3 domain-separated hashes (`OMNIA-FAST-SYNC-V1`)
5. **Replay delta events** since the snapshot to reach the current state

If fast-sync fails (no peers, insufficient agreement), the node falls back to genesis replay via `try_sync_or_fallback()`. Fast-sync is enabled when `config.fast_sync && !config.is_genesis`.

### Q: What are the liveness and readiness probes?

**A:** The `omnia-node` binary exposes separate Kubernetes liveness (`/healthz`) and readiness (`/readyz`) endpoints:

- **Liveness** (`/healthz`): Always returns 200 with `{"status": "alive", "node_id", "uptime_seconds"}`. Indicates the process is running. If this fails, Kubernetes restarts the pod.
- **Readiness** (`/readyz`): Returns 200 when the node has peers, is not syncing, and has recent finalization. Returns 503 with `{"status": "not_ready", "reason": "no_peers"|"syncing"|"no_finalization"}` otherwise. If this fails, Kubernetes removes the pod from service but does not restart it.
- **Legacy** (`/health`): Maps to the liveness handler for backward compatibility.

Configuration: `readiness_min_peers` (default: 1) and `readiness_max_finalization_age` (default: 600 rounds) can be tuned via TOML config.

### Q: How can I participate in the trusted setup ceremony?

**A:** The Omnia Protocol uses a Powers of Tau style trusted setup ceremony for the Groth16 ZK circuit. The ceremony is multi-party: each participant contributes randomness to the transcript, and only one honest participant is needed for security. The transcript hash is initialized using BLAKE3 domain separation (`OMNIA-SETUP-TRANSCRIPT-V1`) and includes Fiat-Shamir Proof of Knowledge on BN254 G1 with real EC operations. See `zk/src/setup/contribution.rs` for the implementation.

### Q: How does gradual slashing work?

**A:** The gradual slashing model (ADR-011) replaces the previous binary slash-point system with a 3-tier escalation:

1. **Warning**: Minor offenses (e.g., brief liveness failure). The validator is flagged but remains active.
2. **Jail**: Repeated or moderate offenses. The validator is temporarily suspended from consensus and cannot produce blocks or earn rewards. Auto-release occurs after a configurable jail period.
3. **Ejection**: Severe or persistent offenses (e.g., equivocation). The validator is permanently removed and their stake is partially burned.

This approach avoids the "nothing to lose" perverse incentive of binary slashing, where a validator close to the threshold has no reason not to cause maximum damage.

---

## Long-Term Vision

_The following describes the long-term vision for Omnia. These are aspirational goals, not current capabilities._

### Q: Will Omnia work on Mars?

**A:** This is a long-term vision. Omnia's causal graph consensus is designed to support partitioned operation (local finality with eventual global consistency via `VectorClock::happened_before()` and `CrossShardMessage`), which could in principle work across interplanetary distances. However, no testing or implementation for interplanetary scenarios has been done.

### Q: Will AI agents run Omnia?

**A:** AI agent identity is implemented with 5 capability types (in `shards/src/identity/agent.rs`):

- `FinancialTransfer { max_amount, currency }` — Bounded financial operations
- `DataProcessing { domains, max_records }` — Scoped data access
- `ContractExecution { contract_types }` — Limited contract interaction
- `ComputeProof { max_compute_units }` — Bounded compute submission
- `GovernanceVote { max_weight }` — Bounded governance participation

AI agents can currently have identities on the network with these capabilities. Full governance rights for AI agents and AI-driven decision-making are aspirational goals for Phase 3 (Years 5-10).

---

## Troubleshooting

### Q: I have a question not answered here

**A:**

- Check the documentation: [ARCHITECTURE.md](../architecture/full-spec.md)
- Ask on Discord: [Join our Discord](https://discord.gg/qYkpAeSYR)
- Open an issue: [GitHub Issues](issues)
- Start a discussion: [GitHub Discussions](discussions)

---

🔙 **Back**: [use-cases/](./) | 🔄 **Related**: [governance.md](./governance.md)  
🚀 **Next**: [governance.md](./governance.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

# ❓ Omnia Protocol: Frequently Asked Questions

## 🌌 General Questions

### Q: What is Omnia?

**A:** Omnia is a universal coordination layer for reality — a decentralized protocol that replaces trust with mathematics. It enables value exchange, identity verification, and physical-digital fusion without intermediaries.

Think of it like the internet. No one owns the internet; it's just a set of rules (TCP/IP) that lets computers talk to each other. Omnia is the same idea, but for **value, identity, and trust**.

### Q: Is Omnia a cryptocurrency?

**A:** No. Omnia is a protocol, not a coin. While Omnia has a token model (UBC — Universal Basic Compute), the protocol itself is much broader. It's a framework for:

- 🆔 Identity management
- 📦 Supply chain tracking
- 🤖 AI agent governance
- 💰 Economic coordination
- 🔗 Physical-digital binding

### Q: Who controls Omnia?

**A:** No one. Omnia is decentralized and governed by the community through transparent, mathematical processes. There is no central authority, company, or government that controls it.

### Q: Is Omnia open source?

**A:** Yes. All code is open source and available on GitHub. The protocol is public domain (CC0), meaning anyone can use, modify, or build on it.

### Q: What is the current state of the project? 📊

**A:** All 5 core layers are implemented and tested (278+ tests passing). The protocol has causal graph consensus, domain shards, a binding layer with provenance tracking, identity hardening with DIDs and Shamir's Secret Sharing, and an economics layer with UBC and quadratic voting. The ZK-rollup settlement layer uses real arkworks R1CS + Groth16 proofs on BN254 with Merkle path verification. Real PQC signatures (Dilithium) and fee enforcement are implemented. Sprint 3 added a binary entrypoint (`omnia-node`), REST API with Swagger UI, persistent slashing (sled), chaos testing, and TLA+ formal verification. Some features like real RF fingerprinting remain ⚠️ stubs awaiting hardware. There is 🌑 no mobile wallet and 🌑 no validator network yet. The ExpandedRollupCircuit uses a simplified field-addition hash placeholder (needs Pedersen/Poseidon for production).

### Q: How do I run the tests? 🧪

**A:** Clone the repository and run:

```bash
git clone https://github.com/Willow7737/omnia-protocol.git
cd omnia-protocol
cargo test --workspace
```

You should see 200+ tests passing.

---

## ⚙️ Technical Questions

### Q: How does causal graph consensus work? 🧠

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

### Q: What are zero-knowledge proofs? 🔐

**A:** Zero-knowledge proofs let you prove something is true without revealing the underlying information.

**Analogy:** You and a friend are looking for Wally in a crowded picture. You found him. How do you prove it without showing where he is?

You take a massive piece of paper with a small hole in it. You place it over the picture so only Wally is visible through the hole. Your friend sees Wally and knows you found him — but learns nothing about where he is in the full picture.

**In Omnia:**
- Prove you're over 18 without revealing your birth date
- Prove you have enough money without revealing your balance
- Prove a medicine is authentic without revealing supply chain details

**Current status:** ⚠️ The ZK circuit is currently a stub using hash chains. Full arkworks R1CS circuit implementation is the production target.

### Q: How does physical anchoring work? 🔗

**A:** The provenance log is ✅ fully implemented — it provides an append-only CRDT log for tracking the lifecycle of physical items (create, transfer, verify, destroy). This gives every tracked item a cryptographic birth certificate and ownership history.

⚠️ RF fingerprinting and quantum commitments are stubs. The RF fingerprinting stub uses Hamming distance comparison but requires real SDR hardware (HackRF/USRP) for production use. The quantum commitment stub uses a hybrid classical + PQC placeholder but requires CRYSTALS-Dilithium integration for real post-quantum security.

🌑 Physical time anchors (previously described as "Gravitational Timestamps") are not implemented. The protocol currently relies on logical time via vector clocks rather than physical time anchors.

### Q: What's a Decentralized Identifier (DID)? 🆔

**A:** A DID is a permanent, self-created digital address that you own forever.

**Format:** `did:omnia:z6MkhaXgBZDvotDkL5257faWxcqACaGVJRPn92ND5CHXvP`

**Properties:**
- You create it yourself (no authority issues it)
- It's cryptographically verifiable
- It cannot be revoked or censored
- It's portable across platforms

The `did:omnia:` method is ✅ fully implemented with validation.

### Q: How does social recovery work? 🛡️

**A:** Social recovery uses **Shamir's Secret Sharing over GF(256)**. Your private key is split into shares using threshold cryptography, and each share is given to a trusted guardian.

1. Your key is split into N shares using Shamir's Secret Sharing
2. Any threshold number of shares (e.g., 3 of 5) can reconstruct the key
3. If you lose your key, guardians provide their shares
4. The key is reconstructed from the threshold number of shares
5. No single guardian has your full key

### Q: What's Universal Basic Compute (UBC)? 💻

**A:** Every identity on Omnia receives a free monthly quota via the UBC token. The UBC token is soulbound (non-transferable) and provides a baseline of compute and transaction capacity. This ensures participation doesn't require money.

The quota system operates on epochs with automatic advancement. The specific quota amounts are configurable parameters in the economics layer.

### Q: How does governance work? 🗳️

**A:** ✅ **Quadratic voting with exponential reputation decay is currently implemented.** This means:

- Voting power scales as the square root of stake (preventing whale dominance)
- Reputation decays exponentially over time (preventing permanent power concentration)

📋 **Planned for Phase 1 (not yet implemented):**
- Conviction voting (locking tokens for longer periods to increase voting power)
- Delegation (delegating your vote to a trusted representative)

---

## 💰 Economic Questions

### Q: How is Omnia currency created?

**A:** Omnia uses the UBC (Universal Basic Compute) token model. UBC tokens are soulbound — they are issued monthly to each identity and cannot be transferred. The token provides quota for transactions and compute.

⚠️ Proof-of-useful-work stubs exist (3 work types defined) but are not production-ready. 🌑 There is no validator reward mechanism or staking system yet. 🌑 Slashing is not implemented.

### Q: What's the fee structure?

**A:** 🌑 There is no fee mechanism implemented yet. The UBC quota system covers transaction costs for all participants. A fee mechanism for high-frequency or commercial use is planned but not yet started.

### Q: Can I convert Omnia to other currencies?

**A:** 🌑 No DEX integration exists yet. There is currently no way to exchange Omnia tokens for other currencies.

---

## 🛡️ Security Questions

### Q: Is Omnia secure?

**A:** Omnia uses multiple layers of security that are implemented and tested:

| Security Layer | Status |
|---------------|--------|
| ✅ Ed25519 signatures | Implemented |
| ✅ BLAKE3 hashing | Implemented |
| ✅ BFT consensus (<1/3 faulty nodes) | Implemented |
| ✅ Replay protection (nonce tracking) | Implemented |
| ✅ State commitments (Merkle root) | Implemented |
| ✅ Event pruning (sustainability) | Implemented |
| 🔄 Post-quantum cryptography (Dilithium) | Stub |
| 🌑 Economic security (slashing, staking) | Not started |
| 🌑 Real ZK proofs | Stub (hash chain) |

### Q: What if my private key is compromised?

**A:** You can use social recovery via Shamir's Secret Sharing to reconstruct your key from guardian shares. The implementation supports configurable thresholds (e.g., 3 of 5 guardians).

---

## 🛠️ Practical Questions

### Q: How do I get started? 🚀

**A:** You can interact with Omnia via the Rust library or the `omnia-node` binary with REST API (Sprint 3). There is 🌑 no wallet and 🌑 no mobile app yet. To experiment:

1. Clone the repository
2. Run `cargo test --workspace` to see all tests passing
3. Run `cargo run -p omnia-node` to start a node with HTTP health/metrics/Swagger UI
4. Explore the crate APIs in `substrate/`, `shards/`, `binding/`, `economics/`, `zk/`

### Q: Which wallet should I use? 👛

**A:** 🌑 No wallet exists yet. All interaction is via the Rust library API. A mobile wallet is planned for Phase 1.

### Q: Can I use Omnia on my phone? 📱

**A:** 🌑 No mobile app exists yet. A mobile wallet is planned for Phase 1.

### Q: How long does a transaction take? ⏱️

**A:** ⚠️ Performance has not been benchmarked at scale yet. The consensus engine processes only new events each round (O(new_events)), which is designed for low latency, but specific TPS and finality numbers have not been measured.

---

## 🔮 Long-Term Vision

*The following describes the long-term vision for Omnia. These are aspirational goals, not current capabilities.*

### Q: Will Omnia work on Mars? 🚀

**A:** This is a long-term vision 🔮. Omnia's causal graph consensus is designed to support partitioned operation (local finality with eventual global consistency), which could in principle work across interplanetary distances. However, no testing or implementation for interplanetary scenarios has been done.

### Q: Will AI agents run Omnia? 🤖

**A:** ✅ AI agent identity is implemented (5 capability types). AI agents can currently have identities on the network. 🔮 Full governance rights for AI agents and AI-driven decision-making are aspirational goals for Phase 3 (Years 5-10).

---

## 🐛 Troubleshooting

### Q: I have a question not answered here ❓

**A:**
- 📖 Check the documentation: [ARCHITECTURE.md](../ARCHITECTURE.md)
- 💬 Ask on Discord: [Join our Discord](https://discord.gg/qYkpAeSYR)
- 🐛 Open an issue: [GitHub Issues](https://github.com/Willow7737/omnia-protocol/issues)
- 💡 Start a discussion: [GitHub Discussions](https://github.com/Willow7737/omnia-protocol/discussions)

---

**Last Updated:** May 2026

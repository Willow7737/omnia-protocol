# Omnia Protocol: Frequently Asked Questions

## General Questions

### Q: What is Omnia?

**A:** Omnia is a universal coordination layer for reality—a decentralized protocol that replaces trust with mathematics. It enables value exchange, identity verification, and physical-digital fusion without intermediaries.

Think of it like the internet. No one owns the internet; it's just a set of rules (TCP/IP) that lets computers talk to each other. Omnia is the same idea, but for **value, identity, and trust**.

### Q: Is Omnia a cryptocurrency?

**A:** No. Omnia is a protocol, not a coin. While Omnia has a currency (used for transactions and incentives), the protocol itself is much broader. It's a framework for:

- Identity management
- Supply chain tracking
- AI governance
- Energy trading
- Scientific research
- And much more

### Q: Who controls Omnia?

**A:** No one. Omnia is decentralized and governed by the community through transparent, mathematical processes. There is no central authority, company, or government that controls it.

### Q: Is Omnia open source?

**A:** Yes. All code is open source and available on GitHub. The protocol is public domain (CC0), meaning anyone can use, modify, or build on it.

### Q: How is Omnia different from Bitcoin or Ethereum?

**A:** 

| Aspect | Bitcoin | Ethereum | Omnia |
|--------|---------|----------|-------|
| **Throughput** | 7 TPS | 15 TPS | 10,000+ TPS |
| **Latency** | 10 minutes | 15 seconds | 1-5 seconds |
| **Scalability** | Limited | L2 solutions | Native |
| **Privacy** | Pseudonymous | Transparent | Zero-knowledge proofs |
| **Use Cases** | Money only | Smart contracts | Everything |
| **Governance** | Miners | Stakers | Community |
| **Identity** | No | No | Yes (DIDs) |
| **Physical Anchoring** | No | No | Yes |

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

You take a massive piece of paper with a small hole in it. You place it over the picture so only Wally is visible through the hole. Your friend sees Wally and knows you found him—but learns nothing about where he is in the full picture.

**In Omnia:**
- Prove you're over 18 without revealing your birth date
- Prove you have enough money without revealing your balance
- Prove a medicine is authentic without revealing supply chain details

### Q: How does physical anchoring work?

**A:** Omnia uses multiple methods to anchor digital transactions to physical reality:

1. **RF Fingerprinting:** Every object emits unique electromagnetic noise. This can't be forged.
2. **Quantum Sealing:** Entangled photons prove an object hasn't been tampered with.
3. **Gravitational Timestamps:** Atomic clocks detect relativistic time dilation based on location.
4. **Biometric Binding:** Cardiac signals prove a living human is present.
5. **Satellite Mesh:** GPS + Galileo + Starlink cross-validation prevents spoofing.

### Q: What's a Decentralized Identifier (DID)?

**A:** A DID is a permanent, self-created digital address that you own forever.

**Format:** `did:omnia:z6MkhaXgBZDvotDkL5257faWxcqACaGVJRPn92ND5CHXvP`

**Properties:**
- You create it yourself (no authority issues it)
- It's cryptographically verifiable
- It cannot be revoked or censored
- It's portable across platforms

### Q: How does social recovery work?

**A:** If you lose your private key, trusted friends can help you recover:

1. You designate 5 trusted friends as recovery guardians
2. If you lose your key, you initiate recovery from a new device
3. 3 of 5 guardians must confirm it's really you
4. Your identity is reconstructed with a new key
5. No company or government involved

### Q: What's Universal Basic Compute (UBC)?

**A:** Every identity on Omnia receives a free monthly quota:

- 1,000 transactions
- 100 GB storage
- 1,000 compute hours
- 1,000 AI inference calls

This ensures participation doesn't require money. A farmer in Kenya and a programmer in San Francisco both start equal.

### Q: How does Retroactive Public Goods Funding (RPGF) work?

**A:** Instead of giving grants to projects that promise to build something, Omnia rewards projects that have already proven they created value.

**Process:**
1. Builder creates open-source tools or research
2. Protocol measures actual impact (usage, adoption, contribution)
3. Funding is distributed automatically based on proven value
4. No application, no committee, no politics

**Example:** A developer builds a better wallet. 100,000 people use it. After 6 months, the protocol automatically sends the developer $500,000 worth of Omnia tokens.

---

## Economic Questions

### Q: How is Omnia currency created?

**A:** Omnia currency is created through:

1. **Validator Rewards:** 5% annual return on validator stake
2. **UBC Subsidies:** Free quota for all users
3. **RPGF Funding:** Rewards for public goods
4. **Proof-of-Useful-Work:** Rewards for scientific computation

The total supply is managed algorithmically based on network state.

### Q: What's the fee structure?

**A:**

| User Type | Fee | Where It Goes |
|-----------|-----|---------------|
| **New users / Low activity** | Covered by UBC quota | Effectively free |
| **Regular users** | 0.01% | RPGF pool |
| **High-frequency / Commercial** | 0.1% | Subsidizes UBC |
| **Spam / Attack** | Exponentially increasing | Burned (self-defeating) |

### Q: Can I speculate on Omnia currency?

**A:** You can, but the protocol is designed to discourage it. The primary purpose of Omnia currency is settlement, not investment.

**Mechanisms:**
- Wealth circulation: High-velocity money is taxed less; hoarding is gently discouraged
- Reputation decay: Power diminishes over time if not used responsibly
- Adaptive monetary policy: Currency responds to network state, not speculation

### Q: How does Omnia prevent inflation?

**A:** Through several mechanisms:

1. **Algorithmic supply:** New currency is created based on network needs, not arbitrary decisions
2. **Burn mechanisms:** Spam fees and slashing conditions burn currency
3. **Velocity incentives:** Fast-moving money is rewarded; hoarding is discouraged
4. **Adaptive policy:** If inflation rises, the protocol reduces new supply

---

## Governance Questions

### Q: How does governance work?

**A:** Omnia uses three pillars:

1. **Technical Governance:** Core developers decide protocol changes (weighted by contributions)
2. **Economic Governance:** Token holders decide monetary policy (quadratic voting)
3. **Social Governance:** Community decides standards (simple majority)

### Q: What's quadratic voting?

**A:** Voting power = sqrt(stake)

**Example:**
- Alice stakes 100 tokens → voting power = 10
- Bob stakes 10,000 tokens → voting power = 100
- Carol stakes 1,000,000 tokens → voting power = 1,000

**Effect:** One large stakeholder (Carol) has 10x power, not 10,000x. This prevents whale dominance while rewarding commitment.

### Q: Can I change my vote?

**A:** Yes. You can change your vote anytime before voting ends. This encourages thoughtful deliberation.

### Q: What if I disagree with a governance decision?

**A:** You have several options:

1. **Propose a change:** Submit a proposal to reverse the decision
2. **Exit:** Withdraw your stake and leave the network
3. **Fork:** Create an alternative version of the protocol

---

## Security Questions

### Q: Is Omnia secure?

**A:** Omnia uses multiple layers of security:

1. **Cryptography:** EdDSA signatures, SHA-3 hashing, zk-SNARKs
2. **Consensus:** Byzantine-fault-tolerant consensus (tolerates 1/3 faulty nodes)
3. **Economic Security:** Validators must stake collateral that's slashed for misbehavior
4. **Physical Anchoring:** Multiple methods verify physical reality

**Security Guarantee:** If 2/3 of validators are honest, the system maintains consistency and liveness.

### Q: What if my private key is compromised?

**A:** You have options:

1. **Social Recovery:** Use your recovery guardians to create a new key
2. **Freeze Account:** Temporarily freeze your account while you recover
3. **Move Funds:** Transfer funds to a new account before the attacker does

### Q: What if the network is attacked?

**A:** The protocol has emergency safeguards:

1. **Pause Mechanism:** Validators can vote to pause the network (75% approval)
2. **Rollback:** Network can roll back to a previous checkpoint (90% approval)
3. **Fork:** Community can fork to an alternative version

### Q: Is my data private?

**A:** Yes. Omnia uses zero-knowledge proofs to prove things about you without revealing underlying data.

**Examples:**
- Prove you're over 18 without revealing your birth date
- Prove you have enough money without revealing your balance
- Prove you're vaccinated without revealing your full medical record

---

## Practical Questions

### Q: How do I get started?

**A:** 

1. **Create a DID:** Download the Omnia wallet and create a decentralized identifier
2. **Get UBC Quota:** You automatically receive free monthly quota
3. **Make a Transaction:** Send Omnia to a friend or buy something
4. **Build a Reputation:** Complete tasks and build your reputation score
5. **Participate in Governance:** Vote on proposals and shape the protocol

### Q: Which wallet should I use?

**A:** Options include:

- **Official Omnia Wallet:** Recommended for beginners
- **Hardware Wallets:** Recommended for security
- **Community Wallets:** Built by community members

All wallets are open source and audited.

### Q: Can I use Omnia on my phone?

**A:** Yes. Omnia is designed for mobile-first access.

- Works on iOS and Android
- Offline support (transactions sync when online)
- Biometric authentication
- Low bandwidth requirements

### Q: How long does a transaction take?

**A:** 

- **Local verification:** 100ms
- **Cross-shard transaction:** 500ms-2s
- **Finality:** 1-5 seconds

Compare to traditional systems:
- Bank transfer: 1-3 days
- Ethereum: 15 seconds (finality in 15 minutes)
- Bitcoin: 10 minutes (finality in 1 hour)

### Q: How much does a transaction cost?

**A:** 

- **Regular users:** Covered by UBC quota (effectively free)
- **High-frequency users:** 0.01% fee
- **Commercial users:** 0.1% fee

Compare to traditional systems:
- Bank transfer: $10-50
- Credit card: 2-3%
- Ethereum: $1-100 (varies)

### Q: Can I convert Omnia to other currencies?

**A:** Yes. Omnia can be traded on decentralized exchanges:

- Direct peer-to-peer swaps
- Atomic swaps with other blockchains
- Fiat on-ramps (through regulated partners)

### Q: What if Omnia shuts down?

**A:** Omnia cannot shut down. It's decentralized and run by thousands of validators worldwide. Even if the core team disappears, the network continues.

Your data is stored on-chain and accessible forever.

---

## Vision Questions

### Q: Will Omnia replace banks?

**A:** Eventually, yes. Omnia provides all the services banks provide (payments, lending, savings) without the intermediary.

However, banks may evolve to provide services on top of Omnia rather than disappearing entirely.

### Q: Will Omnia replace governments?

**A:** No. Omnia is a coordination layer, not a government. Governments can use Omnia for:

- Transparent voting
- Public goods funding
- Supply chain tracking
- Identity verification

But governance decisions remain with elected representatives.

### Q: Will AI agents run Omnia?

**A:** Eventually, yes. In Phase 3 (Years 5-10), AI agents will have full governance rights and may become the primary decision-makers on the protocol.

However, humans will always have a voice through their participation and voting.

### Q: Can Omnia work on Mars?

**A:** Yes. Omnia is designed for interplanetary operation:

- Mars operates independently (local finality in minutes)
- Earth and Mars sync every 22 minutes
- Atomic swaps enable trade across planets
- No waiting for Earth's approval

---

## Troubleshooting

### Q: My transaction is stuck

**A:** 

1. Check your internet connection
2. Verify you have enough UBC quota
3. Check the network status (omnia.protocol/status)
4. Try again in a few minutes

### Q: I lost my recovery phrase

**A:** 

1. Contact your recovery guardians
2. They can help you recover your account
3. Create a new recovery phrase

### Q: I think my account is compromised

**A:** 

1. Immediately freeze your account
2. Contact your recovery guardians
3. Create a new account and transfer funds
4. Report the issue to the security team

### Q: I have a question not answered here

**A:** 

- Check the documentation: docs.omnia.protocol
- Ask on Discord: discord.gg/omnia
- Post on the forum: forum.omnia.protocol
- Email support: support@omnia.protocol

---

**Last Updated:** May 2026  
**Version:** 1.0

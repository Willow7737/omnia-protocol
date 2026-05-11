# Omnia Protocol Architecture

> [!IMPORTANT]
> This document has been superseded by the new [**Architecture Documentation**](../../ARCHITECTURE.md) which includes Layer 1 Substrate specifications.

## Table of Contents

## Table of Contents

1. [System Overview](#system-overview)
2. [Layer 1: The Substrate](#layer-1-the-substrate)
3. [Layer 2: Domain Shards](#layer-2-domain-shards)
4. [Layer 3: The Binding Layer](#layer-3-the-binding-layer)
5. [Layer 4: Identity Layer](#layer-4-identity-layer)
6. [Layer 5: Economic Layer](#layer-5-economic-layer)
7. [Cross-Layer Interactions](#cross-layer-interactions)
8. [Consensus Mechanism](#consensus-mechanism)
9. [Scalability & Performance](#scalability--performance)
10. [Security Model](#security-model)

---

## System Overview

Omnia is a five-layer distributed system designed to enable trustless coordination at global and interplanetary scales.

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 5: Economic Layer                                    │
│  (UBC, RPGF, Adaptive Monetary Policy)                     │
├─────────────────────────────────────────────────────────────┤
│  Layer 4: Identity Layer                                    │
│  (DIDs, Verifiable Credentials, Reputation)                │
├─────────────────────────────────────────────────────────────┤
│  Layer 3: Binding Layer                                     │
│  (Physical Anchoring, Oracles Eliminated)                  │
├─────────────────────────────────────────────────────────────┤
│  Layer 2: Domain Shards                                     │
│  (Financial, Computational, Physical, Biological, etc.)    │
├─────────────────────────────────────────────────────────────┤
│  Layer 1: The Substrate                                     │
│  (Causal Graph Consensus, Vector Clocks)                   │
└─────────────────────────────────────────────────────────────┘
```

---

## Layer 1: The Substrate

### Purpose

The foundation that enables the network to agree on what happened without relying on global clock time or a single authority.

### Key Components

#### Causal Graph Consensus

Instead of organizing events into sequential blocks, Omnia maintains a **directed acyclic graph (DAG)** where:

- Each event (transaction) is a node
- Edges represent causal relationships (event A must happen before event B)
- Unrelated events can be processed in parallel
- The graph naturally captures causality without artificial ordering

**Advantages:**
- Transactions that don't depend on each other can be finalized independently
- Network latency does not block unrelated transactions
- Throughput scales with network size, not down

#### Vector Clocks

Each node maintains a **vector clock**—a data structure that tracks what it has seen from every other node.

```
Node A's vector clock: [3, 2, 5, 1]
                        ↓  ↓  ↓  ↓
                    A's B's C's D's
                    events events events events
```

**Properties:**
- If `VC_A < VC_B` (component-wise), then event A causally precedes event B
- If neither `VC_A < VC_B` nor `VC_B < VC_A`, the events are concurrent
- Nodes can determine ordering without global synchronization

#### Conflict-Free Replicated Data Types (CRDTs)

For state that requires convergence (like account balances), Omnia uses CRDTs that:

- Allow concurrent updates without coordination
- Guarantee that all nodes eventually reach the same state
- Provide deterministic merge semantics

**Example:** A counter CRDT where each node increments its own counter, and the global sum is the sum of all counters.

### Relativistic Boundaries

For interplanetary operation, the protocol acknowledges that communication has physical limits:

- Earth-to-Mars: 3-22 minutes one way
- Mars-to-Jupiter: 5-60 minutes one way

**Solution:** Each region maintains its own causal graph and periodically synchronizes with other regions. A node on Mars does not wait for Earth; it processes what it knows and reconciles later.

---

## Layer 2: Domain Shards

### Purpose

Organize different types of activity into specialized lanes, each with optimized consensus and state management.

### Architecture

Each domain shard is a **projection of the unified state** that:

- Maintains its own state tree
- Processes transactions relevant to its domain
- Can reference state from other shards atomically
- Contributes to the global state root

### Domain Specifications

#### Financial Shard

**Handles:** Money, assets, derivatives, lending, insurance

**State Structure:**
```
Account {
  address: PublicKey
  balance: Amount
  nonce: u64
  commitments: [ZKCommitment]
}
```

**Operations:**
- Transfer (with zero-knowledge proof)
- Atomic swaps
- Collateralized lending
- Derivative settlement

#### Computational Shard

**Handles:** AI training, rendering, proofs, scientific computation

**State Structure:**
```
ComputeJob {
  id: UUID
  owner: DID
  work_description: String
  proof_of_work: Proof
  reward: Amount
  status: JobStatus
}
```

**Operations:**
- Register compute work
- Submit proof of computation
- Distribute rewards
- Verify useful work

#### Physical Shard

**Handles:** Supply chain, real estate, minerals, physical objects

**State Structure:**
```
PhysicalObject {
  id: UUID
  rf_fingerprint: String
  quantum_seal: QuantumProof
  location: GPS
  ownership_chain: [Transaction]
  metadata: JSON
}
```

**Operations:**
- Register physical object
- Transfer ownership
- Update location
- Verify authenticity

#### Biological Shard

**Handles:** Health records, genomics, biometric data

**State Structure:**
```
BiologicalRecord {
  subject: DID
  record_type: String
  zero_knowledge_proof: ZKProof
  timestamp: u64
  issuer: DID
}
```

**Operations:**
- Issue verifiable health credentials
- Prove medical status without revealing details
- Manage genomic data with privacy
- Track vaccination status

#### Energy Shard

**Handles:** Grid trading, carbon credits, battery management

**State Structure:**
```
EnergyAsset {
  id: UUID
  type: EnergyType
  amount: Amount
  source: String
  carbon_footprint: Amount
  owner: DID
}
```

**Operations:**
- Trade energy peer-to-peer
- Tokenize carbon credits
- Manage distributed batteries
- Optimize grid load

#### Temporal Shard

**Handles:** Futures, predictions, time-locked contracts

**State Structure:**
```
TemporalContract {
  id: UUID
  condition: String
  execution_time: u64
  parties: [DID]
  settlement: Transaction
}
```

**Operations:**
- Create futures contracts
- Register conditional execution
- Settle based on oracle data
- Manage time-locked assets

#### Identity Shard

**Handles:** Humans, AI agents, machines, collectives

**State Structure:**
```
Identity {
  did: String
  type: IdentityType
  credentials: [VerifiableCredential]
  reputation: ReputationScore
  recovery_guardians: [DID]
}
```

**Operations:**
- Create and manage DIDs
- Issue verifiable credentials
- Update reputation
- Execute social recovery

### Cross-Shard Transactions

A single transaction can atomically touch multiple shards:

**Example:** AI Agent Loan for Compute

1. **Identity Shard:** Verify AI agent identity and reputation
2. **Financial Shard:** Create loan with collateral
3. **Computational Shard:** Allocate compute resources
4. **Energy Shard:** Reserve energy for computation
5. **Temporal Shard:** Schedule repayment

All steps execute atomically or all fail. No partial state.

---

## Layer 3: The Binding Layer

### Purpose

Anchor the digital system to physical reality without requiring trusted intermediaries (oracles).

### Physical Anchoring Methods

#### RF Fingerprinting

Every physical object emits unique electromagnetic noise due to manufacturing imperfections.

**How it works:**
1. Manufacturer measures RF signature during production
2. Signature is hashed and stored on Omnia
3. Owner can verify the object by measuring its RF signature
4. RF signature cannot be forged without recreating the exact manufacturing defect

**Use case:** Verify authentic luxury goods, pharmaceuticals, electronics

#### Quantum Sealing

Entangled photon pairs are generated and distributed at manufacturing.

**How it works:**
1. Manufacturer creates entangled photons
2. One photon is embedded in the product; one is stored in a secure facility
3. Any tampering breaks the entanglement
4. Measurement of the stored photon proves the product is untampered

**Use case:** High-security items, critical infrastructure, weapons

#### Gravitational Timestamps

Atomic clocks detect relativistic time dilation based on altitude and velocity.

**How it works:**
1. Device contains atomic clock
2. Gravitational time dilation is measured
3. Measurement proves device's altitude and velocity
4. Cannot be spoofed without being at the claimed location

**Use case:** Verify drone location, satellite position, aircraft altitude

#### Biometric Binding

Cardiac electrical patterns (not fingerprints) prove a living human is present.

**How it works:**
1. Each human's cardiac signal is unique and cannot be replicated
2. Device measures cardiac signal via contact
3. Signal is compared to registered pattern
4. Proves a living human, not a replica or deepfake

**Use case:** Verify human identity, prevent AI impersonation, prove liveness

#### Satellite Mesh

GPS + Galileo + Starlink cross-validation prevents spoofing.

**How it works:**
1. Device receives signals from multiple satellite systems
2. Signals are cross-checked for consistency
3. Any spoofing attempt creates inconsistency
4. Consensus across systems proves true location

**Use case:** Verify location for supply chain, autonomous vehicles, emergency services

### Example: Ethical Diamonds

1. **Mining:** Diamond is extracted. RF fingerprint is recorded. Quantum seal is applied.
2. **Cutting:** Cutter signs the transaction with hardware attestation. RF fingerprint is updated.
3. **Polishing:** Polisher signs. RF fingerprint is verified.
4. **Shipping:** Container has quantum seal and GPS tracking via satellite mesh.
5. **Retail:** Jeweler verifies all signatures and physical properties.
6. **Consumer:** Scans ring. Full chain is visible: mine → cutter → polisher → shipper → jeweler → you.

**Result:** Complete provenance without trusting any middleman.

---

## Layer 4: Identity Layer

### Purpose

Enable self-sovereign identity where individuals, AI agents, and collectives own their identity forever.

### Components

#### Decentralized Identifiers (DIDs)

A DID is a permanent, self-created digital address.

**Format:** `did:omnia:z6MkhaXgBZDvotDkL5257faWxcqACaGVJRPn92ND5CHXvP`

**Properties:**
- Created by the user, not issued by any authority
- Cryptographically verifiable
- Cannot be revoked or censored
- Portable across platforms

**Creation:**
```
1. User generates a keypair (public + private key)
2. User hashes the public key to create the DID
3. User stores the private key securely
4. DID is registered on Omnia (immutable)
```

#### Verifiable Credentials

Digital certificates that prove things about you without revealing underlying data.

**Example:** Age Verification

```
Credential {
  issuer: did:omnia:...
  subject: did:omnia:...
  claim: "over_18"
  proof: ZKProof
  expiration: 2027-05-10
}
```

**Properties:**
- Issued by trusted parties (governments, universities, companies)
- Cryptographically signed
- Can be presented without revealing unnecessary details
- Revocable by issuer if needed

#### Social Recovery

If you lose your private key, trusted friends can help you recover.

**Process:**
1. You designate 5 trusted friends as recovery guardians
2. If you lose your key, you initiate recovery from a new device
3. 3 of 5 guardians must confirm it is really you
4. Your identity is reconstructed with a new key
5. No company, no government involved

**Security:** Requires majority of guardians, time-locked to prevent abuse

#### Reputation System

Each identity has a reputation score that reflects trustworthiness.

**Factors:**
- Transaction history
- Credential issuance
- Community votes
- Time-weighted (recent behavior matters more)

**Properties:**
- Reputation decays over time if not maintained
- Cannot be bought (quadratic voting prevents whale dominance)
- Transparent and auditable

### AI Agent Identity

AI agents are first-class citizens with their own DIDs.

**Requirements:**
1. **Proof-of-Compute:** Agent must prove it performed real training/inference work
2. **Behavioral Watermarking:** Each AI has a unique fingerprint in how it generates content
3. **Training Provenance:** Every decision traces to the data and compute that created the model
4. **Stake:** Agent must put up collateral that is burned if it acts maliciously

**Example:** An AI agent trained on medical data can prove:
- It was trained on specific datasets
- It has not been modified since training
- It will stake 1,000 Omnia on its accuracy
- If it makes a harmful recommendation, the stake is burned

### Collective Identity

DAOs, communities, and organizations have collective DIDs.

**Governance:**
- **Quadratic Voting:** Voting power = sqrt(stake), preventing whale dominance
- **Reputation Decay:** Power diminishes over time if not used responsibly
- **Transparent Voting:** All votes are recorded and auditable

---

## Layer 5: Economic Layer

### Purpose

Create a monetary system that serves people, not extracts from them.

### Universal Basic Compute (UBC)

Every identity receives a free monthly quota:

- **Transactions:** 1,000 free transactions/month
- **Storage:** 100 GB free storage/month
- **Compute:** 1,000 compute hours/month
- **AI Inference:** 1,000 inference calls/month

**Funding:** Subsidized by fees from high-frequency users and commercial entities

**Effect:** Participation does not require money. A farmer in Kenya and a programmer in San Francisco both start equal.

### Retroactive Public Goods Funding (RPGF)

Instead of giving grants to projects that promise to build something, Omnia rewards projects that have already proven they created value.

**Process:**
1. Builder creates open-source tools, infrastructure, or research
2. Protocol measures actual impact (usage, adoption, contribution)
3. Funding is distributed automatically based on proven value
4. No application, no committee, no politics

**Example:** A developer builds a better wallet for Omnia. 100,000 people use it. After 6 months, the protocol automatically sends the developer $500,000 worth of Omnia tokens.

**Effect:** Builders get rich by helping everyone. Exit-like incentives for public goods.

### Adaptive Monetary Policy

The currency responds to the state of the network:

**Energy Crisis:**
- Protocol weights toward energy-backed reserves
- Energy tokens get priority routing
- Compute costs increase to conserve energy

**Compute Scarcity:**
- Proof-of-compute becomes the dominant settlement mechanism
- Compute tokens get higher valuation
- UBC compute quota decreases slightly

**Biological Emergency:**
- Medical resource tokens get priority routing
- Healthcare providers get subsidized fees
- Emergency protocols activate

**Implementation:** The protocol reads the state of all shards and adjusts algorithmically. No central authority makes decisions.

### Fee Structure

| User Type | Fee | Where It Goes |
|-----------|-----|---------------|
| **New users / Low activity** | Covered by UBC quota | Effectively free |
| **Regular users** | 0.01% | RPGF pool |
| **High-frequency / Commercial** | 0.1% | Subsidizes UBC |
| **Spam / Attack** | Exponentially increasing | Burned (self-defeating) |

---

## Cross-Layer Interactions

### Example: Coffee Supply Chain

**Scenario:** A farmer in Ethiopia sells coffee beans to a roaster in Portland, who sells to a consumer in New York.

**Layer 1 (Substrate):** Causal graph tracks the sequence of events without requiring global synchronization.

**Layer 2 (Domain Shards):**
- **Physical Shard:** Coffee batch gets RF fingerprint and quantum seal
- **Financial Shard:** Farmer receives payment; roaster receives beans
- **Energy Shard:** Carbon footprint is calculated and tokenized
- **Identity Shard:** Farmer's reputation increases; roaster's increases

**Layer 3 (Binding):** Every handoff is verified:
- Farmer's location verified via satellite mesh
- Container's quantum seal confirms no tampering
- Temperature sensors prove proper storage
- Roasting profile is cryptographically signed

**Layer 4 (Identity):** All parties are verified:
- Farmer has DID and verifiable credentials
- Roaster has DID and reputation score
- Consumer can verify farmer's identity

**Layer 5 (Economic):**
- Farmer receives fair-trade price (verified)
- Roaster's fee is 0.01% (subsidized by commercial users)
- Consumer sees full provenance with one tap
- Farmer's reputation increases, enabling future loans

**Result:** Complete transparency, no intermediaries, mathematically guaranteed trust.

---

## Consensus Mechanism

### Causal+ Consistency

Omnia implements **Causal+ Consistency**, which guarantees:

1. **Causality:** If event A causally precedes event B, all nodes see A before B
2. **Consistency:** All nodes eventually see the same state
3. **Liveness:** The system continues to make progress even if some nodes are offline

### Finality

A transaction is final when:

1. It is included in the causal graph
2. It is referenced by at least 2/3 of validator nodes
3. No conflicting transaction has been finalized

**Time to finality:** Typically 1-5 seconds depending on network latency

### Fork Resolution

If the network splits into two partitions:

1. Each partition continues to process transactions independently
2. When the partition heals, transactions are merged using CRDT semantics
3. Conflicting transactions are resolved by causal ordering
4. If no causal ordering exists, the transaction with more validator support wins

---

## Scalability & Performance

### Throughput

**Theoretical maximum:** Depends on domain shard parallelization

- **Financial transactions:** 100,000+ TPS (limited by cryptographic verification)
- **Computational tasks:** Unlimited (no consensus required for proof-of-work)
- **Physical updates:** 10,000+ TPS (limited by sensor data ingestion)

**Practical:** 10,000-50,000 TPS across all shards combined

### Latency

- **Local transaction:** 100ms (local verification)
- **Cross-shard transaction:** 500ms-2s (requires coordination)
- **Finality:** 1-5 seconds (validator consensus)

### Storage

- **Full node:** 1 TB/year (compressed causal graph)
- **Light node:** 10 GB/year (only relevant shards)
- **Archive node:** 10 TB/year (full history)

### Bandwidth

- **Validator node:** 1 Mbps average (causal graph + proofs)
- **Light node:** 100 Kbps average (only relevant updates)

---

## Security Model

### Threat Model

**Adversaries:**
- Up to 1/3 of validator nodes are Byzantine (faulty or malicious)
- Network may partition temporarily
- Cryptographic primitives are secure (no quantum computers yet)

### Security Guarantees

**Consistency:** If 2/3 of validators are honest, the system maintains consistency

**Liveness:** If the network is connected and 2/3 of validators are honest, the system makes progress

**Finality:** Once a transaction is finalized, it cannot be reversed without burning 1/3 of validator collateral

### Cryptographic Primitives

- **Signatures:** EdDSA (quantum-resistant variant planned)
- **Hashing:** SHA-3
- **Zero-Knowledge Proofs:** zk-SNARKs (Groth16, Plonk)
- **Encryption:** ChaCha20-Poly1305

### Economic Security

Validators must stake collateral (Omnia tokens or compute power) to participate. If they misbehave, their stake is slashed.

**Slashing conditions:**
- Signing conflicting blocks: 100% slash
- Offline for >24 hours: 1% slash per day
- Malicious behavior: Up to 100% slash

---

## Future Enhancements

### Quantum Resistance

As quantum computers become practical, Omnia will transition to quantum-resistant cryptography:

- **Lattice-based signatures:** Dilithium
- **Hash-based signatures:** SPHINCS+
- **Multivariate polynomial cryptography:** Rainbow

### Homomorphic Encryption

Full homomorphic encryption will enable:

- Computing on encrypted data without decryption
- Privacy-preserving smart contracts
- Encrypted machine learning

### Proof-of-Useful-Work

Instead of burning energy on puzzles, validators will prove they performed useful work:

- Scientific computation (protein folding, climate modeling)
- AI training
- Rendering

### Interplanetary Operation

As humanity expands beyond Earth:

- Relativistic consensus for Mars, Moon, and beyond
- Local autonomy with eventual consistency
- Seamless cross-planetary transactions

---

## References

- Lamport, L. (1978). "Time, Clocks, and the Ordering of Events in a Distributed System"
- Shapiro, M., & Preguiça, N. (2011). "Conflict-free Replicated Data Types"
- Ben-Sasson, E., et al. (2014). "Zerocash: Decentralized Anonymous Payments from Bitcoin"
- Buterin, V. (2014). "Ethereum: A Next-Generation Smart Contract and Decentralized Application Platform"

---

**Status:** Theoretical Architecture  
**Version:** 1.0  
**Last Updated:** May 2026

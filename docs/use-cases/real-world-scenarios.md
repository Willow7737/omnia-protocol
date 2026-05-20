# Real-World Scenarios
> 🎯 Audience: Laymen, All
> 🔗 Context: 10 real-world use cases demonstrating how Omnia solves problems across financial inclusion, supply chain, healthcare, IP, AI, and governance
> 📅 Last Updated: 2026-05-20

## 1. Financial Inclusion for the Unbanked

### The Problem

1.7 billion people worldwide lack access to basic financial services. They cannot:
- Save money safely
- Borrow for education or business
- Send money to family
- Access credit history

### Omnia's Solution

**Scenario:** Amara, a farmer in rural Kenya

1. **Identity:** Amara creates a DID on her phone (no government ID needed). The `did:omnia:` method (implemented in `shards/src/identity/did.rs`) creates a self-sovereign identifier from her Ed25519 public key.
2. **Reputation:** She completes small tasks on Omnia, building reputation. Proof-of-Useful-Work submissions (`UsefulWorkProof` in `economics/src/useful_work.rs`) earn her additional UBC via `UbcToken::reward()`.
3. **Borrowing:** With reputation as collateral, she borrows 1,000 Omnia for seeds. The `FinancialShard` (`shards/src/lib.rs`) processes the `FinancialOp::Transfer` operation with strict causal ordering to prevent double-spends.
4. **Selling:** She sells her harvest directly to buyers globally, receiving payment instantly. The `ShardRouter` enforces fees via `FeeSchedule` and deducts UBC from the caller's quota.
5. **Saving:** Her savings earn interest through RPGF rewards.

**Impact:**
- Amara has a financial identity without a bank
- She can access credit based on reputation, not collateral
- She eliminates middlemen and keeps more profit
- Her children can attend school

### Implementation

```
Phase 1: Mobile wallet with offline support
- Works on 2G networks
- Transactions sync when online
- Biometric authentication via BiometricAnchor (shards/src/identity/biometric.rs)

Phase 2: Peer-to-peer lending
- Amara can borrow from other users
- Reputation determines interest rate
- Smart contracts handle repayment

Phase 3: Global marketplace
- Amara sells directly to global buyers
- Instant settlement
- No currency exchange fees
```

---

## 2. Supply Chain Transparency

### The Problem

Global supply chains are opaque. Consumers cannot verify:
- Product authenticity
- Ethical sourcing
- Environmental impact
- Fair labor practices

### Omnia's Solution

**Scenario:** A coffee buyer in New York

**The Journey:**
1. **Farming (Ethiopia):** Farmer registers coffee batch with RF fingerprint and quantum seal. The `PhysicalOp::AnchorItem` operation (in `shards/src/physical/ops.rs`) creates an immutable provenance entry.
2. **Cooperative:** Beans are washed, graded, and RF fingerprint is updated. `PhysicalOp::TransferOwnership` records the transfer in the append-only provenance log.
3. **Export:** Container is sealed with quantum seal and GPS tracking. A `CrossShardMessage` (in `shards/src/cross_shard.rs`) may trigger a financial payment to the cooperative.
4. **Roasting (Portland):** Roaster verifies quantum seal, roasts to specification. `PhysicalOp::VerifyChain` validates the complete provenance chain.
5. **Retail:** Retailer scans QR code, verifies full chain
6. **Consumer:** Buys coffee, scans with phone, sees complete provenance

**What the Consumer Sees:**
```
Your Coffee's Journey
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Origin: Ethiopia, Yirgacheffe Region
Farmer: Abebe Kebede (verified identity, 4.8 reputation)
Harvest: March 2026 (satellite-verified weather data)
Cooperative: Cooperative #47 (RF: 0x9a2f..., quantum seal intact)
Shipping: Container MSCU2847561 (quantum seal verified, temp: 15-20C)
Roasting: Portland Roasters (carbon footprint: 0.3kg CO2)
Fair Trade: Price to farmer: $4.20/kg (verified)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
All claims cryptographically proven.
Trust no one. Verify everything.
```

**Impact:**
- Farmers get fair prices (no middlemen taking cuts)
- Consumers know exactly where their coffee comes from
- Environmental impact is transparent
- Counterfeiting becomes impossible

---

## 3. Healthcare and Vaccination Records

### The Problem

Healthcare records are fragmented across providers. Patients cannot:
- Access their full medical history
- Prove vaccination status without revealing full records
- Share data for research without losing privacy
- Verify prescription authenticity

### Omnia's Solution

**Scenario:** Sarah, traveling to a conference

1. **Vaccination Proof:** Sarah proves she's vaccinated without revealing her full medical record
   - Uses zero-knowledge proof via `BiologicalOp::QueryWithZkProof` (defined in `shards/src/biological/ops.rs`)
   - The Biological shard's consent registry (`ConsentRecord` in `shards/src/biological/state.rs`) verifies that Sarah has granted the conference access
   - No personal data is shared beyond the verified claim

2. **Medical History:** Sarah's doctor can access her full history across providers
   - `BiologicalOp::GrantAccess` authorizes the doctor
   - `BiologicalOp::RevokeAccess` allows Sarah to withdraw consent at any time
   - Sarah controls access through the consent registry

3. **Research Participation:** Sarah contributes her anonymized data to cancer research
   - Her data is encrypted
   - Researchers can analyze without seeing her identity
   - She earns UBC rewards for contribution via `UsefulWorkProof`

**Implementation:**
```
Phase 1: Vaccination records
- Verifiable credentials for vaccines
- Zero-knowledge proofs for travel
- Integration with health authorities

Phase 2: Medical history
- Patient-controlled health records
- Provider integration
- Emergency access protocols

Phase 3: Research participation
- Privacy-preserving data sharing
- Automated consent management
- Reward distribution
```

---

## 4. Intellectual Property and Digital Rights

### The Problem

Artists, musicians, and creators lose control of their work:
- Streaming platforms take 70% of revenue
- Piracy is rampant
- Attribution is lost
- Derivative works are untracked

### Omnia's Solution

**Scenario:** Maya, a musician

1. **Registration:** Maya registers her song on Omnia with cryptographic proof of creation. The `PhysicalOp::AnchorItem` operation creates a permanent provenance entry for the digital work.
2. **Distribution:** She sells directly to listeners (no Spotify middleman). The `FinancialOp::Transfer` operation handles payments with strict causal ordering.
3. **Royalties:** Every derivative work automatically pays her
4. **Attribution:** Her name is cryptographically linked to her work forever
5. **Licensing:** Other artists can license her work with automatic payment

**Impact:**
- Maya keeps 99% of revenue (only 1% fee for Omnia)
- Her work is protected from piracy (cryptographically verified)
- She earns royalties on derivatives
- Her attribution is permanent

---

## 5. Decentralized AI Training

### The Problem

AI models are trained on centralized data, controlled by a few companies:
- Users' data is exploited without consent
- Only large companies can afford to train models
- Model ownership is centralized
- Bias is hidden

### Omnia's Solution

**Scenario:** A medical AI model trained by a distributed network

1. **Data Contribution:** Hospitals contribute anonymized patient data
   - Data is encrypted
   - Hospitals retain control via `BiologicalOp::GrantAccess` with scoped consent
   - Hospitals earn rewards via `UsefulWorkType::AiTraining` (in `economics/src/useful_work.rs`)

2. **Model Training:** Thousands of devices train the model in parallel
   - Each device trains on a subset of data
   - No single entity sees all data
   - Proof-of-useful-work is verified via `UsefulWorkProof::validate()` and `verify_stub()`
   - The `ComputationalOp::SubmitTask`, `SubmitProof`, and `VerifyProof` operations manage the task lifecycle

3. **Model Ownership:** The model is owned by all contributors
   - Voting power is proportional to contribution (quadratic voting via `GovernanceState`)
   - Model improvements are shared
   - Revenue is distributed

4. **Deployment:** The model is deployed on Omnia
   - Hospitals can use it for free (UBC quota: 1,000 UBC/month default)
   - Commercial users pay small fee (enforced by `FeeSchedule::computational_op_fee = 5`)
   - Fees go to RPGF pool

**Impact:**
- Hospitals keep their data private
- Medical AI is more accurate (trained on more diverse data)
- Smaller organizations can participate in AI development
- Model bias is reduced through diverse training data

---

## 6. Interplanetary Trade

### The Problem

As humanity expands to Mars and beyond, traditional finance breaks:
- Earth-Mars communication takes 3-22 minutes
- Traditional blockchains require global synchronization
- Currency exchange is complex
- Trust is difficult to establish

### Omnia's Solution

**Scenario:** A Mars colony trades with Earth

1. **Local Autonomy:** Mars has its own validators and local finality
   - Transactions finalize in minutes
   - No waiting for Earth

2. **Periodic Sync:** Mars and Earth synchronize every 22 minutes
   - Causal graph merges
   - Conflicts resolved by causal ordering via `VectorClock::happened_before()`
   - No hard fork needed

3. **Atomic Swaps:** Martian resources trade for Earth resources
   - Peer-to-peer settlement
   - Time-locked contracts via `TimeLockVoting` (in `economics/src/time_lock.rs`)
   - Automatic execution

**Example Transaction:**
```
Mars Colony sells 100 tons of water ice to Earth
Earth sends 50,000 Omnia

Timeline:
T+0: Mars sends transaction (local finality in 2 minutes)
T+3: Earth receives transaction (via satellite)
T+5: Earth sends payment (local finality in 2 minutes)
T+8: Mars receives payment (via satellite)
T+22: Full synchronization (causal graph merges)

Result: Trade complete, both parties satisfied, no intermediary needed
```

---

## 7. Refugee Identity and Resettlement

### The Problem

Refugees lack official documentation:
- Cannot prove identity
- Cannot access banking
- Cannot prove education/skills
- Cannot get jobs

### Omnia's Solution

**Scenario:** Ahmed, a Syrian refugee

1. **Identity:** Ahmed creates a DID
   - No government ID needed — the `did:omnia:` method requires only a 32-byte Ed25519 public key
   - `BiometricAnchor::enroll()` (in `shards/src/identity/biometric.rs`) proves he's a living human via a salted BLAKE3 commitment — the raw template is never stored
   - Social recovery via `ShamirRecovery::split()` (in `shards/src/identity/recovery.rs`) — configurable K-of-N threshold

2. **Credentials:** Ahmed's skills are verified
   - Former employer issues credential (encrypted)
   - Ahmed proves he's a skilled engineer
   - Doesn't reveal employer details

3. **Employment:** Ahmed applies for jobs in Turkey
   - Employer verifies his skills
   - Ahmed's reputation is visible
   - He gets hired based on merit, not documentation

4. **Banking:** Ahmed opens an account on Omnia
   - Receives UBC quota (1,000 UBC/month default via `DEFAULT_UBC_QUOTA`)
   - Can send money to family
   - Can borrow for housing

**Impact:**
- Ahmed has a digital identity that no government can revoke
- He can prove his skills without official documents
- He can access financial services
- He can rebuild his life

---

## 8. Decentralized Governance and DAOs

### The Problem

Traditional organizations are hierarchical:
- Decisions are made by a few people
- Transparency is limited
- Corruption is common
- Participation is difficult

### Omnia's Solution

**Scenario:** A global climate action DAO

1. **Membership:** Anyone can join by staking Omnia
   - Quadratic voting (`GovernanceState::set_weight()`) prevents whale dominance — voting power = isqrt(stake)
   - Reputation determines voting power
   - Transparent voting via `Proposal` and `VoteChoice` types (in `economics/src/governance.rs`)

2. **Proposals:** Members propose climate initiatives
   - Reforestation in Amazon
   - Solar farm in Sahara
   - Wind turbines in North Sea

3. **Funding:** Community votes on funding allocation
   - `GovernanceState::vote()` casts a weighted vote using effective weight (including decay)
   - Funds are distributed automatically
   - Progress is tracked on-chain

4. **Execution:** Projects are executed by contractors
   - Proof-of-work verifies completion via `UsefulWorkProof::verify_stub()`
   - Payments are released automatically
   - Results are auditable

5. **Flash loan prevention:** Time-locked voting (`TimeLockVoting` in `economics/src/time_lock.rs`) ensures that freshly-locked stake has zero voting power until the lock matures (default: 100 blocks minimum)

6. **AI agent participation:** AI agents with `AgentCapability::GovernanceVote { max_weight }` can participate with bounded influence

**Impact:**
- Climate action is coordinated globally
- Funding is transparent and efficient
- Corruption is eliminated
- Anyone can participate

---

## 9. Energy Grid Optimization

### The Problem

Centralized energy grids are inefficient:
- Renewable energy is wasted
- Demand peaks cause blackouts
- Storage is expensive
- Consumers have no incentive to conserve

### Omnia's Solution

**Scenario:** A neighborhood with solar panels and batteries

1. **Peer-to-Peer Trading:** Neighbors trade energy directly
   - Alice has excess solar → sells to Bob
   - Bob has excess battery → sells to Carol
   - All transactions are automatic via `FinancialOp::Transfer`

2. **Smart Contracts:** Contracts optimize energy flow
   - Charge batteries when solar is abundant
   - Discharge when demand peaks
   - Minimize grid load

3. **Carbon Credits:** Renewable energy is tokenized
   - Each kWh of solar = 1 carbon credit
   - Credits can be traded
   - Polluters must buy credits

4. **Incentives:** Users are rewarded for conservation
   - Reducing peak demand = rewards
   - Sharing renewable energy = rewards
   - Rewards are distributed via `UsefulWorkType::DistributedStorage`

**Impact:**
- Renewable energy is maximized
- Grid is more stable
- Consumers save money
- Carbon emissions decrease

---

## 10. Scientific Research and Open Science

### The Problem

Scientific research is siloed:
- Data is locked behind paywalls
- Researchers cannot collaborate easily
- Reproducibility is difficult
- Funding is limited

### Omnia's Solution

**Scenario:** A global research network studying climate change

1. **Data Sharing:** Researchers share data on Omnia
   - Data is encrypted
   - Researchers retain control via biological consent management
   - Collaboration is easy

2. **Proof-of-Useful-Work:** Compute power is used for research
   - `UsefulWorkType::ScientificSimulation { simulation_id, params_hash }` allows researchers to submit climate models
   - Validators run models on their hardware
   - Validators are rewarded via `UbcToken::reward()` at a 1:1 ratio with compute units consumed

3. **Reproducibility:** All research is verifiable
   - Code is on-chain
   - Data is on-chain
   - Results are reproducible

4. **Funding:** RPGF rewards impactful research
   - Researchers apply for funding
   - Community votes on impact using quadratic voting
   - Funding is distributed automatically

**Impact:**
- Climate research is accelerated
- Findings are reproducible
- Collaboration is seamless
- Funding is merit-based

---

## Implementation Timeline

| Use Case | Phase | Timeline |
|----------|-------|----------|
| Financial Inclusion | Phase 0 | Months 0-18 |
| Supply Chain | Phase 1 | Years 1-2 |
| Healthcare | Phase 1 | Years 1-2 |
| IP and Digital Rights | Phase 1 | Years 1-2 |
| Decentralized AI | Phase 2 | Years 3-5 |
| Interplanetary Trade | Phase 3 | Years 5-10 |
| Refugee Identity | Phase 1 | Years 1-2 |
| Decentralized Governance | Phase 0 | Months 0-18 |
| Energy Grid | Phase 2 | Years 3-5 |
| Scientific Research | Phase 1 | Years 1-2 |

---
🔙 **Back**: [use-cases/](./) | 🔄 **Related**: [phase-alignment.md](./phase-alignment.md)  
🚀 **Next**: [phase-alignment.md](./phase-alignment.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

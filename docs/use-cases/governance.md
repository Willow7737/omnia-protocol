# Governance System

> 🎯 Audience: All
> 🔗 Context: Omnia's governance system — quadratic voting, reputation decay, time-locked voting, AI agent participation, and future plans
> 📅 Last Updated: 2026-05-20

> **This document describes the governance system as implemented. Sections are labeled with their implementation status.**

## Principles

### 1. Decentralization

No single entity controls Omnia. Decisions are made by the community through transparent, mathematical processes.

### 2. Meritocracy

Voting power is earned through contribution, not bought. Quadratic voting prevents whale dominance; time-locked staking prevents flash loan attacks.

### 3. Transparency

All governance decisions are recorded on-chain and auditable by anyone.

### 4. Inclusivity

Everyone — humans, AI agents, collectives — has a voice in governance. AI agents can hold `GovernanceVote` capabilities with a configurable `max_weight` limit.

### 5. Adaptability

The protocol evolves to meet changing needs without requiring hard forks.

---

## Governance Structure

### Three Pillars

#### 1. Technical Governance (Protocol Changes) — Planned

**Who decides:** Core developers and researchers

**Process:**

1. Proposal submitted (RFC format)
2. Community discussion (2 weeks)
3. Technical review (1 week)
4. Implementation (if approved)
5. Staged rollout (shadow fork → testnet → mainnet)

**Voting:** Weighted by code contributions and reputation

#### 2. Economic Governance (Monetary Policy) — Partially Implemented

**Who decides:** Token holders and UBC recipients

**Voting:** Quadratic voting (voting power = isqrt(stake)) — **Implemented** in `economics/src/governance.rs`

Full economic governance process (proposal submission, impact analysis, voting periods) is planned but not yet implemented as an end-to-end flow. The `GovernanceState` type supports creating proposals, casting votes, and tracking participation:

```rust
// economics/src/governance.rs
pub struct GovernanceState {
    pub voting_weights: HashMap<String, u64>,
    pub last_active: HashMap<String, u64>,
    pub decay_rate: DecayRate,
    pub proposals: HashMap<String, Proposal>,
}
```

#### 3. Social Governance (Community Standards) — Planned

**Who decides:** Community members

**Process:**

1. Issue raised (GitHub, [Discord](https://discord.gg/qYkpAeSYR))
2. Community discussion
3. Consensus-building
4. Implementation (if consensus reached)

**Voting:** Simple majority (>50%)

---

## Voting Mechanisms

### Quadratic Voting — Implemented

Voting power = isqrt(stake), where `isqrt` is the integer square root via Newton's method defined in `economics/src/fixed_point.rs`.

**Example:**

- Alice stakes 100 tokens → voting power = 10
- Bob stakes 10,000 tokens → voting power = 100
- Carol stakes 1,000,000 tokens → voting power = 1,000

**Effect:** One large stakeholder (Carol) has 10× power, not 10,000×. This prevents whale dominance while still rewarding commitment.

This is implemented in `economics/src/governance.rs` with multiplicative reputation decay using fixed-point PPM arithmetic (no floating-point). The `VoteChoice` enum supports three options:

```rust
// economics/src/governance.rs
pub enum VoteChoice {
    For,
    Against,
    Abstain,
}
```

The `GovernanceState::vote()` method casts a vote using the DID's effective weight (base weight reduced by inactivity decay):

```rust
pub fn vote(
    &mut self,
    did: &str,
    proposal_id: &str,
    choice: VoteChoice,
    current_epoch: u64,
) -> Result<(), EconomicsError>
```

### Time-Locked Voting — Implemented

Time-locked voting is **implemented** in `economics/src/time_lock.rs`. Stake must be locked for a minimum duration before it grants voting power, preventing flash loan attacks where an attacker borrows stake, votes, and repays in the same block.

**Configuration** (`TimeLockConfig`):

| Parameter            | Default        | Description                           |
| -------------------- | -------------- | ------------------------------------- |
| `min_lock_duration`  | 100 blocks     | Minimum lock (~8 min at 5s finality)  |
| `max_lock_duration`  | 100,000 blocks | Maximum lock (~6 days at 5s finality) |
| `strict_enforcement` | true           | No early withdrawals                  |

**How it works:**

1. A user locks stake via `TimeLockVoting::lock(owner, amount, current_height, duration)`.
2. The stake has zero voting power until `current_height >= lock_start + duration`.
3. After the lock matures, the full stake amount contributes to voting power.
4. The user can release matured stakes via `release_expired()`, which removes them from voting power.

**Flash loan resistance**: Freshly-locked stake has zero voting power. Borrowed funds cannot be used for voting because the lock must mature over multiple blocks.

```rust
// economics/src/time_lock.rs
pub fn voting_power(&self, current_height: u64) -> u64 {
    if self.released { 0 }
    else if self.is_mature(current_height) { self.amount }
    else { 0 }
}
```

### Conviction Voting — Planned

Voters can lock tokens for longer periods to increase voting power beyond the time-lock mechanism. The current time-locked voting implementation provides a binary power model (0 if immature, full amount if mature). Conviction voting would add graduated multipliers based on lock duration.

This is planned for Phase 1 but not yet implemented.

### Delegation — Planned

Voters can delegate their voting power to trusted representatives.

**Process:**

1. Voter selects delegate
2. Delegate votes on behalf of voter
3. Voter can revoke delegation anytime
4. Delegate's voting power is public

This is planned for Phase 1 but not yet implemented.

---

## Governance Cycles — Planned

### Monthly Governance

**First Monday of each month:**

- Governance call (1 hour)
- Community presents proposals
- Voting begins

**Second Monday:**

- Voting ends
- Results announced
- Implementation planning

### Quarterly Reviews

**Every 3 months:**

- Review protocol performance
- Assess community health
- Plan next quarter's initiatives

---

## Proposal Types — Planned

### Tier 1: Minor Updates (Fast Track)

| Property           | Value                                                 |
| ------------------ | ----------------------------------------------------- |
| **Examples**       | Bug fixes, documentation, small parameter adjustments |
| **Timeline**       | 1 week                                                |
| **Voting**         | Simple majority                                       |
| **Implementation** | Immediate                                             |

### Tier 2: Standard Proposals (Normal Track)

| Property           | Value                                                        |
| ------------------ | ------------------------------------------------------------ |
| **Examples**       | New features, protocol improvements, economic policy changes |
| **Timeline**       | 4 weeks                                                      |
| **Voting**         | Quadratic voting (>66% approval)                             |
| **Implementation** | Staged rollout                                               |

### Tier 3: Major Changes (Extended Track)

| Property           | Value                                          |
| ------------------ | ---------------------------------------------- |
| **Examples**       | Consensus mechanism changes, new domain shards |
| **Timeline**       | 12 weeks                                       |
| **Voting**         | Quadratic voting (>75% approval)               |
| **Implementation** | Shadow fork → testnet → mainnet (3+ months)    |

The `Proposal` struct in the codebase supports the basic fields needed for proposals:

```rust
// economics/src/governance.rs
pub struct Proposal {
    pub id: String,
    pub description: String,
    pub created_at_epoch: u64,
    pub expires_at_epoch: u64,
    pub votes_for: u64,
    pub votes_against: u64,
    pub votes_abstain: u64,
}
```

---

## Dispute Resolution — Planned

### Conflict Resolution Process

| Step | Duration | Description                                                                |
| ---- | -------- | -------------------------------------------------------------------------- |
| 1    | 1 week   | Negotiation — Parties discuss directly; mediator facilitates               |
| 2    | 2 weeks  | Arbitration — Neutral arbitrator reviews evidence, proposes solution       |
| 3    | 2 weeks  | Community Vote — If parties disagree with arbitration; decision is binding |

### Slashing — Aspirational

Validators can be slashed (lose stake) for:

| Offense                | Slash Amount | Reason                                    |
| ---------------------- | ------------ | ----------------------------------------- |
| Double-signing         | 100%         | Attempting to finalize conflicting blocks |
| Offline >24h           | 1% per day   | Failing to participate in consensus       |
| Malicious behavior     | 50-100%      | Attacking the network                     |
| Censoring transactions | 25%          | Refusing to include valid transactions    |

Slashing is aspirational — there is no validator network or staking system yet.

---

## Reputation System

### Reputation Decay — Implemented

Reputation decays via multiplicative decay per epoch of inactivity. This is implemented in `economics/src/governance.rs` and `economics/src/fixed_point.rs`. Active users experience slower decay than inactive users because voting updates `last_active` to the current epoch.

**Decay formula** (fixed-point PPM arithmetic, no f64):

```
effective_weight = base_weight * remaining_ppm / BASIS_PPM
where remaining_ppm = (BASIS_PPM - decay_rate.ppm())^epochs / BASIS_PPM^epochs
```

Default decay rate: `DecayRate::ten_percent()` = 100,000 PPM per epoch.

**Effect:** Power cannot concentrate. Even early adopters must stay active to maintain influence.

### Reputation Scoring — Not Started

The full reputation scoring system (transaction history, credential issuance, community votes, validator performance) is not yet implemented. Only the decay mechanism and quadratic weight calculation exist.

### AI Agent Participation — Implemented

AI agents can participate in governance through the `AgentCapability::GovernanceVote` capability type, defined in `shards/src/identity/agent.rs`:

```rust
// shards/src/identity/agent.rs
pub enum AgentCapability {
    FinancialTransfer { max_amount: u64, currency: String },
    DataProcessing { domains: Vec<String>, max_records: u64 },
    ContractExecution { contract_types: Vec<String> },
    ComputeProof { max_compute_units: u64 },
    GovernanceVote { max_weight: u64 },
}
```

The `GovernanceVote` capability includes a `max_weight` parameter that limits the agent's maximum quadratic voting weight, ensuring that AI agents cannot exceed their authorized governance influence.

### Reputation Thresholds — Planned

| Threshold  | Privileges                   |
| ---------- | ---------------------------- |
| **0-10**   | Read-only access             |
| **10-25**  | Can vote on Tier 1 proposals |
| **25-50**  | Can vote on Tier 2 proposals |
| **50-75**  | Can vote on Tier 3 proposals |
| **75-100** | Can propose Tier 3 changes   |

---

## Treasury Management — Aspirational

### Revenue Sources

| Source              | Amount                    | Use                    | Status              |
| ------------------- | ------------------------- | ---------------------- | ------------------- |
| Transaction fees    | Implemented (FeeSchedule) | RPGF pool              | Not yet distributed |
| High-frequency fees | Not implemented           | UBC subsidies          | Not started         |
| Validator rewards   | Not implemented           | Incentivize validation | Not started         |
| Slashing proceeds   | Not implemented           | RPGF pool              | Not started         |

### Spending Categories

| Category          | Allocation | Purpose                   | Status       |
| ----------------- | ---------- | ------------------------- | ------------ |
| RPGF              | 40%        | Reward public goods       | Aspirational |
| UBC subsidies     | 30%        | Free access for all       | Aspirational |
| Research          | 15%        | Academic partnerships     | Aspirational |
| Infrastructure    | 10%        | Nodes, storage, bandwidth | Aspirational |
| Emergency reserve | 5%         | Crisis response           | Aspirational |

All treasury distribution is aspirational — there are no transaction fee distribution mechanisms, no validator rewards, and no treasury mechanism implemented yet. The `FeeSchedule` collects fees via the `ShardRouter`, but the collected UBC is simply consumed (burned), not redirected to a treasury.

### RPGF Process — Aspirational

**Quarterly RPGF Rounds:**

1. **Nomination Phase** (2 weeks) — Community nominates projects
2. **Evaluation Phase** (2 weeks) — Community evaluates impact
3. **Voting Phase** (2 weeks) — Quadratic voting on allocations
4. **Distribution Phase** (1 week) — Funds automatically distributed

---

## Amending Governance — Planned

### Governance Change Process

To change the governance system itself:

1. **Proposal:** Detailed proposal submitted (Tier 3)
2. **Discussion:** 4-week community discussion
3. **Voting:** 75% approval required
4. **Implementation:** 3-month shadow fork testing
5. **Activation:** Community vote to activate

### Constitutional Constraints

Governance cannot:

- Centralize power in a single entity
- Eliminate quadratic voting
- Remove transparency requirements
- Violate privacy of users
- Discriminate based on identity type (human, AI, collective)

---

## Community Participation

### How to Get Involved

#### 1. Contribute Code

**Process:**

1. Fork repository
2. Create feature branch
3. Implement with tests
4. Submit pull request
5. Code review (1 approval for now — small team)
6. Merge

#### 2. Participate in Governance

**Process:**

1. Run the protocol locally
2. Test governance features (quadratic voting, reputation decay, time-locked voting)
3. Propose changes via GitHub Issues or Discussions

#### 3. Build Applications

**Process:**

1. Build tool/service using the Omnia Rust library
2. Attract users
3. Apply for RPGF funding (when implemented)

---

## Governance FAQ

### Q: What if I disagree with a governance decision?

**A:** You have several options:

1. **Propose a change:** Submit a proposal to reverse the decision
2. **Exit:** Withdraw your stake and leave the network
3. **Fork:** Create an alternative version of the protocol (requires community support)

### Q: Can governance decisions be reversed?

**A:** Yes. Any decision can be reversed through the same process that created it. This requires:

- Tier 3 proposal (if it was a Tier 3 decision)
- 75% approval
- 3-month implementation period

---

## Governance Roadmap

### Year 1: Foundation — In Progress

- Implement quadratic voting with reputation decay
- Implement time-locked voting (flash loan prevention)
- Implement AI agent governance capability
- Establish governance processes
- Build community participation
- Launch RPGF program

### Year 2: Maturation — Planned

- Expand validator participation
- Increase governance frequency
- Introduce conviction voting (graduated multipliers)
- Establish academic partnerships

### Year 3: Decentralization — Planned

- Reduce core team influence
- Empower community governance
- Implement self-modifying protocol
- Enable full AI agent governance participation

### Year 4+: Post-Human Governance — Aspirational

_This section describes a long-term vision. It is not currently being developed._

- AI agents have full voting rights
- Collective intelligence guides decisions
- Protocol evolves without human intervention
- Governance becomes mathematical

---

🔙 **Back**: [use-cases/](./) | 🔄 **Related**: [../architecture/layer-5-economics.md](../architecture/layer-5-economics.md)  
🚀 **Next**: [real-world-scenarios.md](./real-world-scenarios.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

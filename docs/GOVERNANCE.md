# Omnia Protocol Governance

> **⚠️ This document describes the governance vision. Only quadratic voting with exponential decay is currently implemented. Sections are labeled as Implemented, Planned, or Aspirational.**

## Principles

### 1. Decentralization

No single entity controls Omnia. Decisions are made by the community through transparent, mathematical processes.

### 2. Meritocracy

Voting power is earned through contribution, not bought. Quadratic voting prevents whale dominance.

### 3. Transparency

All governance decisions are recorded on-chain and auditable by anyone.

### 4. Inclusivity

Everyone — humans, AI agents, collectives — has a voice in governance.

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

**Voting:** Quadratic voting (voting power = sqrt(stake)) — **Implemented**

Full economic governance process (proposal submission, impact analysis, voting periods) is planned but not yet implemented.

#### 3. Social Governance (Community Standards) — Planned

**Who decides:** Community members

**Process:**
1. Issue raised (GitHub, Discord)
2. Community discussion
3. Consensus-building
4. Implementation (if consensus reached)

**Voting:** Simple majority (>50%)

---

## Voting Mechanisms

### Quadratic Voting — Implemented ✅

Voting power = sqrt(stake)

**Example:**
- Alice stakes 100 tokens → voting power = 10
- Bob stakes 10,000 tokens → voting power = 100
- Carol stakes 1,000,000 tokens → voting power = 1,000

**Effect:** One large stakeholder (Carol) has 10x power, not 10,000x. This prevents whale dominance while still rewarding commitment.

This is implemented in `economics/src/governance.rs` with exponential reputation decay.

### Conviction Voting — Planned 📋

Voters can lock tokens for longer periods to increase voting power.

**Formula:** Voting power = stake × (lock_period / max_lock_period)

**Example:**
- Alice locks 100 tokens for 1 month → voting power = 10 × (1/12) = 0.83
- Bob locks 100 tokens for 12 months → voting power = 10 × (12/12) = 10

**Effect:** Long-term believers have more influence than short-term speculators.

This is planned for Phase 1 but not yet implemented.

### Delegation — Planned 📋

Voters can delegate their voting power to trusted representatives.

**Process:**
1. Voter selects delegate
2. Delegate votes on behalf of voter
3. Voter can revoke delegation anytime
4. Delegate's voting power is public

**Use case:** Voters who lack time or expertise can delegate to domain experts.

This is planned for Phase 1 but not yet implemented.

---

## Governance Cycles — Planned 📋

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

## Proposal Types — Planned 📋

### Tier 1: Minor Updates (Fast Track)

**Examples:**
- Bug fixes
- Documentation updates
- Small parameter adjustments

**Timeline:** 1 week
**Voting:** Simple majority
**Implementation:** Immediate

### Tier 2: Standard Proposals (Normal Track)

**Examples:**
- New features
- Protocol improvements
- Economic policy changes

**Timeline:** 4 weeks
**Voting:** Quadratic voting (>66% approval)
**Implementation:** Staged rollout

### Tier 3: Major Changes (Extended Track)

**Examples:**
- Consensus mechanism changes
- New domain shards
- Fundamental protocol redesigns

**Timeline:** 12 weeks
**Voting:** Quadratic voting (>75% approval)
**Implementation:** Shadow fork → testnet → mainnet (3+ months)

---

## Dispute Resolution — Planned 📋

### Conflict Resolution Process

**Step 1: Negotiation** (1 week)
- Parties discuss directly
- Mediator facilitates if needed

**Step 2: Arbitration** (2 weeks)
- Neutral arbitrator reviews evidence
- Arbitrator proposes solution

**Step 3: Community Vote** (2 weeks)
- If parties disagree with arbitration
- Community votes on resolution
- Decision is binding

### Slashing — Aspirational 🔮

Validators can be slashed (lose stake) for:

| Offense | Slash Amount | Reason |
|---------|--------------|--------|
| Double-signing | 100% | Attempting to finalize conflicting blocks |
| Offline >24h | 1% per day | Failing to participate in consensus |
| Malicious behavior | 50-100% | Attacking the network |
| Censoring transactions | 25% | Refusing to include valid transactions |

Slashing is aspirational — there is no validator network or staking system yet.

---

## Reputation System — Partially Implemented 🏗️

### Reputation Decay — Implemented ✅

Reputation decays exponentially over time. This is implemented in the governance module. Active users experience slower decay than inactive users.

**Effect:** Power cannot concentrate. Even early adopters must stay active to maintain influence.

### Reputation Scoring — Not Started 🌑

The full reputation scoring system (transaction history, credential issuance, community votes, validator performance) is not yet implemented. Only the decay mechanism exists.

### Reputation Thresholds — Planned 📋

| Threshold | Privileges |
|-----------|-----------|
| **0-10** | Read-only access |
| **10-25** | Can vote on Tier 1 proposals |
| **25-50** | Can vote on Tier 2 proposals |
| **50-75** | Can vote on Tier 3 proposals |
| **75-100** | Can propose Tier 3 changes |

---

## Treasury Management — Aspirational 🔮

### Revenue Sources

| Source | Amount | Use |
|--------|--------|-----|
| **Transaction fees** | Not yet implemented | RPGF pool |
| **High-frequency fees** | Not yet implemented | UBC subsidies |
| **Validator rewards** | Not yet implemented | Incentivize validation |
| **Slashing proceeds** | Not yet implemented | RPGF pool |

### Spending Categories

| Category | Allocation | Purpose |
|----------|-----------|---------|
| **RPGF** | 40% | Reward public goods |
| **UBC subsidies** | 30% | Free access for all |
| **Research** | 15% | Academic partnerships |
| **Infrastructure** | 10% | Nodes, storage, bandwidth |
| **Emergency reserve** | 5% | Crisis response |

All treasury management is aspirational — there are no transaction fees, no validator rewards, and no treasury mechanism implemented yet.

### RPGF Process — Aspirational 🔮

**Quarterly RPGF Rounds:**

1. **Nomination Phase** (2 weeks) — Community nominates projects
2. **Evaluation Phase** (2 weeks) — Community evaluates impact
3. **Voting Phase** (2 weeks) — Quadratic voting on allocations
4. **Distribution Phase** (1 week) — Funds automatically distributed

---

## Amending Governance — Planned 📋

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
2. Test governance features (quadratic voting, reputation decay)
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
- ✅ Implement quadratic voting with exponential decay
- 📋 Establish governance processes
- 📋 Build community participation
- 📋 Launch RPGF program

### Year 2: Maturation — Planned 📋
- Expand validator participation
- Increase governance frequency
- Introduce conviction voting
- Establish academic partnerships

### Year 3: Decentralization — Planned 📋
- Reduce core team influence
- Empower community governance
- Implement self-modifying protocol
- Enable AI agent participation

### Year 4+: Post-Human Governance — Aspirational 🔮

*This section describes a long-term vision. It is not currently being developed.*

- AI agents have full voting rights
- Collective intelligence guides decisions
- Protocol evolves without human intervention
- Governance becomes mathematical

---

**Status:** Governance Framework — Partially Implemented
**Implemented:** Quadratic voting with exponential decay
**Planned:** Conviction voting, delegation, treasury, validator governance
**Aspirational:** RPGF, slashing, post-human governance
**Version:** 2.0
**Last Updated:** May 2026

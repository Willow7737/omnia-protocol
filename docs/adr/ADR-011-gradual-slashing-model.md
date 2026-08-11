# ADR-011: Gradual Slashing Model

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-08-11

## Status

Accepted

## Date

2025-05-18

## Version

1.0.0

## Decision

Implement a 3-tier graded slashing system: Warning → Jail → Ejection, with partial stake burns and configurable jail periods.

## Context

The original slashing system was binary — once a validator accumulated enough slash points (default 500), their entire stake was forfeited. This created several problems:

1. **Perverse incentives**: Once a validator is close to the slash threshold, they have nothing to lose from additional misbehavior, potentially causing maximum damage.
2. **No proportional response**: A single liveness violation (100 points) and an equivocation (500 points) have the same binary outcome once the threshold is crossed.
3. **No recovery path**: There was no mechanism for a validator to be temporarily suspended and then restored, forcing a permanent ejection for offenses that may be unintentional.
4. **Economic harshness**: Full slashing for first offenses discourages validator participation and centralizes the validator set among those who can afford to never make mistakes.

## Alternatives Considered

### Cosmos-Style Slashing

Cosmos uses a slashing rate that depends on the fraction of total stake that misbehaved. This is mathematically elegant but complex to implement and requires global stake information that may not be available in a sharded architecture.

### Polkadot-Style Slashing

Polkadot uses a gradual slashing model with "offence counts" that escalate penalties. This is similar to our chosen approach but includes a "chilling" period and more complex reward/penalty interaction.

### Binary Slashing (Status Quo)

Keep the existing binary system. Simple but creates the perverse incentives described above.

## Consequences

### Positive

- Proportional penalties: first offenses receive warnings, not ejection
- Jail mechanism allows temporary suspension without permanent removal
- Auto-release after jail term reduces governance overhead for minor offenses
- Slashing events provide audit trail for external monitoring
- Economic fairness: validators aren't fully slashed for first-time mistakes

### Negative

- More complex state machine: jail_registry, typed_offense_history, SlashingPenalty enum
- Offense history must be persisted (increased storage requirements)
- Governance-triggered early release adds coordination overhead
- Graduated system may be seen as "soft on crime" by some stakeholders

### Trade-offs

- Chose economic fairness over simplicity
- 3 tiers provide clear escalation without excessive granularity
- Auto-release for first offenses balances automation with safety

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

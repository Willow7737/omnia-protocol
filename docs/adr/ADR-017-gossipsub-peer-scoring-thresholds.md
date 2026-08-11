# ADR-017: GossipSub Peer Scoring Thresholds

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-08-11

## Status

Accepted

## Date

2026-05-19

## Version

1.0.0

## Decision

Implement GossipSub peer scoring with penalty weights for misbehavior, a graylist threshold of -100, topic-specific scoring for `omnia-events` and `omnia-consensus` topics, and a decay interval of 1 minute. Application-level `PeerScoreTracker` provides `record_validation()` and `is_graylisted()` for fine-grained scoring.

## Context

GossipSub is the pubsub protocol used for broadcasting events and consensus messages in Omnia. Without peer scoring, a malicious or misbehaving peer can:

1. **Flood the network** with invalid messages, wasting bandwidth and processing time.
2. **Withhold messages** by not forwarding them, breaking liveness.
3. **Send duplicate messages**, wasting resources.
4. **Participate in eclipse attacks** by controlling a victim's mesh connections.

The default GossipSub parameters in libp2p are generic and not tuned for Omnia's threat model. A customized scoring system is needed to:

- Penalize peers that send invalid messages (signature failures, malformed payloads)
- Penalize peers that fail to deliver messages in the mesh
- Reward peers that deliver first-seen valid messages
- Graylist persistently bad peers to protect the network
- Decay scores over time so that temporarily misbehaving peers can recover

## Alternatives Considered

### No Peer Scoring

Rely on the default GossipSub behavior without custom scoring. Simple but provides no protection against malicious peers beyond basic protocol compliance. Bad actors can degrade network quality without consequence.

### Binary Allow/Deny Lists

Manually maintain lists of allowed and denied peers. Provides absolute control but doesn't scale (requires manual curation) and cannot handle temporary misbehavior or gradual degradation. A peer is either fully trusted or fully blocked, with no middle ground.

## Consequences

### Positive

- Graduated response to bad peers — scores decay, allowing recovery from temporary issues
- Invalid message deliveries penalized at -150 per delivery (strong deterrent)
- Mesh delivery failure penalized at -50 (moderate deterrent for lazy peers)
- First message delivery rewarded at +1 per delivery (incentivizes timely forwarding)
- Graylisting at score -100 removes persistently bad peers from mesh connections
- Topic-specific scoring for `omnia-events` and `omnia-consensus` allows domain-specific tuning
- 1-minute decay interval ensures scores reflect recent behavior
- Application-level `PeerScoreTracker` integrates with message validation pipeline

### Negative

- More complex configuration — scoring parameters need ongoing tuning based on network conditions
- Score computation adds per-message overhead
- Graylisted peers may experience temporary exclusion even if behavior improves (until score decays)
- Threshold values (-100, penalty weights) are empirically chosen and may need adjustment

### Trade-offs

- Chose graduated scoring over binary lists for self-healing capability
- Penalty weights are asymmetric (heavier for invalid messages than for delivery failures) to prioritize network integrity
- Decay interval of 1 minute provides fast recovery without erasing recent behavior too quickly

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)

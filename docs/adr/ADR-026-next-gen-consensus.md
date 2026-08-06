# ADR-026: Next-Generation Consensus — VRF Leader Election, Unpredictable Beacon, and Beyond

> Status: 🔄 Proposed (Phase 1 landing incrementally)
> Date: 2026-07-20
> Supersedes: the leader-selection decisions in ADR-012 and ADR-015
> Context: AUDIT-2026-07 C1 (#339)

## Context

The external architecture audit (AUDIT-2026-07) flagged **C1 — leader
selection is not a VRF and leaders are publicly predictable** as a mainnet
blocker. The implementation computed

```
leader(round) = BLAKE3(round_seed || round_number) mod total_stake
```

where `round_seed` is stored in plain consensus state and evolved by a
public hash chain. Consequences:

- **Every future round's leader is computable by anyone**, indefinitely.
- Candidate keypairs were threaded into the selection function but never
  used — there was no secret-key binding, so the "VRF" label (ADR-012 V2,
  ADR-015) was inaccurate. The `vrf.rs` "V2 ECVRF" scaffold likewise did
  no elliptic-curve operations (`gamma = BLAKE3(sk || H)`), so it carried
  none of a VRF's uniqueness guarantees.

Predictable leaders enable targeted DoS, MEV extraction, and coordinated
equivocation against the known upcoming proposer.

Rather than patch the seed, we take the opportunity to move Omnia's
consensus toward a modern design. This ADR records the target architecture
and the phased path to it, and documents what has actually shipped at each
step (the project's standard: docs match reality, no aspirational claims).

## Decision

Adopt a next-generation consensus architecture with six pillars:

1. **Unpredictable distributed randomness beacon** — per-round randomness
   that no single party can predict or grind.
2. **Secret VRF leader election with cryptographic proofs** — each
   validator learns *only its own* eligibility, proven to others with a
   verifiable proof (Algorand-style cryptographic sortition).
3. **Multiple backup leaders for zero-timeout failover** — an ordered
   leader schedule so a silent/slashed primary is replaced immediately,
   not after a round timeout.
4. **DAG-based transaction dissemination** — decouple data availability
   from ordering (Narwhal-style), so throughput scales with bandwidth and
   proposals reference already-disseminated data.
5. **Adaptive committees** — sample a stake-weighted committee per round
   for sublinear message complexity at large validator counts.
6. **Pipelined deterministic finality** — overlap proposal, voting, and
   commit across rounds for high throughput with bounded, deterministic
   finality latency.

The real cryptographic foundation is an **Edwards25519 EC-VRF**
(`omnia-crypto::ecvrf`): `Gamma = x·H` with a Schnorr/DLEQ proof `(c, s)`
following the RFC 9381 ECVRF construction, using each validator's existing
Ed25519 identity key as its VRF key (`Y = x·B`). This is a genuine VRF —
unique, unforgeable, verifiable — not a hash construction.

### Honesty note on RFC 9381

The EC-VRF follows the RFC 9381 ECVRF-EDWARDS25519 structure (try-and-
increment hash-to-curve, cofactor-cleared output, five-point Schnorr
challenge). Its security properties (uniqueness, pseudorandomness,
collision resistance) hold under standard random-oracle assumptions
independent of the exact hash-to-curve encoding. It is **not yet pinned to
the RFC's byte-exact interoperability test vectors** — cross-implementation
KAT validation is tracked follow-up hardening. We claim a real EC-VRF with
the security properties consensus needs; we do **not** claim certified
RFC 9381 byte-interop. This is deliberately more modest, and more accurate,
than the previous "ECVRF per RFC 9381" documentation, which this ADR
corrects.

## Phased roadmap

### Phase 1 — VRF foundation + unpredictable beacon + backups (this ADR's initial landing)

Shipped:

- `omnia-crypto::ecvrf` — real Edwards25519 EC-VRF (prove / verify /
  output), with the identity Ed25519 key as the VRF key. Full test suite:
  roundtrip, determinism, wrong-key/input rejection, tamper detection,
  non-canonical-scalar rejection, output distribution.
- `omnia-consensus::vrf_election`:
  - `fold_commitment` — evolves the leader-election beacon by absorbing the
    **committed DAG frontier** each round. Committed event IDs depend on
    user signatures that do not exist until the events are created, so no
    observer can compute the beacon — and therefore future leaders —
    beyond the committed frontier. All honest nodes fold the identical
    committed set, so the beacon stays deterministic with **no new
    messages** (the network's single-leader agreement is preserved).
  - `leader_schedule` — stake-weighted **ordered** schedule (primary +
    ranked backups) keyed on the beacon, for zero-timeout failover.
  - `ticket_alpha` / `make_ticket` / `verify_ticket` — the verifiable
    per-validator VRF ticket path, so a validator can prove its claim to a
    slot. Tested; wired for Phase 2's secret sortition.
- Engine: `compute_leader` and new `compute_leader_schedule` select via the
  tested election module using the beacon (`round_seed` is the beacon);
  `advance_beacon_from_committed` folds the committed frontier on the commit
  path. The substrate run loop proposes as the highest-ranked non-slashed
  validator in the schedule (backup failover).

Effect on C1: leaders are no longer a pure public function of a static
seed. They depend on the rolling beacon, which is unpredictable beyond the
committed frontier and evolves from data bound to many users' signatures.
The predictable-in-advance flaw is closed for the liveness/fairness role
leader election plays in the current DAG-BFT loop (proposals are not a
safety-critical single-proposer gate; events validate and commit
independently).

Remaining for full closure (tracked, keeps #339 open until done):

- **Secret single-leader sortition** — broadcast per-validator VRF tickets
  so nodes agree on a *secret* leader (a validator learns it leads only by
  evaluating its own VRF), rather than a deterministic schedule everyone
  can compute once the beacon is known. This needs ticket gossip (Phase 2 /
  Pillar 4).

### Phase 2 — Secret VRF sortition + ticket dissemination (Pillars 2, 3)

Broadcast VRF tickets; lowest stake-weighted ticket leads with backups by
rank; proposals carry the winner's proof. Builds on `vrf_election`'s
tested ticket API.

### Phase 3 — DAG dissemination (Pillar 4)

Narwhal-style mempool/availability layer; proposals reference certificates
of disseminated batches.

### Phase 4 — Adaptive committees + pipelined finality (Pillars 5, 6)

Per-round stake-weighted committee sampling (via the same VRF) and
pipelined proposal/vote/commit for deterministic low-latency finality at
scale.

## Consequences

- **Positive:** closes the predictable-leader flaw with a real EC-VRF and
  an unpredictable beacon; adds backup-leader failover; establishes the
  cryptographic and structural foundation for a competitive modern
  consensus; corrects inaccurate VRF documentation.
- **Negative / cost:** the full vision is multi-phase; Phases 2–4 are
  substantial and are tracked as separate issues. Phase 1 improves but does
  not by itself deliver secret sortition, so #339 remains open with a
  precise status until Phase 2 lands.
- **Migration:** `round_seed` is reinterpreted as the beacon (genesis =
  the configured random seed); no state-schema change, no new persisted
  field, no new network messages in Phase 1.

## References

- RFC 9381 — Verifiable Random Functions (VRFs)
- Gilad et al., *Algorand* (cryptographic sortition)
- Danezis et al., *Narwhal & Tusk* (DAG mempool)
- AUDIT-2026-07 C1 (#339); tracking issue #378

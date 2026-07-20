//! VRF-keyed, stake-weighted leader election with backup failover.
//!
//! AUDIT-2026-07 C1 (#339): leader selection was
//! `BLAKE3(round_seed || round) % total_stake` with `round_seed` a public
//! value evolved by a public hash chain — so every future round's leader
//! was computable by anyone, forever. That let an adversary pre-target
//! leaders for DoS/MEV.
//!
//! This module rebuilds leader election on two ideas:
//!
//! 1. **An unpredictable rolling beacon.** Instead of a public hash chain,
//!    the beacon absorbs the *committed DAG frontier* each round
//!    ([`fold_commitment`]). Committed event IDs depend on user signatures
//!    that do not exist until the events are created, so no one can
//!    compute the beacon — and therefore the leader — for a round beyond
//!    the current committed frontier. All honest nodes fold the identical
//!    committed set, so the beacon stays deterministic across the network
//!    (no new messages, no divergence).
//!
//! 2. **A stake-weighted ordered schedule, not a single point.**
//!    [`leader_schedule`] returns the primary leader *and* ranked backups
//!    for a round, so if the primary is slashed or silent a backup can
//!    step in immediately (zero-timeout failover) rather than waiting out
//!    a round timeout.
//!
//! The [`ecvrf`](omnia_crypto::ecvrf) primitive underpins the verifiable
//! per-validator ticket path ([`ticket_priority`] / [`verify_ticket`]):
//! a validator proves its own claim to a slot with a VRF proof anyone can
//! check. Broadcasting those tickets so nodes agree on a *secret* single
//! leader (Algorand-style sortition) is the next consensus pillar; the
//! verifiable-ticket math lands here so that work builds on tested code.

use omnia_crypto::ecvrf::{self, VrfProof};
use omnia_primitives::NodeId;
use std::collections::HashMap;

/// Domain tag for beacon evolution.
const BEACON_DOMAIN: &[u8] = b"OMNIA-LEADER-BEACON-V1";
/// Domain tag for the per-(round, candidate) selection draw.
const DRAW_DOMAIN: &[u8] = b"OMNIA-LEADER-DRAW-V1";
/// Domain tag for the VRF ticket input.
const TICKET_ALPHA_DOMAIN: &[u8] = b"OMNIA-LEADER-TICKET-V1";

/// Fold the committed DAG frontier into the beacon.
///
/// `committed` is the set of event IDs committed this round. The order is
/// normalized (sorted) so every node derives the identical next beacon
/// regardless of local commit ordering. When `committed` is empty the
/// beacon still advances via the round-independent domain fold, so idle
/// rounds do not freeze the schedule.
pub fn fold_commitment(beacon: &[u8; 32], committed: &[[u8; 32]]) -> [u8; 32] {
    let mut sorted: Vec<&[u8; 32]> = committed.iter().collect();
    sorted.sort_unstable();
    let mut hasher = blake3::Hasher::new();
    hasher.update(BEACON_DOMAIN);
    hasher.update(beacon);
    hasher.update(&(sorted.len() as u64).to_le_bytes());
    for id in sorted {
        hasher.update(id);
    }
    *hasher.finalize().as_bytes()
}

/// Compute a candidate's 64-bit draw for a round from the beacon.
///
/// Deterministic in `(beacon, round, node)`. Because the beacon is
/// unpredictable beyond the committed frontier, so is this draw.
fn candidate_draw(beacon: &[u8; 32], round: u64, node: &NodeId) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DRAW_DOMAIN);
    hasher.update(beacon);
    hasher.update(&round.to_le_bytes());
    hasher.update(node);
    let digest = hasher.finalize();
    u64::from_le_bytes(digest.as_bytes()[..8].try_into().expect("8 bytes"))
}

/// Stake-weighted priority key for a candidate.
///
/// We combine the uniform draw with stake so that higher-stake validators
/// are proportionally more likely to sort first, while every positive-stake
/// candidate retains a nonzero chance. The key is `draw / stake` computed in
/// 128-bit fixed point: scaling the uniform draw down by stake means larger
/// stake yields a smaller expected key (higher priority). Lower key sorts
/// earlier. Ties break by `NodeId` for determinism.
fn priority_key(draw: u64, stake: u64) -> u128 {
    debug_assert!(stake > 0, "priority_key requires positive stake");
    // (draw as u128) << 64 / stake keeps full precision without floats.
    ((draw as u128) << 64) / (stake as u128)
}

/// Produce the ordered leader schedule for a round: primary first, then
/// ranked backups. Zero-stake candidates are excluded. At most `count`
/// entries are returned (all of them if `count` exceeds the field).
///
/// Deterministic across all nodes given the same `beacon`, `round`, and
/// candidate set — this preserves the network's single-leader agreement
/// while adding failover ordering.
pub fn leader_schedule(candidates: &HashMap<NodeId, u64>, beacon: &[u8; 32], round: u64, count: usize) -> Vec<NodeId> {
    let mut ranked: Vec<(u128, NodeId)> = candidates
        .iter()
        .filter(|(_, &stake)| stake > 0)
        .map(|(node, &stake)| {
            let draw = candidate_draw(beacon, round, node);
            (priority_key(draw, stake), *node)
        })
        .collect();

    // Sort by priority key, breaking ties by NodeId for determinism.
    ranked.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    ranked.into_iter().take(count).map(|(_, node)| node).collect()
}

/// The primary leader for a round (schedule position 0), or `None` if no
/// positive-stake candidate exists.
pub fn primary_leader(candidates: &HashMap<NodeId, u64>, beacon: &[u8; 32], round: u64) -> Option<NodeId> {
    leader_schedule(candidates, beacon, round, 1).into_iter().next()
}

/// The VRF input (`alpha`) a validator signs to claim a slot in `round`.
///
/// Bound to the unpredictable beacon and the round, so a ticket for one
/// round cannot be replayed into another.
pub fn ticket_alpha(beacon: &[u8; 32], round: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(TICKET_ALPHA_DOMAIN);
    hasher.update(beacon);
    hasher.update(&round.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Derive a stake-weighted priority key from a VRF output.
///
/// This is the verifiable analogue of [`priority_key`]: the `beta` is an
/// EC-VRF output only the ticket holder could produce, so the resulting
/// priority is unforgeable and unpredictable, yet checkable by anyone via
/// [`verify_ticket`].
pub fn ticket_priority(beta: &[u8; ecvrf::OUTPUT_LEN], stake: u64) -> u128 {
    let draw = u64::from_le_bytes(beta[..8].try_into().expect("8 bytes"));
    priority_key(draw, stake.max(1))
}

/// Verify a validator's VRF ticket for a round and return its priority key.
///
/// Checks the EC-VRF proof against the claimed public key and the round's
/// [`ticket_alpha`], then derives the stake-weighted priority. A lower key
/// means a stronger claim to the leader slot.
pub fn verify_ticket(
    public_key: &ed25519_dalek::VerifyingKey,
    beacon: &[u8; 32],
    round: u64,
    stake: u64,
    proof: &VrfProof,
) -> Result<u128, ecvrf::VrfError> {
    let alpha = ticket_alpha(beacon, round);
    let beta = ecvrf::verify(public_key, &alpha, proof)?;
    Ok(ticket_priority(&beta, stake))
}

/// Produce this validator's VRF ticket (proof) for a round.
pub fn make_ticket(signing_key: &ed25519_dalek::SigningKey, beacon: &[u8; 32], round: u64) -> VrfProof {
    ecvrf::prove(signing_key, &ticket_alpha(beacon, round))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    fn candidates(stakes: &[(u8, u64)]) -> HashMap<NodeId, u64> {
        stakes.iter().map(|&(id, s)| (node(id), s)).collect()
    }

    #[test]
    fn schedule_is_deterministic() {
        let c = candidates(&[(1, 100), (2, 100), (3, 100)]);
        let beacon = [7u8; 32];
        let a = leader_schedule(&c, &beacon, 5, 3);
        let b = leader_schedule(&c, &beacon, 5, 3);
        assert_eq!(a, b);
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn schedule_excludes_zero_stake() {
        let c = candidates(&[(1, 0), (2, 50), (3, 0)]);
        let sched = leader_schedule(&c, &[1u8; 32], 1, 10);
        assert_eq!(sched, vec![node(2)]);
    }

    #[test]
    fn primary_matches_schedule_head() {
        let c = candidates(&[(1, 30), (2, 40), (3, 30)]);
        let beacon = [3u8; 32];
        let sched = leader_schedule(&c, &beacon, 9, 3);
        assert_eq!(primary_leader(&c, &beacon, 9), Some(sched[0]));
    }

    #[test]
    fn different_beacons_reshuffle_leaders() {
        // The whole point of the beacon: change it and the leader can change.
        let c = candidates(&[(1, 100), (2, 100), (3, 100), (4, 100), (5, 100)]);
        let mut changed = 0;
        let base = primary_leader(&c, &[0u8; 32], 1).unwrap();
        for b in 1u8..=40 {
            if primary_leader(&c, &[b; 32], 1).unwrap() != base {
                changed += 1;
            }
        }
        assert!(changed > 0, "beacon must influence leader selection");
    }

    #[test]
    fn fold_commitment_is_order_independent_and_advances() {
        let beacon = [1u8; 32];
        let a = fold_commitment(&beacon, &[[9u8; 32], [4u8; 32], [6u8; 32]]);
        let b = fold_commitment(&beacon, &[[4u8; 32], [6u8; 32], [9u8; 32]]);
        assert_eq!(a, b, "commit order must not affect the beacon");
        assert_ne!(a, beacon, "beacon must change after folding");
        // Idle round still advances.
        let idle = fold_commitment(&beacon, &[]);
        assert_ne!(idle, beacon);
    }

    #[test]
    fn beacon_is_unpredictable_without_the_committed_set() {
        // Two different committed frontiers give unrelated beacons — an
        // observer who cannot predict the frontier cannot predict the beacon.
        let beacon = [5u8; 32];
        let x = fold_commitment(&beacon, &[[1u8; 32]]);
        let y = fold_commitment(&beacon, &[[2u8; 32]]);
        assert_ne!(x, y);
    }

    #[test]
    fn higher_stake_wins_more_often() {
        // Over many beacons, the high-stake validator should lead more.
        let c = candidates(&[(1, 900), (2, 100)]);
        let mut high = 0;
        for b in 0u8..=200 {
            if primary_leader(&c, &[b; 32], 0) == Some(node(1)) {
                high += 1;
            }
        }
        // ~90% expected; assert a comfortable majority.
        assert!(
            high > 130,
            "high-stake validator should lead most rounds, got {high}/201"
        );
    }

    #[test]
    fn vrf_ticket_roundtrips_and_binds_to_round() {
        let mut rng = StdRng::seed_from_u64(1);
        let sk = SigningKey::generate(&mut rng);
        let beacon = [8u8; 32];

        let proof = make_ticket(&sk, &beacon, 10);
        let prio = verify_ticket(&sk.verifying_key(), &beacon, 10, 100, &proof).unwrap();
        // Same proof for the wrong round must not verify.
        assert!(verify_ticket(&sk.verifying_key(), &beacon, 11, 100, &proof).is_err());
        // Priority is stake-monotone: more stake → smaller (stronger) key.
        let prio_more_stake = ticket_priority(&ecvrf::output(&proof).unwrap(), 1000);
        assert!(prio_more_stake < prio);
    }

    #[test]
    fn vrf_ticket_priority_matches_verified_output() {
        let mut rng = StdRng::seed_from_u64(2);
        let sk = SigningKey::generate(&mut rng);
        let beacon = [2u8; 32];
        let proof = make_ticket(&sk, &beacon, 3);
        let via_verify = verify_ticket(&sk.verifying_key(), &beacon, 3, 250, &proof).unwrap();
        let beta = ecvrf::verify(&sk.verifying_key(), &ticket_alpha(&beacon, 3), &proof).unwrap();
        assert_eq!(via_verify, ticket_priority(&beta, 250));
    }
}

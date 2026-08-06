#![allow(clippy::unwrap_used)]
//! Consensus arena — adversarial property-based gate for Lane 0
//! (ADR-025 Stage 5).
//!
//! Each property drives a fleet of simulated honest nodes (each owning an
//! independent [`CertificateStore`]) through randomized adversarial
//! schedules — ack withholding, duplication, reordering, forged
//! signatures, outsider acks, and validator-set rotations at arbitrary
//! points — and asserts the Lane 0 safety invariants hold for every
//! honest node afterwards:
//!
//! 1. **No forged finality** — an event is final at an honest node only
//!    if a genuine stake quorum of the active set acked it.
//! 2. **Convergence** — honest nodes that received the same set of acks
//!    (in any order, with any duplication) agree on finality.
//! 3. **Epoch-fence monotonicity** — a rotation never un-finalizes
//!    anything at any node.
//! 4. **Withholding is only a liveness attack** — a node that saw a
//!    subset of the acks never finalizes an event that the full set of
//!    acks would not justify.
//!
//! Lane 1's equivalent adversarial gate (equivocation, partition/heal)
//! already lives in `omnia-chaos-tests`; this file covers the Lane 0
//! certificate layer and the Stage 4 epoch fence, completing the
//! "consensus arena" CI gate from ADR-025 Stage 5.

use std::sync::OnceLock;

use omnia_substrate::crypto::{generate_keypair, NodeKeypair};
use omnia_substrate::lane0::{AckOutcome, CertificateStore, Lane0Error, SignedAck, ValidatorSet, UNBOUND_STATE_ROOT};
use proptest::prelude::*;

/// Deterministic event id from a small integer.
fn eid(n: u8) -> [u8; 32] {
    let mut id = [0u8; 32];
    id[0] = n;
    id
}

/// Shared keypair pool. Ed25519 keypair generation dominates runtime when
/// every proptest case regenerates its fleet, so all cases draw from one
/// pre-generated pool instead. Two disjoint halves let the rotation
/// property build genuinely fresh "new set" members from the second half.
const POOL_SIZE: usize = 16;

fn keypair_pool() -> &'static [NodeKeypair] {
    static POOL: OnceLock<Vec<NodeKeypair>> = OnceLock::new();
    POOL.get_or_init(|| (0..POOL_SIZE).map(|_| generate_keypair()).collect())
}

/// A fleet of validator keypairs plus the matching equal-stake set,
/// drawn from `keypair_pool()[offset..offset + count]`.
fn make_validators(offset: usize, count: usize) -> (Vec<NodeKeypair>, ValidatorSet) {
    let keys: Vec<NodeKeypair> = keypair_pool()[offset..offset + count].to_vec();
    let set = ValidatorSet::new(keys.iter().map(|k| (k.verifying_key().to_bytes(), 1))).unwrap();
    (keys, set)
}

/// Ground truth: does `delivered` (indices into `keys`) constitute a
/// quorum of `set`? Computed independently of CertificateStore so the
/// test does not trust the code under test.
fn is_true_quorum(delivered: &[usize], keys: &[NodeKeypair], set: &ValidatorSet) -> bool {
    let mut unique: Vec<usize> = delivered.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let stake: u64 = unique
        .iter()
        .filter_map(|&i| set.stake_of(&keys[i].verifying_key().to_bytes()))
        .sum();
    set.is_quorum(stake)
}

proptest! {
    // 64 randomized cases per property: the deterministic edge cases are
    // already pinned by lane0.rs's unit tests; the arena's job is broad
    // schedule coverage without blowing up CI wall-time (Ed25519 signing
    // per delivered ack is the dominant cost).
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Invariant 1 + 4: for ANY subset of validator acks delivered to a
    /// node (with arbitrary duplication and order), the node finalizes
    /// the event IFF the distinct delivered validators form a genuine
    /// stake quorum. Finality can never be conjured from withheld,
    /// duplicated, or reordered acks — and never withheld when a true
    /// quorum was delivered.
    #[test]
    fn arena_finality_iff_true_quorum(
        validator_count in 1usize..7,
        // Delivery schedule: indices (mod validator_count) with duplicates.
        schedule in prop::collection::vec(0usize..7, 0..20),
    ) {
        let (keys, set) = make_validators(0, validator_count);
        let mut store = CertificateStore::new();
        let id = eid(1);

        let delivered: Vec<usize> = schedule.iter().map(|i| i % validator_count).collect();
        for &i in &delivered {
            let _ = store.add_ack(SignedAck::sign(id, UNBOUND_STATE_ROOT, &keys[i]), &set);
        }

        prop_assert_eq!(
            store.is_final(&id),
            is_true_quorum(&delivered, &keys, &set),
            "finality must match ground-truth quorum: delivered={:?} of {} validators",
            delivered, validator_count
        );
    }

    /// Invariant 2 (convergence): two honest nodes receiving the same
    /// acks in different orders — one of them additionally receiving
    /// arbitrary duplicates — reach identical finality for every event.
    #[test]
    fn arena_convergence_under_reorder_and_duplication(
        validator_count in 1usize..6,
        event_count in 1usize..4,
        schedule in prop::collection::vec((0usize..6, 0usize..4), 1..24),
        seed in 0u64..u64::MAX,
    ) {
        let (keys, set) = make_validators(0, validator_count);

        // The canonical multiset of (validator, event) ack deliveries.
        let deliveries: Vec<(usize, u8)> = schedule
            .iter()
            .map(|&(v, e)| (v % validator_count, (e % event_count) as u8))
            .collect();

        // Node A: in-order delivery.
        let mut node_a = CertificateStore::new();
        for &(v, e) in &deliveries {
            let _ = node_a.add_ack(SignedAck::sign(eid(e), UNBOUND_STATE_ROOT, &keys[v]), &set);
        }

        // Node B: deterministically shuffled (seeded Fisher-Yates via
        // a simple LCG — no rand dependency needed) plus every ack
        // delivered twice.
        let mut shuffled = deliveries.clone();
        let mut state = seed | 1;
        for i in (1..shuffled.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (state >> 33) as usize % (i + 1);
            shuffled.swap(i, j);
        }
        let mut node_b = CertificateStore::new();
        for &(v, e) in &shuffled {
            let _ = node_b.add_ack(SignedAck::sign(eid(e), UNBOUND_STATE_ROOT, &keys[v]), &set);
            let _ = node_b.add_ack(SignedAck::sign(eid(e), UNBOUND_STATE_ROOT, &keys[v]), &set); // duplicate
        }

        for e in 0..event_count as u8 {
            prop_assert_eq!(
                node_a.is_final(&eid(e)),
                node_b.is_final(&eid(e)),
                "honest nodes diverged on event {} under reorder+duplication", e
            );
        }
    }

    /// Invariant 3 (epoch fence): whatever was final before a rotation
    /// stays final after it, at every node, for ANY new validator set —
    /// including sets disjoint from the old one. And any event newly
    /// finalized by the rotation itself must satisfy the NEW set's
    /// ground-truth quorum from the acks that survive the rotation.
    #[test]
    fn arena_rotation_is_monotone_and_sound(
        old_count in 1usize..5,
        new_count in 1usize..5,
        overlap in 0usize..5,
        schedule in prop::collection::vec((0usize..5, 0usize..3), 0..18),
    ) {
        let (old_keys, old_set) = make_validators(0, old_count);

        // New set: `overlap` carried-over members plus fresh ones from
        // the pool's second half (disjoint from the old set's range).
        let carry = overlap.min(old_count).min(new_count);
        let mut new_keys: Vec<NodeKeypair> = old_keys.iter().take(carry).cloned().collect();
        let mut fresh = keypair_pool()[POOL_SIZE / 2..].iter();
        while new_keys.len() < new_count {
            new_keys.push(fresh.next().expect("pool large enough").clone());
        }
        let new_set = ValidatorSet::new(new_keys.iter().map(|k| (k.verifying_key().to_bytes(), 1))).unwrap();

        let mut store = CertificateStore::new();
        let deliveries: Vec<(usize, u8)> = schedule
            .iter()
            .map(|&(v, e)| (v % old_count, (e % 3) as u8))
            .collect();
        for &(v, e) in &deliveries {
            let _ = store.add_ack(SignedAck::sign(eid(e), UNBOUND_STATE_ROOT, &old_keys[v]), &old_set);
        }

        let final_before: Vec<bool> = (0..3u8).map(|e| store.is_final(&eid(e))).collect();
        let newly_final = store.rotate_validators(&new_set);

        for e in 0..3u8 {
            // Monotone: nothing un-finalizes.
            if final_before[e as usize] {
                prop_assert!(
                    store.is_final(&eid(e)),
                    "rotation un-finalized event {} — epoch fence violated", e
                );
            }
            // Sound: anything the rotation newly finalized must have a
            // genuine quorum of the NEW set among the SURVIVING ackers
            // (old-set members that are also new-set members).
            if newly_final.contains(&eid(e)) {
                let survivors: Vec<usize> = deliveries
                    .iter()
                    .filter(|&&(_, ev)| ev == e)
                    .map(|&(v, _)| v)
                    .filter(|&v| new_set.contains(&old_keys[v].verifying_key().to_bytes()))
                    .collect();
                prop_assert!(
                    is_true_quorum_keys(&survivors, &old_keys, &new_set),
                    "rotation finalized event {} without a genuine new-set quorum", e
                );
            }
        }
    }

    /// Byzantine inputs: forged signatures and outsider acks are always
    /// rejected, never mutate certificates, and never finalize anything
    /// — regardless of how many are thrown at the store.
    #[test]
    fn arena_forgeries_and_outsiders_never_finalize(
        validator_count in 1usize..5,
        attack_count in 1usize..16,
        honest_acks in 0usize..2,
    ) {
        let (keys, set) = make_validators(0, validator_count);
        let mut store = CertificateStore::new();
        let id = eid(1);

        // A sub-quorum trickle of honest acks (never enough to finalize
        // unless validator_count == 1, in which case skip honest acks).
        let honest_delivered = if validator_count > 1 { honest_acks.min(1) } else { 0 };
        for k in keys.iter().take(honest_delivered) {
            let _ = store.add_ack(SignedAck::sign(id, UNBOUND_STATE_ROOT, k), &set);
        }

        // Pool tail is disjoint from the set (which uses pool[0..validator_count]).
        let outsider = keypair_pool()[POOL_SIZE - 1].clone();
        for i in 0..attack_count {
            if i % 2 == 0 {
                // Outsider: valid signature, not in the set.
                let ack = SignedAck::sign(id, UNBOUND_STATE_ROOT, &outsider);
                prop_assert!(matches!(store.add_ack(ack, &set), Err(Lane0Error::UnknownValidator)));
            } else {
                // Forgery: in-set pubkey, corrupted signature.
                let mut forged = SignedAck::sign(id, UNBOUND_STATE_ROOT, &keys[i % validator_count]);
                forged.signature[i % 64] ^= 0xFF;
                prop_assert!(matches!(store.add_ack(forged, &set), Err(Lane0Error::InvalidSignature)));
            }
        }

        prop_assert!(!store.is_final(&id), "attack traffic must never produce finality");
        let (accepted, rejected, finalized) = store.stats();
        prop_assert_eq!(accepted, honest_delivered as u64);
        prop_assert_eq!(rejected, attack_count as u64);
        prop_assert_eq!(finalized, 0);
    }

    /// Duplicate replay is idempotent at any point in the schedule: the
    /// final state after N deliveries of the same ack equals the state
    /// after 1 — and the AckOutcome sequence reflects it.
    #[test]
    fn arena_replay_is_idempotent(
        validator_count in 2usize..5,
        replays in 1usize..10,
    ) {
        let (keys, set) = make_validators(0, validator_count);
        let mut store = CertificateStore::new();
        let id = eid(1);
        let ack = SignedAck::sign(id, UNBOUND_STATE_ROOT, &keys[0]);

        prop_assert_eq!(store.add_ack(ack.clone(), &set).unwrap(), AckOutcome::Recorded);
        for _ in 0..replays {
            prop_assert_eq!(store.add_ack(ack.clone(), &set).unwrap(), AckOutcome::Duplicate);
        }
        prop_assert_eq!(store.certificate(&id).unwrap().acked_stake(), 1);
        prop_assert_eq!(store.certificate(&id).unwrap().ack_count(), 1);
        prop_assert!(!store.is_final(&id));
    }
}

/// Ground-truth quorum for a set of surviving acker indices against an
/// arbitrary validator set (used by the rotation property, where the
/// membership set differs from the signing keys' original set).
fn is_true_quorum_keys(indices: &[usize], keys: &[NodeKeypair], set: &ValidatorSet) -> bool {
    let mut unique: Vec<usize> = indices.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let stake: u64 = unique
        .iter()
        .filter_map(|&i| set.stake_of(&keys[i].verifying_key().to_bytes()))
        .sum();
    set.is_quorum(stake)
}

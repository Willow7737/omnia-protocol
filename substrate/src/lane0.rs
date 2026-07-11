//! Lane 0 — consensusless fast-path finality for single-writer operations
//! (ADR-025 Stage 3, v1).
//!
//! UBC operations are single-writer by construction: soulbound semantics
//! mean a transfer only ever debits the sender's own balance, and senders
//! already totally order their own events via `sequence` + self-parent
//! chaining. Such events need no network-wide total ordering — they are
//! **final** as soon as a stake-weighted quorum of validators has
//! acknowledged them.
//!
//! # Protocol
//!
//! 1. A validator that validates + inserts an event into its causal graph
//!    signs a [`SignedAck`] over `blake3_hash_domain("omnia-lane0-ack", event_id)`
//!    and gossips it on the dedicated `omnia_lane0_acks` topic.
//! 2. Every node folds received acks into its [`CertificateStore`]. A
//!    per-event certificate is a **grow-only set CRDT** keyed by validator
//!    public key: merging is idempotent, commutative, and associative, so
//!    duplicate or reordered gossip deliveries are harmless.
//! 3. When the acked stake exceeds 2/3 of the configured total stake, the
//!    event is Lane 0-final. Finality is monotone — once final, always
//!    final.
//!
//! # Validator set (v1: static)
//!
//! ADR-025 routes validator-set *changes* through Lane 1 (they are
//! contested, shared-state operations). Until Lane 1 lands, the validator
//! set is operator-configured via `OMNIA_LANE0_VALIDATORS` — a
//! comma-separated list of `hex64_ed25519_pubkey:stake` entries. When the
//! variable is unset or empty, Lane 0 is **disabled** and this module is
//! inert (no acks signed, published, or accepted).
//!
//! # Bounded memory
//!
//! The store tracks at most [`MAX_TRACKED_EVENTS`] in-flight certificates
//! and remembers at most [`MAX_FINALIZED_EVENTS`] finalized event IDs
//! (oldest evicted first). An ack for an evicted event simply reopens a
//! certificate — safety is unaffected because certificates only ever grow
//! from verified acks.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::blake3_domain::blake3_hash_domain;
use crate::crypto::{NodeKeypair, NodePublicKey, Signer, Verifier};
use omnia_primitives::EventId;

/// Domain separator for Lane 0 acknowledgment signatures.
///
/// Signing a *domain-separated hash* of the event ID (rather than the raw
/// ID) guarantees a Lane 0 ack can never be replayed as an event
/// signature or any other protocol signature, and vice versa.
pub const LANE0_ACK_DOMAIN: &[u8] = b"omnia-lane0-ack";

/// Gossipsub topic on which Lane 0 acks are broadcast.
pub const LANE0_ACKS_TOPIC: &str = "omnia_lane0_acks";

/// Wire-format version byte for serialized ack batches.
pub const LANE0_WIRE_VERSION: u8 = 1;

/// Maximum acks accepted in a single gossip message (DoS bound).
pub const MAX_ACKS_PER_MESSAGE: usize = 1024;

/// Maximum in-flight (not yet final) certificates tracked.
pub const MAX_TRACKED_EVENTS: usize = 100_000;

/// Maximum finalized event IDs remembered.
pub const MAX_FINALIZED_EVENTS: usize = 100_000;

/// Errors from Lane 0 processing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Lane0Error {
    /// The ack's signature does not verify against its claimed public key.
    #[error("invalid ack signature")]
    InvalidSignature,
    /// The ack's public key is not in the configured validator set.
    #[error("ack from unknown validator")]
    UnknownValidator,
    /// Serialization/deserialization failed.
    #[error("codec error: {0}")]
    Codec(String),
    /// Config parse failure.
    #[error("invalid OMNIA_LANE0_VALIDATORS entry: {0}")]
    InvalidConfig(String),
}

/// A validator's signed acknowledgment that an event validated cleanly
/// and was inserted into its causal graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedAck {
    /// The acknowledged event.
    pub event_id: EventId,
    /// Ed25519 public key of the acking validator.
    pub validator_pubkey: [u8; 32],
    /// Ed25519 signature over `blake3_hash_domain(LANE0_ACK_DOMAIN, event_id)`.
    #[serde(with = "serde_sig64")]
    pub signature: [u8; 64],
}

/// Serde helper: serde has no built-in impls for `[u8; 64]`.
mod serde_sig64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(data)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        let bytes: Vec<u8> = Vec::deserialize(d)?;
        bytes
            .as_slice()
            .try_into()
            .map_err(|_| serde::de::Error::custom(format!("expected 64 bytes, got {}", bytes.len())))
    }
}

impl SignedAck {
    /// Sign an acknowledgment for `event_id` with the validator's keypair.
    pub fn sign(event_id: EventId, keypair: &NodeKeypair) -> Self {
        let digest = blake3_hash_domain(LANE0_ACK_DOMAIN, &event_id);
        let signature = keypair.sign(&digest).to_bytes();
        Self {
            event_id,
            validator_pubkey: keypair.verifying_key().to_bytes(),
            signature,
        }
    }

    /// Verify the ack's signature against its claimed public key.
    pub fn verify(&self) -> bool {
        let Ok(pubkey) = NodePublicKey::from_bytes(&self.validator_pubkey) else {
            return false;
        };
        let Ok(signature) = ed25519_dalek::Signature::from_slice(&self.signature) else {
            return false;
        };
        let digest = blake3_hash_domain(LANE0_ACK_DOMAIN, &self.event_id);
        pubkey.verify(&digest, &signature).is_ok()
    }
}

/// Serialize a batch of acks for the gossip wire:
/// `[LANE0_WIRE_VERSION] ++ postcard(Vec<SignedAck>)`.
pub fn encode_ack_batch(acks: &[SignedAck]) -> Result<Vec<u8>, Lane0Error> {
    let mut bytes = vec![LANE0_WIRE_VERSION];
    bytes.extend(postcard::to_allocvec(acks).map_err(|e| Lane0Error::Codec(e.to_string()))?);
    Ok(bytes)
}

/// Decode a batch of acks from the gossip wire, enforcing the version
/// byte and the [`MAX_ACKS_PER_MESSAGE`] bound.
pub fn decode_ack_batch(data: &[u8]) -> Result<Vec<SignedAck>, Lane0Error> {
    match data.split_first() {
        Some((&LANE0_WIRE_VERSION, rest)) => {
            let acks: Vec<SignedAck> = postcard::from_bytes(rest).map_err(|e| Lane0Error::Codec(e.to_string()))?;
            if acks.len() > MAX_ACKS_PER_MESSAGE {
                return Err(Lane0Error::Codec(format!(
                    "ack batch too large: {} (max {})",
                    acks.len(),
                    MAX_ACKS_PER_MESSAGE
                )));
            }
            Ok(acks)
        }
        Some((v, _)) => Err(Lane0Error::Codec(format!("unknown lane0 wire version: {v}"))),
        None => Err(Lane0Error::Codec("empty lane0 message".to_string())),
    }
}

/// The static Lane 0 validator set: Ed25519 public key → stake weight.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidatorSet {
    stakes: BTreeMap<[u8; 32], u64>,
    total_stake: u64,
}

impl ValidatorSet {
    /// Build a validator set from `(pubkey, stake)` pairs.
    ///
    /// Zero-stake entries are rejected — they could never contribute to a
    /// quorum and would only inflate the map.
    pub fn new(entries: impl IntoIterator<Item = ([u8; 32], u64)>) -> Result<Self, Lane0Error> {
        let mut stakes = BTreeMap::new();
        for (pubkey, stake) in entries {
            if stake == 0 {
                return Err(Lane0Error::InvalidConfig(format!(
                    "zero stake for validator {}",
                    hex::encode(pubkey)
                )));
            }
            stakes.insert(pubkey, stake);
        }
        let total_stake = stakes
            .values()
            .try_fold(0u64, |acc, s| acc.checked_add(*s))
            .ok_or_else(|| Lane0Error::InvalidConfig("total stake overflows u64".to_string()))?;
        Ok(Self { stakes, total_stake })
    }

    /// Parse the `OMNIA_LANE0_VALIDATORS` format:
    /// `hex64_pubkey:stake[,hex64_pubkey:stake...]`.
    ///
    /// Returns `Ok(None)` for an empty/whitespace-only string (Lane 0
    /// disabled), `Err` for a malformed one — a typo must fail loudly
    /// rather than silently disable finality.
    pub fn parse(spec: &str) -> Result<Option<Self>, Lane0Error> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Ok(None);
        }
        let mut entries = Vec::new();
        for part in spec.split(',') {
            let part = part.trim();
            let (pk_hex, stake_str) = part
                .split_once(':')
                .ok_or_else(|| Lane0Error::InvalidConfig(format!("missing ':' in '{part}'")))?;
            let pk_bytes = hex::decode(pk_hex.trim())
                .map_err(|e| Lane0Error::InvalidConfig(format!("bad pubkey hex in '{part}': {e}")))?;
            let pubkey: [u8; 32] = pk_bytes
                .as_slice()
                .try_into()
                .map_err(|_| Lane0Error::InvalidConfig(format!("pubkey must be 32 bytes in '{part}'")))?;
            let stake: u64 = stake_str
                .trim()
                .parse()
                .map_err(|e| Lane0Error::InvalidConfig(format!("bad stake in '{part}': {e}")))?;
            entries.push((pubkey, stake));
        }
        Ok(Some(Self::new(entries)?))
    }

    /// Stake of a validator, or `None` if not a member.
    pub fn stake_of(&self, pubkey: &[u8; 32]) -> Option<u64> {
        self.stakes.get(pubkey).copied()
    }

    /// Whether `pubkey` is a member of the set.
    pub fn contains(&self, pubkey: &[u8; 32]) -> bool {
        self.stakes.contains_key(pubkey)
    }

    /// Sum of all stakes.
    pub fn total_stake(&self) -> u64 {
        self.total_stake
    }

    /// Number of validators.
    pub fn len(&self) -> usize {
        self.stakes.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.stakes.is_empty()
    }

    /// The BFT quorum test: strictly more than 2/3 of total stake.
    ///
    /// Uses u128 arithmetic so `stake * 3` cannot overflow.
    pub fn is_quorum(&self, acked_stake: u64) -> bool {
        (acked_stake as u128) * 3 > (self.total_stake as u128) * 2
    }
}

/// A per-event finality certificate: the grow-only set of verified acks,
/// keyed by validator public key (G-Set CRDT — merge is set union).
#[derive(Clone, Debug, Default)]
pub struct FinalityCertificate {
    acks: BTreeMap<[u8; 32], SignedAck>,
    acked_stake: u64,
}

impl FinalityCertificate {
    /// The verified acks collected so far.
    pub fn acks(&self) -> impl Iterator<Item = &SignedAck> {
        self.acks.values()
    }

    /// Total stake represented by the collected acks.
    pub fn acked_stake(&self) -> u64 {
        self.acked_stake
    }

    /// Number of distinct validators that have acked.
    pub fn ack_count(&self) -> usize {
        self.acks.len()
    }
}

/// Outcome of folding one ack into the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckOutcome {
    /// The ack was new and the event is now Lane 0-final.
    NewlyFinal,
    /// The ack was recorded; quorum not yet reached.
    Recorded,
    /// The ack was a duplicate or the event is already final — no change.
    Duplicate,
}

/// Bounded store of in-flight certificates and finalized event IDs.
#[derive(Debug, Default)]
pub struct CertificateStore {
    /// In-flight certificates (not yet final).
    pending: HashMap<EventId, FinalityCertificate>,
    /// Insertion order of `pending`, for bounded eviction.
    pending_order: VecDeque<EventId>,
    /// Finalized event IDs.
    finalized: HashSet<EventId>,
    /// Insertion order of `finalized`, for bounded eviction.
    finalized_order: VecDeque<EventId>,
    /// Total acks accepted (unique, verified).
    acks_accepted: u64,
    /// Total acks rejected (bad signature / unknown validator).
    acks_rejected: u64,
    /// Total events finalized through Lane 0.
    events_finalized: u64,
}

impl CertificateStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Verify and fold one ack into the store.
    ///
    /// Rejects acks with invalid signatures or from public keys outside
    /// `validators`. Duplicate acks are no-ops (G-Set merge semantics).
    pub fn add_ack(&mut self, ack: SignedAck, validators: &ValidatorSet) -> Result<AckOutcome, Lane0Error> {
        let Some(stake) = validators.stake_of(&ack.validator_pubkey) else {
            self.acks_rejected += 1;
            return Err(Lane0Error::UnknownValidator);
        };
        if !ack.verify() {
            self.acks_rejected += 1;
            return Err(Lane0Error::InvalidSignature);
        }

        if self.finalized.contains(&ack.event_id) {
            return Ok(AckOutcome::Duplicate);
        }

        let is_new_event = !self.pending.contains_key(&ack.event_id);
        let cert = self.pending.entry(ack.event_id).or_default();
        if cert.acks.contains_key(&ack.validator_pubkey) {
            return Ok(AckOutcome::Duplicate);
        }
        let event_id = ack.event_id;
        cert.acked_stake = cert.acked_stake.saturating_add(stake);
        cert.acks.insert(ack.validator_pubkey, ack);
        self.acks_accepted += 1;

        if is_new_event {
            self.pending_order.push_back(event_id);
            // Bounded memory: evict the oldest in-flight certificate.
            while self.pending.len() > MAX_TRACKED_EVENTS {
                if let Some(evicted) = self.pending_order.pop_front() {
                    self.pending.remove(&evicted);
                } else {
                    break;
                }
            }
        }

        let acked = self.pending.get(&event_id).map(|c| c.acked_stake).unwrap_or(0);
        if validators.is_quorum(acked) {
            self.pending.remove(&event_id);
            self.finalized.insert(event_id);
            self.finalized_order.push_back(event_id);
            self.events_finalized += 1;
            while self.finalized.len() > MAX_FINALIZED_EVENTS {
                if let Some(evicted) = self.finalized_order.pop_front() {
                    self.finalized.remove(&evicted);
                } else {
                    break;
                }
            }
            Ok(AckOutcome::NewlyFinal)
        } else {
            Ok(AckOutcome::Recorded)
        }
    }

    /// Whether an event has reached Lane 0 finality.
    pub fn is_final(&self, event_id: &EventId) -> bool {
        self.finalized.contains(event_id)
    }

    /// The in-flight certificate for an event, if any.
    pub fn certificate(&self, event_id: &EventId) -> Option<&FinalityCertificate> {
        self.pending.get(event_id)
    }

    /// `(acks_accepted, acks_rejected, events_finalized)` counters.
    pub fn stats(&self) -> (u64, u64, u64) {
        (self.acks_accepted, self.acks_rejected, self.events_finalized)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::crypto::generate_keypair;

    fn eid(n: u8) -> EventId {
        let mut id = [0u8; 32];
        id[0] = n;
        id
    }

    fn three_validators() -> (Vec<NodeKeypair>, ValidatorSet) {
        let keys: Vec<NodeKeypair> = (0..3).map(|_| generate_keypair()).collect();
        let set = ValidatorSet::new(keys.iter().map(|k| (k.verifying_key().to_bytes(), 1))).unwrap();
        (keys, set)
    }

    #[test]
    fn test_ack_sign_verify_roundtrip() {
        let key = generate_keypair();
        let ack = SignedAck::sign(eid(1), &key);
        assert!(ack.verify());
    }

    #[test]
    fn test_ack_verify_rejects_tamper() {
        let key = generate_keypair();
        let mut ack = SignedAck::sign(eid(1), &key);
        ack.event_id = eid(2); // signature no longer matches
        assert!(!ack.verify());
    }

    #[test]
    fn test_ack_domain_separation() {
        // An event signature (over the raw event id) must not verify as a
        // Lane 0 ack (over the domain-separated digest).
        let key = generate_keypair();
        let id = eid(7);
        let event_style_sig = key.sign(&id).to_bytes();
        let forged = SignedAck {
            event_id: id,
            validator_pubkey: key.verifying_key().to_bytes(),
            signature: event_style_sig,
        };
        assert!(!forged.verify());
    }

    #[test]
    fn test_ack_batch_wire_roundtrip() {
        let key = generate_keypair();
        let acks = vec![SignedAck::sign(eid(1), &key), SignedAck::sign(eid(2), &key)];
        let bytes = encode_ack_batch(&acks).unwrap();
        assert_eq!(bytes[0], LANE0_WIRE_VERSION);
        let decoded = decode_ack_batch(&bytes).unwrap();
        assert_eq!(decoded, acks);
    }

    #[test]
    fn test_ack_batch_rejects_bad_version_and_garbage() {
        assert!(decode_ack_batch(&[]).is_err());
        assert!(decode_ack_batch(&[99, 0]).is_err());
        assert!(decode_ack_batch(&[LANE0_WIRE_VERSION, 0xFF, 0xFF, 0xFF]).is_err());
    }

    #[test]
    fn test_validator_set_parse() {
        let key = generate_keypair();
        let pk_hex = hex::encode(key.verifying_key().to_bytes());
        let set = ValidatorSet::parse(&format!("{pk_hex}:5")).unwrap().unwrap();
        assert_eq!(set.len(), 1);
        assert_eq!(set.total_stake(), 5);
        assert_eq!(set.stake_of(&key.verifying_key().to_bytes()), Some(5));

        // Empty spec disables Lane 0.
        assert!(ValidatorSet::parse("").unwrap().is_none());
        assert!(ValidatorSet::parse("   ").unwrap().is_none());

        // Malformed specs fail loudly.
        assert!(ValidatorSet::parse("nothex:1").is_err());
        assert!(ValidatorSet::parse(&format!("{pk_hex}:0")).is_err());
        assert!(ValidatorSet::parse(&format!("{pk_hex}")).is_err());
        assert!(ValidatorSet::parse(&format!("{pk_hex}:abc")).is_err());
    }

    #[test]
    fn test_quorum_math() {
        let set = ValidatorSet::new([([1u8; 32], 1), ([2u8; 32], 1), ([3u8; 32], 1)]).unwrap();
        // 3 validators, stake 1 each: quorum needs > 2 → all 3.
        assert!(!set.is_quorum(0));
        assert!(!set.is_quorum(1));
        assert!(!set.is_quorum(2));
        assert!(set.is_quorum(3));

        // 4 equal validators: quorum needs > 8/3 → 3.
        let set4 = ValidatorSet::new((1..=4).map(|i| ([i as u8; 32], 1))).unwrap();
        assert!(!set4.is_quorum(2));
        assert!(set4.is_quorum(3));

        // Weighted: total 10, quorum needs > 6.66 → 7.
        let weighted = ValidatorSet::new([([1u8; 32], 7), ([2u8; 32], 3)]).unwrap();
        assert!(weighted.is_quorum(7));
        assert!(!weighted.is_quorum(6));
    }

    #[test]
    fn test_quorum_no_overflow_at_max_stake() {
        let set = ValidatorSet::new([([1u8; 32], u64::MAX)]).unwrap();
        assert!(set.is_quorum(u64::MAX));
    }

    #[test]
    fn test_certificate_store_finality_flow() {
        let (keys, set) = three_validators();
        let mut store = CertificateStore::new();
        let id = eid(1);

        assert_eq!(
            store.add_ack(SignedAck::sign(id, &keys[0]), &set).unwrap(),
            AckOutcome::Recorded
        );
        assert!(!store.is_final(&id));
        assert_eq!(store.certificate(&id).unwrap().ack_count(), 1);

        assert_eq!(
            store.add_ack(SignedAck::sign(id, &keys[1]), &set).unwrap(),
            AckOutcome::Recorded
        );
        assert!(!store.is_final(&id));

        assert_eq!(
            store.add_ack(SignedAck::sign(id, &keys[2]), &set).unwrap(),
            AckOutcome::NewlyFinal
        );
        assert!(store.is_final(&id));
        assert_eq!(store.stats(), (3, 0, 1));
    }

    #[test]
    fn test_certificate_store_merge_is_idempotent() {
        let (keys, set) = three_validators();
        let mut store = CertificateStore::new();
        let id = eid(1);
        let ack = SignedAck::sign(id, &keys[0]);

        assert_eq!(store.add_ack(ack.clone(), &set).unwrap(), AckOutcome::Recorded);
        // Same ack again: duplicate, stake not double-counted.
        assert_eq!(store.add_ack(ack, &set).unwrap(), AckOutcome::Duplicate);
        assert_eq!(store.certificate(&id).unwrap().acked_stake(), 1);
        assert_eq!(store.stats().0, 1);
    }

    #[test]
    fn test_certificate_store_order_independent() {
        // CRDT property: any delivery order reaches the same final state.
        let (keys, set) = three_validators();
        let id = eid(1);
        let acks: Vec<SignedAck> = keys.iter().map(|k| SignedAck::sign(id, k)).collect();

        for order in [[0, 1, 2], [2, 0, 1], [1, 2, 0]] {
            let mut store = CertificateStore::new();
            let mut outcomes = Vec::new();
            for i in order {
                outcomes.push(store.add_ack(acks[i].clone(), &set).unwrap());
            }
            assert_eq!(outcomes.last(), Some(&AckOutcome::NewlyFinal));
            assert!(store.is_final(&id));
        }
    }

    #[test]
    fn test_certificate_store_rejects_outsiders_and_forgeries() {
        let (_, set) = three_validators();
        let outsider = generate_keypair();
        let mut store = CertificateStore::new();

        // Valid signature, but not in the validator set.
        let outsider_ack = SignedAck::sign(eid(1), &outsider);
        assert!(matches!(
            store.add_ack(outsider_ack, &set),
            Err(Lane0Error::UnknownValidator)
        ));

        // In-set pubkey with a forged signature.
        let (keys, set) = three_validators();
        let mut forged = SignedAck::sign(eid(1), &keys[0]);
        forged.signature[0] ^= 0xFF;
        assert!(matches!(store.add_ack(forged, &set), Err(Lane0Error::InvalidSignature)));
        // Both the outsider ack and the forgery were counted as rejected.
        assert_eq!(store.stats().1, 2);
    }

    #[test]
    fn test_finality_is_monotone() {
        let (keys, set) = three_validators();
        let mut store = CertificateStore::new();
        let id = eid(1);
        for k in &keys {
            let _ = store.add_ack(SignedAck::sign(id, k), &set);
        }
        assert!(store.is_final(&id));
        // A late duplicate does not un-finalize or corrupt anything.
        assert_eq!(
            store.add_ack(SignedAck::sign(id, &keys[0]), &set).unwrap(),
            AckOutcome::Duplicate
        );
        assert!(store.is_final(&id));
    }

    #[test]
    fn test_single_validator_set_self_finalizes() {
        // Single-node testnet degenerate case: own ack is a quorum.
        let key = generate_keypair();
        let set = ValidatorSet::new([(key.verifying_key().to_bytes(), 1)]).unwrap();
        let mut store = CertificateStore::new();
        assert_eq!(
            store.add_ack(SignedAck::sign(eid(1), &key), &set).unwrap(),
            AckOutcome::NewlyFinal
        );
    }
}

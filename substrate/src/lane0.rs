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
//! # Validator set (v1: operator-configured boot set; v2: epoch-fenced rotation)
//!
//! The validator set starts operator-configured via `OMNIA_LANE0_VALIDATORS`
//! — a comma-separated list of `hex64_ed25519_pubkey:stake` entries. When
//! the variable is unset or empty, Lane 0 is **disabled** and this module
//! is inert (no acks signed, published, or accepted).
//!
//! ADR-025 routes validator-set *changes* through Lane 1 (they are
//! contested, shared-state operations): the set is contested, so its
//! definition of truth cannot be a Lane 0 decision. [`CertificateStore::rotate_validators`]
//! is the epoch-fencing primitive that lets a Lane-1-committed validator-set
//! change safely replace the active set — see that method's docs for the
//! monotonicity guarantee (a certificate finalized under one epoch's set
//! stays final forever, regardless of later rotations).
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

/// Payload tag marking an event as a Lane 1 validator-set change
/// (ADR-025 Stage 4 follow-up: the rotation trigger).
///
/// An event whose payload starts with this tag carries a postcard-encoded
/// [`ValidatorSetChange`]. When such an event is **committed by Lane 1's
/// DAG consensus** (never merely gossiped or submitted), the node applies
/// the change via `Substrate::rotate_lane0_validators` — the commit is
/// the epoch fence.
pub const VSET_PAYLOAD_TAG: &[u8] = b"OMNIA_VSET_V1";

/// Maximum entries accepted in a validator-set change (DoS bound —
/// matches no realistic validator-set size while keeping decode cheap).
pub const MAX_VSET_ENTRIES: usize = 1024;

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

/// A Lane 1 validator-set-change operation: the complete replacement set.
///
/// Carried in an event payload tagged with [`VSET_PAYLOAD_TAG`]. The set
/// is a full replacement (not a delta) so the operation is idempotent
/// and self-contained — applying the same committed change twice, or
/// applying it on a node that missed earlier changes, converges to the
/// same active set.
///
/// # Authorization (v1)
///
/// A change is applied only if the committed event's `creator_pubkey` is
/// a member of the **currently active** Lane 0 validator set — existing
/// validators govern their own succession (proof-of-authority style).
/// Routing through a quadratic-voting governance proposal instead is the
/// planned upgrade once proposal execution carries typed actions; the
/// commit-time application point stays identical either way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorSetChange {
    /// The new validator set as `(ed25519_pubkey, stake)` pairs.
    pub entries: Vec<([u8; 32], u64)>,
}

/// Encode a validator-set change for an event payload:
/// `VSET_PAYLOAD_TAG ++ postcard(ValidatorSetChange)`.
pub fn encode_vset_change(change: &ValidatorSetChange) -> Result<Vec<u8>, Lane0Error> {
    if change.entries.len() > MAX_VSET_ENTRIES {
        return Err(Lane0Error::Codec(format!(
            "validator-set change too large: {} entries (max {MAX_VSET_ENTRIES})",
            change.entries.len()
        )));
    }
    let mut bytes = VSET_PAYLOAD_TAG.to_vec();
    bytes.extend(postcard::to_allocvec(change).map_err(|e| Lane0Error::Codec(e.to_string()))?);
    Ok(bytes)
}

/// Decode a validator-set change from an event payload, if the payload
/// carries the [`VSET_PAYLOAD_TAG`]. Returns `Ok(None)` for payloads of
/// other kinds (not an error — most events are not set changes), `Err`
/// for a tagged payload that fails to decode or exceeds
/// [`MAX_VSET_ENTRIES`].
pub fn decode_vset_change(payload: &[u8]) -> Result<Option<ValidatorSetChange>, Lane0Error> {
    let Some(body) = payload.strip_prefix(VSET_PAYLOAD_TAG) else {
        return Ok(None);
    };
    let change: ValidatorSetChange = postcard::from_bytes(body).map_err(|e| Lane0Error::Codec(e.to_string()))?;
    if change.entries.len() > MAX_VSET_ENTRIES {
        return Err(Lane0Error::Codec(format!(
            "validator-set change too large: {} entries (max {MAX_VSET_ENTRIES})",
            change.entries.len()
        )));
    }
    Ok(Some(change))
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
        // Surfaced in every parse error so a malformed OMNIA_LANE0_VALIDATORS
        // is diagnosable at a glance rather than via a cryptic hex error.
        const FORMAT_HINT: &str = "expected a comma-separated list of \
             `<64-hex-pubkey>:<stake>` entries (generate them with \
             scripts/setup-validators.sh)";

        let spec = spec.trim();
        if spec.is_empty() {
            return Ok(None);
        }
        let mut entries = Vec::new();
        for part in spec.split(',') {
            let part = part.trim();
            let (pk_hex, stake_str) = part.split_once(':').ok_or_else(|| {
                Lane0Error::InvalidConfig(format!("missing ':' in '{part}' — {FORMAT_HINT}"))
            })?;
            let pk_hex = pk_hex.trim();
            // A JWT (header.payload.signature) is the classic paste mistake
            // here — the benchmark script mints one, and a stray redirect can
            // land it in the env var. Name it directly instead of emitting a
            // baffling "Invalid character '.'"/"'y'" hex error.
            if pk_hex.contains('.') {
                return Err(Lane0Error::InvalidConfig(format!(
                    "'{pk_hex}' looks like a JWT, not an Ed25519 public key — {FORMAT_HINT}"
                )));
            }
            if pk_hex.len() != 64 {
                return Err(Lane0Error::InvalidConfig(format!(
                    "pubkey must be 64 hex chars (32 bytes), got {} in '{part}' — {FORMAT_HINT}",
                    pk_hex.len()
                )));
            }
            let pk_bytes = hex::decode(pk_hex).map_err(|e| {
                Lane0Error::InvalidConfig(format!("bad pubkey hex in '{part}': {e} — {FORMAT_HINT}"))
            })?;
            let pubkey: [u8; 32] = pk_bytes
                .as_slice()
                .try_into()
                .map_err(|_| Lane0Error::InvalidConfig(format!("pubkey must be 32 bytes in '{part}'")))?;
            let stake: u64 = stake_str.trim().parse().map_err(|e| {
                Lane0Error::InvalidConfig(format!("bad stake in '{part}': {e} — {FORMAT_HINT}"))
            })?;
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
    /// Number of validator-set rotations applied (see [`Self::rotate_validators`]).
    epoch: u64,
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

    /// Apply a validator-set rotation (ADR-025 Stage 4 epoch fence).
    ///
    /// MUST be called identically, at the identical logical point in the
    /// causal graph, by every honest node — i.e., driven by a Lane 1
    /// (DAG-consensus-committed) validator-set-change event. Because
    /// commit order is agreed by the DAG commit rule, every honest node
    /// applies the same rotation with the same `new_validators` at the
    /// same point, so the outcome is deterministic and independent of
    /// gossip/ack delivery order.
    ///
    /// # Monotonicity
    ///
    /// Already-finalized certificates are **never** re-evaluated:
    /// finality is permanent regardless of how many rotations follow, so
    /// a Lane 0 certificate stays valid forever once decided, even after
    /// the validator set that decided it has since changed.
    ///
    /// Pending (not-yet-final) certificates are re-evaluated against
    /// `new_validators`:
    /// - Acks from validators that are not members of the new set are
    ///   dropped — their stake no longer counts toward quorum.
    /// - Acks from validators that remain members keep their ack, now
    ///   weighted by the **new** set's stake for that validator.
    /// - If the recomputed stake now meets `new_validators.is_quorum(..)`,
    ///   the certificate is immediately finalized. This is deterministic,
    ///   so every honest node reaches the same conclusion for the same
    ///   rotation.
    ///
    /// Returns the event IDs that became newly final as a direct result
    /// of the rotation (empty in the common case), in ascending order.
    pub fn rotate_validators(&mut self, new_validators: &ValidatorSet) -> Vec<EventId> {
        self.epoch += 1;
        let mut newly_final = Vec::new();

        // Deterministic evaluation order: HashMap iteration order is not
        // itself relevant to the outcome (each certificate is evaluated
        // independently), but the returned Vec's order should be stable
        // for callers that broadcast finality.
        let mut event_ids: Vec<EventId> = self.pending.keys().copied().collect();
        event_ids.sort();

        for event_id in event_ids {
            let Some(cert) = self.pending.get_mut(&event_id) else {
                continue;
            };
            let mut acked_stake: u64 = 0;
            cert.acks.retain(|pubkey, _| match new_validators.stake_of(pubkey) {
                Some(stake) => {
                    acked_stake = acked_stake.saturating_add(stake);
                    true
                }
                None => false,
            });
            cert.acked_stake = acked_stake;

            if new_validators.is_quorum(acked_stake) {
                self.pending.remove(&event_id);
                self.finalized.insert(event_id);
                self.finalized_order.push_back(event_id);
                self.events_finalized += 1;
                newly_final.push(event_id);
            }
        }

        if !newly_final.is_empty() {
            // One-pass cleanup of pending_order rather than a retain per
            // finalized event — avoids O(n^2) when a single rotation
            // finalizes many certificates at once.
            let finalized_now: HashSet<EventId> = newly_final.iter().copied().collect();
            self.pending_order.retain(|id| !finalized_now.contains(id));
        }

        while self.finalized.len() > MAX_FINALIZED_EVENTS {
            if let Some(evicted) = self.finalized_order.pop_front() {
                self.finalized.remove(&evicted);
            } else {
                break;
            }
        }

        newly_final
    }

    /// Number of validator-set rotations applied so far (0 = original set).
    pub fn epoch(&self) -> u64 {
        self.epoch
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
        assert!(ValidatorSet::parse(&pk_hex).is_err()); // missing ':<stake>'
        assert!(ValidatorSet::parse(&format!("{pk_hex}:abc")).is_err());

        // A pasted JWT (the classic env-var mistake) fails with a targeted
        // hint naming the JWT, not a cryptic hex error.
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhZG1pbiJ9.Wi4edhVq1tTAqS6MeC0";
        let err = ValidatorSet::parse(&format!("{jwt}:1")).unwrap_err();
        assert!(
            err.to_string().contains("looks like a JWT"),
            "JWT paste should be named explicitly, got: {err}"
        );

        // A wrong-length pubkey names the 64-hex-char expectation.
        let short = ValidatorSet::parse("abcd:1").unwrap_err();
        assert!(
            short.to_string().contains("64 hex chars"),
            "short pubkey should cite the 64-hex requirement, got: {short}"
        );
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

    // ── Stage 4 trigger: validator-set-change payload codec ─────────────

    #[test]
    fn test_vset_change_roundtrip() {
        let change = ValidatorSetChange {
            entries: vec![([1u8; 32], 5), ([2u8; 32], 3)],
        };
        let bytes = encode_vset_change(&change).unwrap();
        assert!(bytes.starts_with(VSET_PAYLOAD_TAG));
        let decoded = decode_vset_change(&bytes).unwrap().unwrap();
        assert_eq!(decoded, change);
    }

    #[test]
    fn test_vset_change_ignores_other_payloads() {
        // Non-tagged payloads are Ok(None) — not an error.
        assert_eq!(decode_vset_change(b"").unwrap(), None);
        assert_eq!(decode_vset_change(b"hello omnia").unwrap(), None);
        assert_eq!(decode_vset_change(b"OMNIA_XFER_V1rest").unwrap(), None);
    }

    #[test]
    fn test_vset_change_rejects_garbage_and_oversize() {
        // Tagged but undecodable body.
        let mut garbage = VSET_PAYLOAD_TAG.to_vec();
        garbage.extend([0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(decode_vset_change(&garbage).is_err());

        // Oversize set rejected at encode time…
        let big = ValidatorSetChange {
            entries: (0..=MAX_VSET_ENTRIES).map(|i| ([(i % 251) as u8; 32], 1)).collect(),
        };
        assert!(encode_vset_change(&big).is_err());
        // …and at decode time (defense in depth against a hand-crafted
        // payload that skips the encoder).
        let mut hand_crafted = VSET_PAYLOAD_TAG.to_vec();
        hand_crafted.extend(postcard::to_allocvec(&big).unwrap());
        assert!(decode_vset_change(&hand_crafted).is_err());
    }

    // ── Stage 4: epoch-fenced validator rotation ────────────────────────

    #[test]
    fn test_rotate_validators_preserves_finalized_monotone() {
        // An event finalized under the old set must stay final forever,
        // even when the new set shares no members with the old one.
        let (keys, old_set) = three_validators();
        let mut store = CertificateStore::new();
        let id = eid(1);
        for k in &keys {
            let _ = store.add_ack(SignedAck::sign(id, k), &old_set);
        }
        assert!(store.is_final(&id));

        let (_new_keys, new_set) = three_validators(); // disjoint validator set
        let newly_final = store.rotate_validators(&new_set);

        assert!(newly_final.is_empty(), "already-final events are not re-announced");
        assert!(store.is_final(&id), "finality must survive a validator-set rotation");
        assert_eq!(store.epoch(), 1);
    }

    #[test]
    fn test_rotate_validators_drops_nonmember_acks() {
        // A pending cert with 2/3 acks under the old set: rotating to a
        // set that excludes one acker must drop that ack's stake.
        let (keys, old_set) = three_validators();
        let mut store = CertificateStore::new();
        let id = eid(1);
        store.add_ack(SignedAck::sign(id, &keys[0]), &old_set).unwrap();
        store.add_ack(SignedAck::sign(id, &keys[1]), &old_set).unwrap();
        assert_eq!(store.certificate(&id).unwrap().acked_stake(), 2);
        assert!(!store.is_final(&id));

        // New set: only keys[0] carries over, plus two new validators.
        let extra: Vec<NodeKeypair> = (0..2).map(|_| generate_keypair()).collect();
        let new_set = ValidatorSet::new(
            std::iter::once((keys[0].verifying_key().to_bytes(), 1))
                .chain(extra.iter().map(|k| (k.verifying_key().to_bytes(), 1))),
        )
        .unwrap();

        let newly_final = store.rotate_validators(&new_set);
        assert!(newly_final.is_empty());
        assert!(!store.is_final(&id));
        // Only keys[0]'s ack survives; keys[1]'s stake is dropped.
        assert_eq!(store.certificate(&id).unwrap().acked_stake(), 1);
        assert_eq!(store.certificate(&id).unwrap().ack_count(), 1);
    }

    #[test]
    fn test_rotate_validators_can_immediately_finalize() {
        // A pending cert with acks that don't reach quorum under the old
        // (larger) set can cross quorum immediately upon rotating to a
        // smaller set where the surviving acks are now a supermajority.
        let (keys, old_set) = three_validators(); // quorum = 3 of 3
        let mut store = CertificateStore::new();
        let id = eid(1);
        // Only 1 of 3 acked under the old set — nowhere near quorum.
        store.add_ack(SignedAck::sign(id, &keys[0]), &old_set).unwrap();
        assert!(!store.is_final(&id));

        // New set: just the one validator that already acked.
        let new_set = ValidatorSet::new([(keys[0].verifying_key().to_bytes(), 1)]).unwrap();
        let newly_final = store.rotate_validators(&new_set);

        assert_eq!(newly_final, vec![id]);
        assert!(store.is_final(&id));
    }

    #[test]
    fn test_rotate_validators_deterministic_ascending_order() {
        // When a rotation finalizes multiple certificates at once, the
        // returned Vec must be in ascending EventId order so callers get
        // a stable, reproducible result.
        let key = generate_keypair();
        let old_set = ValidatorSet::new([(key.verifying_key().to_bytes(), 1), ([0xFF; 32], 1)]).unwrap();
        let mut store = CertificateStore::new();

        let ids = [eid(3), eid(1), eid(2)];
        for id in ids {
            store.add_ack(SignedAck::sign(id, &key), &old_set).unwrap();
        }

        // New set: just this one validator — every pending cert now has
        // a lone quorum-crossing ack.
        let new_set = ValidatorSet::new([(key.verifying_key().to_bytes(), 1)]).unwrap();
        let newly_final = store.rotate_validators(&new_set);

        assert_eq!(newly_final, vec![eid(1), eid(2), eid(3)]);
    }

    #[test]
    fn test_rotate_validators_epoch_increments_even_with_no_pending() {
        let (_keys, set) = three_validators();
        let mut store = CertificateStore::new();
        assert_eq!(store.epoch(), 0);
        store.rotate_validators(&set);
        assert_eq!(store.epoch(), 1);
        store.rotate_validators(&set);
        assert_eq!(store.epoch(), 2);
    }
}

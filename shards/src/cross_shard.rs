//! Cross-shard messaging protocol.
//!
//! Cross-shard messages let one shard drive an operation on another. The
//! causal graph's vector clocks order them; the **attestation** below is
//! what *authorizes* them.
//!
//! # AUDIT-2026-07 C4 (#342): authentication is not authorization
//!
//! The previous design carried a `causal_proof` vector clock set by the
//! message creator and a `source_signature` verified against
//! `event.creator_pubkey` — **both attacker-controlled**. Any
//! authenticated user could create an event carrying a cross-shard message
//! claiming any source shard produced any operation, sign it with their own
//! key, and the router accepted it: the signature checked out (it was the
//! attacker's own key) and the causal proof was self-referential. Cross-
//! shard messaging had authentication but no authorization.
//!
//! The fix: a cross-shard message must carry an **attestation** — a
//! signature by the *source shard's registered validator-set key* over a
//! commitment to the source shard's state transition
//! (`source_shard, target_shard, source_event_id, source_state_root,
//! payload, causal_proof`). The router verifies the attestation against the
//! key registered for `source_shard` (see
//! `ShardRouter::register_shard_attestation_key`), **not** against whoever
//! created the carrying event. A user who does not hold the source shard's
//! attestation key cannot forge a message from that shard.
//!
//! The registered key is a node-operator/genesis action, deliberately not
//! reachable from consensus payloads — the same fail-closed registry
//! pattern used for the ZK circuit registry (C9). In production the
//! attestation key is the source shard's validator-set aggregate key (the
//! threshold-BLS group key from C2); this module verifies an Ed25519
//! attestation, which a single-signer or threshold-adapter validator set
//! produces. A full IBC-style relay (per-shard validator DKG, state-root
//! binding to committed source state, timeouts/acks) is tracked follow-up
//! on #342; this closes the authorization hole that made messages
//! forgeable.

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

use crate::shard::ShardId;

/// Domain separator for the cross-shard attestation commitment.
const ATTESTATION_DOMAIN: &[u8] = b"OMNIA-CROSS-SHARD-ATTESTATION-V1";

/// A message sent from one shard to another through the causal graph.
///
/// Cross-shard messages are embedded as `ShardOp::CrossShard` operations
/// inside events. The target shard detects these messages by scanning events
/// whose `ShardPayload.shard_id` matches its own, and whose operation is a
/// `CrossShardMessage` targeting it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossShardMessage {
    /// The shard that sent this message.
    pub source_shard: ShardId,
    /// The shard that should receive this message.
    pub target_shard: ShardId,
    /// Serialized operation for the target shard to execute.
    pub payload: Vec<u8>,
    /// The source shard's event that produced this operation. Binds the
    /// attestation to a specific source-shard state transition so it
    /// cannot be replayed for a different one.
    #[serde(default)]
    pub source_event_id: [u8; 32],
    /// The source shard's state root after applying the originating
    /// operation. Part of the attested commitment.
    #[serde(default)]
    pub source_state_root: [u8; 32],
    /// Vector clock proving the source operation happened before this message.
    pub causal_proof: VectorClock,
    /// Attestation signature by the source shard's registered validator-set
    /// key over [`attestation_commitment`](Self::attestation_commitment).
    /// The router rejects the message if this is absent or does not verify
    /// against the key registered for `source_shard`.
    #[serde(default)]
    pub source_signature: Option<Vec<u8>>,
}

impl CrossShardMessage {
    /// Create a new, unattested cross-shard message.
    ///
    /// Production callers must then call [`attest`](Self::attest) with the
    /// source shard's validator-set keypair before sending; the receiving
    /// router rejects any message whose attestation is missing or does not
    /// verify against the registered source-shard key.
    pub fn new(
        source_shard: ShardId,
        target_shard: ShardId,
        payload: Vec<u8>,
        source_event_id: [u8; 32],
        source_state_root: [u8; 32],
        causal_proof: VectorClock,
    ) -> Self {
        Self {
            source_shard,
            target_shard,
            payload,
            source_event_id,
            source_state_root,
            causal_proof,
            source_signature: None,
        }
    }

    /// The commitment the source shard's validator-set key attests to.
    ///
    /// Binds every authorization-relevant field: the source and target
    /// shards, the originating source event and its resulting state root,
    /// the payload, and the causal proof. Domain-separated so an
    /// attestation cannot be repurposed as any other kind of signature.
    pub fn attestation_commitment(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(ATTESTATION_DOMAIN);
        hasher.update(&self.source_shard.0);
        hasher.update(&self.target_shard.0);
        hasher.update(&self.source_event_id);
        hasher.update(&self.source_state_root);
        hasher.update(&(self.payload.len() as u64).to_le_bytes());
        hasher.update(&self.payload);
        if let Ok(proof_bytes) = self.causal_proof.to_bytes() {
            hasher.update(&(proof_bytes.len() as u64).to_le_bytes());
            hasher.update(&proof_bytes);
        }
        *hasher.finalize().as_bytes()
    }

    /// Attest this message with the source shard's validator-set keypair.
    ///
    /// Signs [`attestation_commitment`](Self::attestation_commitment). The
    /// public half of this keypair must be registered for `source_shard`
    /// on the receiving router (`register_shard_attestation_key`), or the
    /// message will be rejected.
    pub fn attest(&mut self, keypair: &omnia_substrate::crypto::NodeKeypair) {
        use ed25519_dalek::Signer;
        let sig: ed25519_dalek::Signature = keypair.sign(&self.attestation_commitment());
        self.source_signature = Some(sig.to_bytes().to_vec());
    }

    /// Verify that the source event causally precedes this message.
    ///
    /// Returns `true` if `source_vc` (the vector clock at the source shard
    /// when the originating operation was applied) happened before the
    /// `target_vc` (the current vector clock at the target shard). This is
    /// an *ordering* check — authorization is
    /// [`verify_attestation`](Self::verify_attestation).
    pub fn verify_causality(&self, source_vc: &VectorClock, target_vc: &VectorClock) -> bool {
        source_vc.happened_before(target_vc)
    }

    /// Verify the attestation against the source shard's **registered**
    /// validator-set public key.
    ///
    /// This is the authorization check the router performs. `source_key`
    /// must be the key the router has registered for `self.source_shard` —
    /// never the carrying event's creator key. Returns `false` if the
    /// attestation is absent, malformed, or does not verify.
    pub fn verify_attestation(&self, source_key: &[u8; 32]) -> bool {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let sig = match &self.source_signature {
            Some(s) if s.len() == 64 => match Signature::from_slice(s) {
                Ok(sig) => sig,
                Err(_) => return false,
            },
            _ => return false,
        };
        let pk = match VerifyingKey::from_bytes(source_key) {
            Ok(pk) => pk,
            Err(_) => return false,
        };
        pk.verify(&self.attestation_commitment(), &sig).is_ok()
    }
}

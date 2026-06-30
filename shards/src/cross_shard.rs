//! Cross-shard messaging protocol
//!
//! Cross-shard messages allow one shard to communicate with another through
//! the causal graph. The key insight is that cross-shard messages leverage
//! the same vector clock mechanism that the substrate uses for causal
//! ordering — if Shard A sends a message to Shard B, the vector clock
//! captures the dependency, ensuring that B processes the message only
//! after A's state transition is causally confirmed.

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

use crate::shard::ShardId;

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
    /// Vector clock proving the source operation happened before this message.
    pub causal_proof: VectorClock,
    /// Optional cryptographic signature from the source shard's signing key.
    /// When present, the router verifies this signature before processing
    /// the cross-shard message. When None, the message is rejected in
    /// production (accepted only in test builds for backward compat).
    #[serde(default)]
    pub source_signature: Option<Vec<u8>>,
}

impl CrossShardMessage {
    /// Create a new cross-shard message.
    pub fn new(source_shard: ShardId, target_shard: ShardId, payload: Vec<u8>, causal_proof: VectorClock) -> Self {
        Self {
            source_shard,
            target_shard,
            payload,
            causal_proof,
            // P0-6 fix: default to unsigned. Production callers must call
            // `sign` (or set this field directly) to attach an
            // Ed25519 signature over the payload before sending; otherwise
            // the receiving router will reject the message.
            source_signature: None,
        }
    }

    /// Sign this cross-shard message with the given Ed25519 keypair.
    ///
    /// Signs over `payload || causal_proof.to_bytes()` — the same data
    /// that `verify_source_signature` checks. The signature is stored
    /// in `source_signature`.
    pub fn sign(&mut self, keypair: &omnia_substrate::crypto::NodeKeypair) {
        use ed25519_dalek::{Signer, SigningKey};

        let mut signed_data = Vec::new();
        signed_data.extend_from_slice(&self.payload);
        if let Ok(proof_bytes) = self.causal_proof.to_bytes() {
            signed_data.extend_from_slice(&proof_bytes);
        }

        // NodeKeypair is ed25519_dalek::SigningKey
        let sig: ed25519_dalek::Signature = keypair.sign(&signed_data);
        self.source_signature = Some(sig.to_bytes().to_vec());
    }

    /// Verify that the source event causally precedes this message.
    ///
    /// Returns `true` if `source_vc` (the vector clock at the source shard
    /// when the originating operation was applied) happened before the
    /// `target_vc` (the current vector clock at the target shard).
    pub fn verify_causality(&self, source_vc: &VectorClock, target_vc: &VectorClock) -> bool {
        source_vc.happened_before(target_vc)
    }

    /// Verify the source signature on this cross-shard message.
    /// Returns true if the signature is valid, false otherwise.
    /// Returns false if no signature is present.
    pub fn verify_source_signature(&self, source_pubkey: &[u8; 32]) -> bool {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let sig = match &self.source_signature {
            Some(s) if s.len() == 64 => match Signature::from_slice(s) {
                Ok(sig) => sig,
                Err(_) => return false,
            },
            _ => return false,
        };
        let pk = match VerifyingKey::from_bytes(source_pubkey) {
            Ok(pk) => pk,
            Err(_) => return false,
        };
        // NEW-C1 fix: sign over BOTH payload AND causal_proof (not just payload).
        // The previous code only signed self.payload, allowing an attacker to
        // swap causal_proof without invalidating the signature.
        let mut signed_data = Vec::new();
        signed_data.extend_from_slice(&self.payload);
        if let Ok(proof_bytes) = self.causal_proof.to_bytes() {
            signed_data.extend_from_slice(&proof_bytes);
        }
        pk.verify(&signed_data, &sig).is_ok()
    }
}

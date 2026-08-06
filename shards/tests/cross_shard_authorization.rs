#![allow(clippy::unwrap_used)]
//! AUDIT-2026-07 C4 (#342) regression tests: cross-shard messages must be
//! AUTHORIZED by the source shard's registered validator-set key, not just
//! authenticated by whoever created the carrying event.
//!
//! The vulnerability: `route_cross_shard` verified `source_signature`
//! against `event.creator_pubkey` — the creator of the carrying event, who
//! can be any authenticated user. So any user could forge a message "from"
//! any source shard by signing it with their own key. These tests lock the
//! forgery out and confirm the legitimate (registered-key) path still works.

use omnia_shards::{CrossShardMessage, FinancialShard, IdentityShard, ShardId, ShardOp, ShardPayload, ShardRouter};
use omnia_substrate::{crypto::generate_keypair, Event, NodeId, NodeKeypair, VectorClock};

fn test_node(id: u8) -> NodeId {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

/// A valid inner operation for the identity source shard.
fn identity_create_did_op(keypair: &NodeKeypair) -> Vec<u8> {
    postcard::to_allocvec(&ShardOp::Identity(omnia_shards::IdentityOp::CreateDid {
        document: omnia_shards::DidDocument::new(
            format!("did:omnia:{}", hex::encode(keypair.verifying_key().to_bytes())),
            keypair.verifying_key().to_bytes(),
            0,
        ),
    }))
    .unwrap()
}

/// Build a router with the financial + identity shards and route a
/// cross-shard message (source = identity) inside an event created by
/// `event_creator`. `configure` runs after registration so a test can
/// register (or not) the source shard's attestation key.
fn route_message(
    msg: CrossShardMessage,
    event_creator: &NodeKeypair,
    configure: impl FnOnce(&mut ShardRouter),
) -> Result<(), omnia_shards::ShardError> {
    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(FinancialShard::new()));
    router.register(Box::new(IdentityShard::new()));
    configure(&mut router);

    // Event vc dominates the message's causal_proof (test_node(2), 1) so the
    // causality check passes and routing reaches the authorization check.
    let mut vc = VectorClock::with_node(test_node(1), 1);
    vc.merge(&VectorClock::with_node(test_node(2), 1));
    let payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::CrossShard(msg),
        nonce: 1,
    }
    .to_bytes()
    .unwrap();
    let mut event = Event::new(test_node(1), 0, vc, None, None, payload).unwrap();
    event.sign_with_keypair(event_creator).unwrap();
    router.route_event(&event)
}

fn base_message(source_op: Vec<u8>) -> CrossShardMessage {
    CrossShardMessage::new(
        ShardId::identity(),
        ShardId::identity(),
        source_op,
        [0u8; 32],
        [0u8; 32],
        VectorClock::with_node(test_node(2), 1),
    )
}

/// THE C4 regression: an attacker who is a normal authenticated user
/// attests a message "from" the identity shard with their OWN key. The
/// identity shard's registered attestation key is a DIFFERENT key, so the
/// forgery is rejected. On the old code this was accepted because the
/// signature was checked against the attacker's own event-creator key.
#[test]
fn forged_message_signed_by_event_creator_is_rejected() {
    let attacker = generate_keypair();
    let legit_shard_key = generate_keypair();

    let mut msg = base_message(identity_create_did_op(&attacker));
    msg.attest(&attacker); // attacker signs with their own key

    let result = route_message(msg, &attacker, |router| {
        // The real identity-shard key is registered — NOT the attacker's.
        router.register_shard_attestation_key(ShardId::identity(), legit_shard_key.verifying_key().to_bytes());
    });

    let err = result.expect_err("a message not attested by the registered key must be rejected");
    assert!(
        err.to_string().contains("does not verify against the"),
        "rejection must be the attestation check, got: {err}"
    );
}

/// If the source shard has no registered attestation key at all, the
/// message is rejected fail-closed — an unregistered shard cannot
/// originate cross-shard messages.
#[test]
fn message_from_unregistered_source_shard_is_rejected() {
    let attacker = generate_keypair();
    let mut msg = base_message(identity_create_did_op(&attacker));
    msg.attest(&attacker);

    let result = route_message(msg, &attacker, |_router| {
        // Deliberately register nothing.
    });
    let err = result.expect_err("unregistered source shard must be rejected");
    assert!(
        err.to_string().contains("no registered attestation key"),
        "rejection must be the missing-key check, got: {err}"
    );
}

/// A message with no attestation at all is rejected even when the source
/// shard key is registered.
#[test]
fn unattested_message_is_rejected() {
    let shard_key = generate_keypair();
    let creator = generate_keypair();
    let msg = base_message(identity_create_did_op(&shard_key)); // never attested

    let result = route_message(msg, &creator, |router| {
        router.register_shard_attestation_key(ShardId::identity(), shard_key.verifying_key().to_bytes());
    });
    assert!(result.is_err(), "unattested cross-shard message must be rejected");
}

/// The legitimate path: the message is attested by the identity shard's
/// registered validator-set key. Authorization passes and the message is
/// routed to the target shard for processing — even though the carrying
/// event was created by a *different* key.
#[test]
fn message_attested_by_registered_key_is_authorized() {
    let shard_key = generate_keypair();
    let event_creator = generate_keypair();

    // The inner DID op is owned by the event creator (the identity shard
    // enforces owner == event.creator_pubkey downstream); the message is
    // ATTESTED by the source shard's registered key. Both facts hold at
    // once: authorization comes from the shard key, the op's own owner
    // check from the event creator.
    let mut msg = base_message(identity_create_did_op(&event_creator));
    msg.attest(&shard_key);

    let result = route_message(msg, &event_creator, |router| {
        router.register_shard_attestation_key(ShardId::identity(), shard_key.verifying_key().to_bytes());
    });
    assert!(
        result.is_ok(),
        "a message attested by the registered source-shard key must be authorized, got: {result:?}"
    );
}

/// Tampering with the payload after attestation invalidates the
/// attestation (the commitment binds the payload).
#[test]
fn tampering_payload_after_attestation_is_rejected() {
    let shard_key = generate_keypair();

    let mut msg = base_message(identity_create_did_op(&shard_key));
    msg.attest(&shard_key);
    // Swap the payload for a different (still valid) op after signing.
    let other = generate_keypair();
    msg.payload = identity_create_did_op(&other);

    let result = route_message(msg, &shard_key, |router| {
        router.register_shard_attestation_key(ShardId::identity(), shard_key.verifying_key().to_bytes());
    });
    let err = result.expect_err("payload tampering must invalidate the attestation");
    assert!(err.to_string().contains("does not verify against the"), "got: {err}");
}

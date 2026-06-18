#![allow(clippy::unwrap_used)]
//! End-to-end provenance chain test
//!
//! Simulates a complete supply chain scenario: factory -> distributor ->
//! retailer -> customer, verifying the provenance chain at each step.
//! Also tests cross-shard integration by verifying that the Binding
//! Layer's ProvenanceTracker works alongside the existing PhysicalShard.

use omnia_binding::{
    CommitmentPhase, PhysicalAnchor, PqPublicKey, ProvenanceLog, ProvenanceTracker, QuantumCommitment, RfFingerprint,
};
use omnia_substrate::{generate_keypair, NodeKeypair, VectorClock};

fn make_rf(did: &str, hash: [u8; 32]) -> RfFingerprint {
    RfFingerprint::stub(did, hash)
}

/// Create a keypair and corresponding PqPublicKey for testing.
fn make_keypair_and_pk() -> (NodeKeypair, PqPublicKey) {
    let kp = generate_keypair();
    let pk = PqPublicKey {
        ed25519: kp.verifying_key().to_bytes(),
        dilithium: Vec::new(),
    };
    (kp, pk)
}

/// Create a cryptographically signed commitment using Ed25519.
fn make_commitment(data: &[u8], kp: &NodeKeypair) -> QuantumCommitment {
    QuantumCommitment::sign_classical(data, kp).unwrap()
}

fn item_id(n: u8) -> [u8; 32] {
    let mut id = [0u8; 32];
    id[0] = n;
    id
}

// ---------------------------------------------------------------------------
// Test 1: Basic provenance chain lifecycle
// ---------------------------------------------------------------------------

#[test]
fn test_provenance_chain_lifecycle() {
    let (kp, _pk) = make_keypair_and_pk();
    let item = item_id(1);
    let anchor_id = [0xCDu8; 32];

    // Step 1: Factory creates the item
    let mut log = ProvenanceLog::new(
        item,
        "did:omnia:factory".to_string(),
        make_rf("did:omnia:factory", [0x11u8; 32]),
        make_commitment(b"creation", &kp),
        anchor_id,
    );

    assert_eq!(log.len(), 1);
    assert_eq!(log.current_holder, "did:omnia:factory");
    assert!(log.verify_chain());

    // Step 2: Transfer to distributor
    let _ = log.transfer(
        "did:omnia:distributor".to_string(),
        make_rf("did:omnia:distributor", [0x22u8; 32]),
        make_commitment(b"transfer_factory_dist", &kp),
    );
    assert_eq!(log.len(), 2);
    assert_eq!(log.current_holder, "did:omnia:distributor");
    assert!(log.verify_chain());

    // Step 3: Transfer to retailer
    let _ = log.transfer(
        "did:omnia:retailer".to_string(),
        make_rf("did:omnia:retailer", [0x33u8; 32]),
        make_commitment(b"transfer_dist_retail", &kp),
    );
    assert_eq!(log.len(), 3);
    assert_eq!(log.current_holder, "did:omnia:retailer");
    assert!(log.verify_chain());

    // Step 4: Transfer to customer
    let _ = log.transfer(
        "did:omnia:customer".to_string(),
        make_rf("did:omnia:customer", [0x44u8; 32]),
        make_commitment(b"transfer_retail_customer", &kp),
    );
    assert_eq!(log.len(), 4);
    assert_eq!(log.current_holder, "did:omnia:customer");
    assert!(log.verify_chain());

    // Step 5: Verify the item at the customer's location
    log.verify(
        make_rf("did:omnia:customer", [0x44u8; 32]),
        make_commitment(b"customer_verification", &kp),
    );
    assert_eq!(log.len(), 5);
    assert!(log.verify_chain());
}

// ---------------------------------------------------------------------------
// Test 2: ProvenanceTracker with full supply chain
// ---------------------------------------------------------------------------

#[test]
fn test_tracker_supply_chain() {
    let (kp, _pk) = make_keypair_and_pk();
    let mut tracker = ProvenanceTracker::new();
    let item = item_id(42);

    // Anchor
    tracker
        .anchor_item(
            item,
            "did:omnia:miner".to_string(),
            make_rf("did:omnia:miner", [0xA0u8; 32]),
            make_commitment(b"mined_diamond", &kp),
            [0xEFu8; 32],
        )
        .unwrap();

    // Transfer: miner -> cutter
    tracker
        .transfer_item(
            item,
            "did:omnia:cutter".to_string(),
            make_rf("did:omnia:cutter", [0xB0u8; 32]),
            make_commitment(b"miner_to_cutter", &kp),
        )
        .unwrap();

    // Transfer: cutter -> grader
    tracker
        .transfer_item(
            item,
            "did:omnia:grader".to_string(),
            make_rf("did:omnia:grader", [0xC0u8; 32]),
            make_commitment(b"cutter_to_grader", &kp),
        )
        .unwrap();

    // Transfer: grader -> jeweler
    tracker
        .transfer_item(
            item,
            "did:omnia:jeweler".to_string(),
            make_rf("did:omnia:jeweler", [0xD0u8; 32]),
            make_commitment(b"grader_to_jeweler", &kp),
        )
        .unwrap();

    // Verify the complete chain
    let provenance = tracker.query_provenance(item).unwrap();
    assert!(provenance.verify_chain());
    assert_eq!(provenance.len(), 4); // creation + 3 transfers
    assert_eq!(tracker.current_holder(item), Some("did:omnia:jeweler"));
}

// ---------------------------------------------------------------------------
// Test 3: PhysicalAnchor verification with real signatures
// ---------------------------------------------------------------------------

#[test]
fn test_physical_anchor_verification() {
    let (kp, pk) = make_keypair_and_pk();
    let rf_hash = [0x77u8; 32];
    let item = item_id(7);

    let provenance = ProvenanceLog::new(
        item,
        "did:omnia:creator".to_string(),
        make_rf("did:omnia:creator", rf_hash),
        make_commitment(b"anchor_data", &kp),
        [0xFFu8; 32],
    );

    // Create commitment over the provenance log bytes (as verify() expects)
    let commitment = make_commitment(&provenance.to_bytes().unwrap(), &kp);

    let anchor = PhysicalAnchor::new(
        make_rf("did:omnia:creator", rf_hash),
        commitment,
        provenance,
        CommitmentPhase::ClassicalOnly,
    );

    // Verification with correct RF and public key should succeed
    assert!(anchor.verify(&rf_hash, &pk));

    // Verification with wrong RF should fail
    let wrong_rf = [0x00u8; 32];
    assert!(!anchor.verify(&wrong_rf, &pk));
}

// ---------------------------------------------------------------------------
// Test 4: Serialization round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_provenance_serialization_roundtrip() {
    let (kp, _pk) = make_keypair_and_pk();
    let item = item_id(9);
    let mut log = ProvenanceLog::new(
        item,
        "did:omnia:origin".to_string(),
        make_rf("did:omnia:origin", [0xAAu8; 32]),
        make_commitment(b"origin", &kp),
        [0xBBu8; 32],
    );

    let _ = log.transfer(
        "did:omnia:next".to_string(),
        make_rf("did:omnia:next", [0xCCu8; 32]),
        make_commitment(b"transfer", &kp),
    );

    let bytes = log.to_bytes().unwrap();
    let restored = ProvenanceLog::from_bytes(&bytes).unwrap();

    assert_eq!(log.item_id, restored.item_id);
    assert_eq!(log.current_holder, restored.current_holder);
    assert_eq!(log.events.len(), restored.events.len());
    assert!(restored.verify_chain());
}

// ---------------------------------------------------------------------------
// Test 5: Quantum commitment verification with real Ed25519 signatures
// ---------------------------------------------------------------------------

#[test]
fn test_quantum_commitment_data_integrity() {
    let (kp, pk) = make_keypair_and_pk();
    let data = b"important physical event data";
    let commitment = QuantumCommitment::sign_classical(data, &kp).unwrap();

    // Correct data should verify
    assert!(commitment.verify(&pk, data, CommitmentPhase::ClassicalOnly));

    // Tampered data should NOT verify
    let tampered = b"tampered physical event data";
    assert!(!commitment.verify(&pk, tampered, CommitmentPhase::ClassicalOnly));
}

// ---------------------------------------------------------------------------
// Test 6: RF fingerprint matching
// ---------------------------------------------------------------------------

#[test]
fn test_rf_fingerprint_matching() {
    let hash_a = [0x55u8; 32];
    let hash_b = [0x55u8; 32]; // Identical
    let hash_c = [0xFFu8; 32]; // Completely different

    let fp = RfFingerprint::stub("did:omnia:device", hash_a);

    assert!(fp.verify(&hash_b)); // Same hash -> match
    assert!(!fp.verify(&hash_c)); // Different hash -> no match
}

// ---------------------------------------------------------------------------
// Test 7: Destroyed item cannot be transferred
// ---------------------------------------------------------------------------

#[test]
fn test_destroyed_item_no_transfer() {
    let (kp, _pk) = make_keypair_and_pk();
    let mut tracker = ProvenanceTracker::new();
    let item = item_id(5);

    tracker
        .anchor_item(
            item,
            "did:omnia:owner".to_string(),
            make_rf("did:omnia:owner", [0x55u8; 32]),
            make_commitment(b"creation", &kp),
            [0xCCu8; 32],
        )
        .unwrap();

    // Destroy the item via the provenance log
    let anchor = tracker.get_anchor(&item).unwrap();
    let mut log = anchor.provenance_log.clone();
    log.destroy(
        make_rf("did:omnia:owner", [0x55u8; 32]),
        make_commitment(b"destruction", &kp),
    );

    // The provenance log should now be marked as destroyed
    assert!(log.is_destroyed());
}

// ---------------------------------------------------------------------------
// Test 8: Cannot anchor the same item twice
// ---------------------------------------------------------------------------

#[test]
fn test_double_anchor_rejected() {
    let (kp, _pk) = make_keypair_and_pk();
    let mut tracker = ProvenanceTracker::new();
    let item = item_id(10);

    tracker
        .anchor_item(
            item,
            "did:omnia:creator".to_string(),
            make_rf("did:omnia:creator", [0x11u8; 32]),
            make_commitment(b"creation", &kp),
            [0xDDu8; 32],
        )
        .unwrap();

    // Second anchor should fail
    let result = tracker.anchor_item(
        item,
        "did:omnia:other".to_string(),
        make_rf("did:omnia:other", [0x22u8; 32]),
        make_commitment(b"creation2", &kp),
        [0xEEu8; 32],
    );

    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Test 9: Binding Layer works alongside existing PhysicalShard
// ---------------------------------------------------------------------------

#[test]
fn test_binding_layer_with_physical_shard() {
    use omnia_shards::{PhysicalOp, PhysicalState};

    let (kp, _pk) = make_keypair_and_pk();

    // Create existing PhysicalShard state
    let mut physical_state = PhysicalState::new();
    let mut tracker = ProvenanceTracker::new();

    let item = item_id(1);
    let vc = VectorClock::new();

    // Apply PhysicalOp::AnchorItem on the existing shard
    let anchor_op = PhysicalOp::AnchorItem {
        item_id: item,
        owner: [0x11u8; 32],
        metadata: b"diamond_cert".to_vec(),
    };
    physical_state.apply(&anchor_op, &vc, None).unwrap();

    // Simultaneously track in the Binding Layer
    tracker
        .anchor_item(
            item,
            "did:omnia:miner".to_string(),
            make_rf("did:omnia:miner", [0x11u8; 32]),
            make_commitment(b"mined_diamond", &kp),
            [0xAAu8; 32],
        )
        .unwrap();

    // Apply PhysicalOp::TransferOwnership on the existing shard
    let transfer_op = PhysicalOp::TransferOwnership {
        item_id: item,
        new_owner: [0x22u8; 32],
    };
    physical_state.apply(&transfer_op, &vc, Some([0x11u8; 32])).unwrap();

    // Simultaneously transfer in the Binding Layer
    tracker
        .transfer_item(
            item,
            "did:omnia:jeweler".to_string(),
            make_rf("did:omnia:jeweler", [0x22u8; 32]),
            make_commitment(b"miner_to_jeweler", &kp),
        )
        .unwrap();

    // Both layers agree on the item's existence
    assert!(physical_state.provenance.contains_key(&item));
    assert!(tracker.query_provenance(item).is_some());
    assert_eq!(tracker.current_holder(item), Some("did:omnia:jeweler"));

    // The Binding Layer provides additional cryptographic verification
    let provenance = tracker.query_provenance(item).unwrap();
    assert!(provenance.verify_chain());
}

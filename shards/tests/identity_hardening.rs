//! Integration test: Layer 4 Identity Hardening
//!
//! Tests the full lifecycle of the hardened identity system:
//! - Shamir's Secret Sharing for social recovery
//! - Privacy-preserving biometric anchors
//! - AI agent identity with capability-based access control

use omnia_shards::{
    AgentCapability, AgentIdentity, BiometricAnchor, DidDocument, IdentityOp, IdentityState,
    ShamirRecovery,
};
use omnia_substrate::{crypto::generate_keypair, VectorClock};

/// Helper: create a VectorClock with a single node at counter 1.
fn test_vc() -> VectorClock {
    let kp = generate_keypair();
    VectorClock::with_node(kp.verifying_key().to_bytes(), 1)
}

#[test]
fn test_shamir_split_and_reconstruct() {
    let secret = b"my super secret key";
    let shares = ShamirRecovery::split(secret, 3, 5);
    assert_eq!(shares.len(), 5);

    // Reconstruct with exactly threshold shares
    let reconstructed = ShamirRecovery::reconstruct(&shares[0..3]).unwrap();
    assert_eq!(reconstructed, secret);

    // Reconstruct with a different set of threshold shares
    let reconstructed = ShamirRecovery::reconstruct(&shares[1..4]).unwrap();
    assert_eq!(reconstructed, secret);

    // Reconstruct with all shares
    let reconstructed = ShamirRecovery::reconstruct(&shares).unwrap();
    assert_eq!(reconstructed, secret);

    // Reconstruct with fewer than threshold shares should return wrong data
    let result = ShamirRecovery::reconstruct(&shares[0..2]);
    assert!(result.is_some());
    assert_ne!(result.unwrap(), secret);
}

#[test]
fn test_biometric_enroll_and_verify() {
    let template = b"fingerprint_template_bytes";
    let anchor = BiometricAnchor::enroll(template, "fingerprint_v2");

    // Same template should verify
    assert!(anchor.verify(template));

    // Different template should fail
    assert!(!anchor.verify(b"wrong_template"));
}

#[test]
fn test_agent_capability_check() {
    let agent = AgentIdentity {
        did: "did:omnia:agent1".to_string(),
        owner_did: "did:omnia:human1".to_string(),
        capabilities: vec![
            AgentCapability::FinancialTransfer {
                max_amount: 1000,
                currency: "UBC".to_string(),
            },
            AgentCapability::DataProcessing {
                domains: vec!["health".to_string()],
                max_records: 100,
            },
        ],
        created_at: VectorClock::new(),
        expires_at: None,
        revoked: false,
    };

    assert!(agent.has_capability(&AgentCapability::FinancialTransfer {
        max_amount: 500,
        currency: "UBC".to_string(),
    }));
    assert!(!agent.has_capability(&AgentCapability::FinancialTransfer {
        max_amount: 2000,
        currency: "UBC".to_string(),
    }));
    assert!(!agent.has_capability(&AgentCapability::FinancialTransfer {
        max_amount: 500,
        currency: "ETH".to_string(),
    }));
}

#[test]
fn test_full_identity_lifecycle() {
    let mut state = IdentityState::new();
    let vc = test_vc();

    // 1. Create DID
    let keypair = generate_keypair();
    let pubkey = keypair.verifying_key().to_bytes();
    let did = format!("did:omnia:{}", hex::encode(pubkey));
    let doc = DidDocument::new(did.clone(), pubkey, 0);
    state
        .apply(&IdentityOp::CreateDid { document: doc }, &vc)
        .unwrap();

    // 2. Enroll biometric
    state
        .apply(
            &IdentityOp::EnrollBiometric {
                did: did.clone(),
                template: b"fingerprint_data".to_vec(),
                algorithm: "fingerprint_v2".to_string(),
            },
            &vc,
        )
        .unwrap();
    assert!(state.verify_biometric(&did, b"fingerprint_data").unwrap());

    // 3. Create recovery shares
    let secret = b"master_secret_key_32_bytes_long";
    let shares = state.create_recovery_shares(&did, secret, 3, 5).unwrap();
    assert_eq!(shares.len(), 5);

    // 4. Recover with 3 shares
    let recovered = state.recover_did(&did, &shares[0..3]).unwrap();
    assert_eq!(recovered, secret);

    // 5. Register AI agent
    let agent = AgentIdentity {
        did: "did:omnia:agent1".to_string(),
        owner_did: did.clone(),
        capabilities: vec![AgentCapability::FinancialTransfer {
            max_amount: 100,
            currency: "UBC".to_string(),
        }],
        created_at: VectorClock::new(),
        expires_at: None,
        revoked: false,
    };
    state.register_agent(agent).unwrap();

    // 6. Verify agent exists
    assert!(state.agent_registry.contains_key("did:omnia:agent1"));

    // 7. Revoke agent
    state
        .apply(
            &IdentityOp::RevokeAgent {
                agent_did: "did:omnia:agent1".to_string(),
            },
            &vc,
        )
        .unwrap();
    let agent = state.agent_registry.get("did:omnia:agent1").unwrap();
    assert!(agent.revoked);
}

#[test]
fn test_biometric_verification_via_apply() {
    let mut state = IdentityState::new();
    let vc = test_vc();

    let keypair = generate_keypair();
    let pubkey = keypair.verifying_key().to_bytes();
    let did = format!("did:omnia:{}", hex::encode(pubkey));
    let doc = DidDocument::new(did.clone(), pubkey, 0);
    state
        .apply(&IdentityOp::CreateDid { document: doc }, &vc)
        .unwrap();

    // Enroll
    state
        .apply(
            &IdentityOp::EnrollBiometric {
                did: did.clone(),
                template: b"iris_scan".to_vec(),
                algorithm: "iris_v3".to_string(),
            },
            &vc,
        )
        .unwrap();

    // Verify with correct template should succeed
    let result = state.apply(
        &IdentityOp::VerifyBiometric {
            did: did.clone(),
            template: b"iris_scan".to_vec(),
        },
        &vc,
    );
    assert!(result.is_ok());

    // Verify with wrong template should fail
    let result = state.apply(
        &IdentityOp::VerifyBiometric {
            did: did.clone(),
            template: b"wrong_scan".to_vec(),
        },
        &vc,
    );
    assert!(result.is_err());
}

#[test]
fn test_configure_recovery_via_apply() {
    let mut state = IdentityState::new();
    let vc = test_vc();

    let keypair = generate_keypair();
    let pubkey = keypair.verifying_key().to_bytes();
    let did = format!("did:omnia:{}", hex::encode(pubkey));
    let doc = DidDocument::new(did.clone(), pubkey, 0);
    state
        .apply(&IdentityOp::CreateDid { document: doc }, &vc)
        .unwrap();

    // Configure recovery
    state
        .apply(
            &IdentityOp::ConfigureRecovery {
                did: did.clone(),
                secret: b"my_secret_key".to_vec(),
                threshold: 3,
                total_shares: 5,
            },
            &vc,
        )
        .unwrap();

    // Recovery config should exist
    assert!(state.recovery_registry.contains_key(&did));
    let config = state.recovery_registry.get(&did).unwrap();
    assert_eq!(config.threshold, 3);
    assert_eq!(config.total_shares, 5);
}

#[test]
fn test_agent_revocation_disables_capabilities() {
    let mut state = IdentityState::new();
    let vc = test_vc();

    let keypair = generate_keypair();
    let pubkey = keypair.verifying_key().to_bytes();
    let owner_did = format!("did:omnia:{}", hex::encode(pubkey));
    let doc = DidDocument::new(owner_did.clone(), pubkey, 0);
    state
        .apply(&IdentityOp::CreateDid { document: doc }, &vc)
        .unwrap();

    let agent = AgentIdentity {
        did: "did:omnia:agent:compute1".to_string(),
        owner_did: owner_did.clone(),
        capabilities: vec![AgentCapability::ComputeProof {
            max_compute_units: 1000,
        }],
        created_at: VectorClock::new(),
        expires_at: None,
        revoked: false,
    };

    state
        .apply(
            &IdentityOp::AddAgent {
                did: owner_did.clone(),
                agent,
            },
            &vc,
        )
        .unwrap();

    // Agent should have capability before revocation
    let agent = state
        .agent_registry
        .get("did:omnia:agent:compute1")
        .unwrap();
    assert!(agent.has_capability(&AgentCapability::ComputeProof {
        max_compute_units: 500,
    }));

    // Revoke
    state
        .apply(
            &IdentityOp::RevokeAgent {
                agent_did: "did:omnia:agent:compute1".to_string(),
            },
            &vc,
        )
        .unwrap();

    // Agent should NOT have capability after revocation
    let agent = state
        .agent_registry
        .get("did:omnia:agent:compute1")
        .unwrap();
    assert!(!agent.has_capability(&AgentCapability::ComputeProof {
        max_compute_units: 500,
    }));
    assert!(agent.revoked);
}

#[test]
fn test_duplicate_agent_rejected() {
    let mut state = IdentityState::new();
    let vc = test_vc();

    let keypair = generate_keypair();
    let pubkey = keypair.verifying_key().to_bytes();
    let owner_did = format!("did:omnia:{}", hex::encode(pubkey));
    let doc = DidDocument::new(owner_did.clone(), pubkey, 0);
    state
        .apply(&IdentityOp::CreateDid { document: doc }, &vc)
        .unwrap();

    let agent1 = AgentIdentity {
        did: "did:omnia:agent:dup".to_string(),
        owner_did: owner_did.clone(),
        capabilities: vec![],
        created_at: VectorClock::new(),
        expires_at: None,
        revoked: false,
    };

    state
        .apply(
            &IdentityOp::AddAgent {
                did: owner_did.clone(),
                agent: agent1,
            },
            &vc,
        )
        .unwrap();

    // Adding same agent DID again should fail
    let agent2 = AgentIdentity {
        did: "did:omnia:agent:dup".to_string(),
        owner_did: owner_did.clone(),
        capabilities: vec![],
        created_at: VectorClock::new(),
        expires_at: None,
        revoked: false,
    };

    let result = state.apply(
        &IdentityOp::AddAgent {
            did: owner_did,
            agent: agent2,
        },
        &vc,
    );
    assert!(result.is_err());
}

#[test]
fn test_biometric_for_nonexistent_did_fails() {
    let mut state = IdentityState::new();
    let vc = test_vc();

    let result = state.apply(
        &IdentityOp::EnrollBiometric {
            did: "did:omnia:nonexistent".to_string(),
            template: b"template".to_vec(),
            algorithm: "face_v1".to_string(),
        },
        &vc,
    );
    assert!(result.is_err());
}

#[test]
fn test_shamir_higher_threshold() {
    let secret = b"a_32_byte_secret_key_for_testing!";
    let shares = ShamirRecovery::split(secret, 5, 10);
    assert_eq!(shares.len(), 10);

    // Any 5 shares should reconstruct
    let reconstructed = ShamirRecovery::reconstruct(&shares[2..7]).unwrap();
    assert_eq!(reconstructed, secret);

    // Any other 5 shares should also reconstruct
    let reconstructed = ShamirRecovery::reconstruct(&shares[0..5]).unwrap();
    assert_eq!(reconstructed, secret);

    // 4 shares should fail
    let result = ShamirRecovery::reconstruct(&shares[0..4]);
    assert!(result.is_some());
    assert_ne!(result.unwrap(), secret);
}

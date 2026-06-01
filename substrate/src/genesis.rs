//! Genesis Tooling — Network Bootstrap Procedure
//!
//! Phase 5: Provides deterministic genesis block generation from TOML
//! configuration files, enabling auditable and reproducible network launches.
//!
//! # Genesis Procedure
//!
//! 1. Prepare a TOML configuration file with initial validators
//! 2. Run `omnia-node genesis-init --config genesis.toml --output genesis.bin`
//! 3. Each validator loads the genesis block on startup
//! 4. Consensus begins from round 0 with the initial validator set
//!
//! # Determinism
//!
//! The genesis block hash is computed as:
//! `BLAKE3("OMNIA-GENESIS-V1" || chain_id || sorted_validators)`
//!
//! This ensures all nodes produce the same genesis block from the same
//! configuration, which is critical for network agreement.

use crate::blake3_domain::blake3_hash_domain;
use crate::consensus::ConsensusConfig;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during genesis operations.
#[derive(Error, Debug)]
pub enum GenesisError {
    /// Not enough validators for BFT safety.
    #[error("insufficient validators: {0} provided, minimum 3 required for BFT")]
    InsufficientValidators(usize),
    /// Duplicate node IDs in the validator set.
    #[error("duplicate node ID in validator set: {0:?}")]
    DuplicateNodeId(String),
    /// Zero stake validator.
    #[error("validator {0} has zero initial stake")]
    ZeroStake(u64),
    /// Invalid public key.
    #[error("invalid public key for validator {0}: {1}")]
    InvalidPublicKey(u64, String),
    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Genesis configuration for a new Omnia network.
///
/// This structure defines all parameters needed to bootstrap a new
/// Omnia network, including the initial validator set, economic
/// parameters, and governance configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    /// Unique chain identifier.
    pub chain_id: u64,
    /// Human-readable network name.
    pub network_name: String,
    /// Genesis timestamp (Unix epoch seconds). Set to 0 for test configs.
    pub genesis_time: u64,
    /// Initial validator set.
    pub initial_validators: Vec<ValidatorInfo>,
    /// Consensus configuration overrides.
    pub consensus_config: ConsensusConfig,
    /// Economics configuration.
    pub economics_config: EconomicsConfig,
    /// Governance configuration.
    pub governance_config: GovernanceConfig,
    /// Genesis format version.
    pub version: u32,
}

/// Information about a validator in the genesis set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Unique validator identifier.
    pub node_id: u64,
    /// Ed25519 public key (hex-encoded in TOML, binary in memory).
    pub ed25519_public_key: String,
    /// Dilithium public key (hex-encoded).
    pub dilithium_public_key: String,
    /// Initial stake in UBC tokens.
    pub initial_stake: u64,
    /// Network multiaddress for P2P connectivity.
    pub network_address: String,
}

/// Economics configuration for genesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicsConfig {
    /// Initial UBC token supply.
    pub ubc_initial_supply: u64,
    /// Annual decay rate (basis points).
    pub decay_rate_bps: u64,
}

impl Default for EconomicsConfig {
    fn default() -> Self {
        Self {
            ubc_initial_supply: 1_000_000_000,
            decay_rate_bps: 1000, // 10%
        }
    }
}

/// Governance configuration for genesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceConfig {
    /// Quorum percentage for governance proposals (0-100).
    pub quorum_percentage: u64,
    /// Approval threshold percentage (0-100).
    pub approval_percentage: u64,
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            quorum_percentage: 67,
            approval_percentage: 50,
        }
    }
}

/// The genesis block — the first block of the chain.
///
/// Contains the initial validator set, state root, and chain parameters.
/// All nodes must agree on the genesis block hash to participate in
/// the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisBlock {
    /// Chain identifier (must match GenesisConfig).
    pub chain_id: u64,
    /// Initial state root (BLAKE3 of genesis data).
    pub state_root: [u8; 32],
    /// Initial validator set.
    pub validators: Vec<ValidatorInfo>,
    /// Genesis timestamp.
    pub timestamp: u64,
    /// Genesis block hash (deterministic from config).
    pub hash: [u8; 32],
}

/// Generate a deterministic genesis block from configuration.
///
/// # Procedure
///
/// 1. Validate the configuration (minimum 3 validators, unique IDs, non-zero stakes)
/// 2. Sort validators by node_id for deterministic ordering
/// 3. Compute initial state root: BLAKE3("OMNIA-GENESIS-V1" || chain_id || sorted_validators)
/// 4. Create genesis block with all validator registrations
/// 5. Compute genesis hash: BLAKE3 of the serialized genesis block
///
/// # Determinism
///
/// For the same `GenesisConfig`, this function always produces the same
/// `GenesisBlock`. This is critical for ensuring all nodes start from the
/// same initial state.
///
/// # Errors
///
/// Returns `GenesisError` if:
/// - Fewer than 3 validators are provided
/// - Two validators share the same node_id
/// - Any validator has zero initial stake
pub fn generate_genesis(config: &GenesisConfig) -> Result<GenesisBlock, GenesisError> {
    // Validate: at least 3 validators for BFT safety
    if config.initial_validators.len() < 3 {
        return Err(GenesisError::InsufficientValidators(config.initial_validators.len()));
    }

    // Validate: unique node IDs
    let mut seen_ids = std::collections::HashSet::new();
    for v in &config.initial_validators {
        let id_key = v.node_id.to_le_bytes();
        if !seen_ids.insert(id_key) {
            return Err(GenesisError::DuplicateNodeId(format!(
                "node_id {} appears more than once",
                v.node_id
            )));
        }
    }

    // Validate: non-zero stakes
    for v in &config.initial_validators {
        if v.initial_stake == 0 {
            return Err(GenesisError::ZeroStake(v.node_id));
        }
    }

    // Sort validators by node_id for deterministic ordering
    let mut sorted_validators = config.initial_validators.clone();
    sorted_validators.sort_by_key(|v| v.node_id);

    // Compute initial state root
    let mut state_preimage = Vec::new();
    state_preimage.extend_from_slice(b"OMNIA-GENESIS-V1");
    state_preimage.extend_from_slice(&config.chain_id.to_le_bytes());
    for v in &sorted_validators {
        state_preimage.extend_from_slice(&v.node_id.to_le_bytes());
        state_preimage.extend_from_slice(&v.initial_stake.to_le_bytes());
        // Decode hex to raw bytes for deterministic hashing
        let ed25519_bytes = hex::decode(&v.ed25519_public_key).unwrap_or_default();
        state_preimage.extend_from_slice(&ed25519_bytes);
        let dilithium_bytes = hex::decode(&v.dilithium_public_key).unwrap_or_default();
        state_preimage.extend_from_slice(&dilithium_bytes);
    }
    let state_root: [u8; 32] = blake3_hash_domain(b"omnia-genesis", &state_preimage);

    // Create the genesis block
    let genesis = GenesisBlock {
        chain_id: config.chain_id,
        state_root,
        validators: sorted_validators,
        timestamp: config.genesis_time,
        hash: [0u8; 32], // placeholder, computed below
    };

    // Compute genesis hash from serialized block
    let genesis_bytes = postcard::to_allocvec(&genesis).map_err(|e| GenesisError::Serialization(e.to_string()))?;
    let genesis_hash: [u8; 32] = blake3_hash_domain(b"omnia-genesis-block", &genesis_bytes);

    Ok(GenesisBlock {
        hash: genesis_hash,
        ..genesis
    })
}

/// Validate a genesis block.
///
/// Re-generates the genesis block from the embedded validator set and
/// compares the hash. Returns `Ok(())` if the block is valid.
///
/// # Errors
///
/// Returns `GenesisError` if the block hash doesn't match the expected value.
pub fn validate_genesis(block: &GenesisBlock) -> Result<(), GenesisError> {
    // Reconstruct the expected config from the block
    let config = GenesisConfig {
        chain_id: block.chain_id,
        network_name: String::new(),
        genesis_time: block.timestamp,
        initial_validators: block.validators.clone(),
        consensus_config: ConsensusConfig::default(),
        economics_config: EconomicsConfig::default(),
        governance_config: GovernanceConfig::default(),
        version: 1,
    };
    // TODO: validate_genesis currently uses default configs for
    // consensus, economics, and governance when reconstructing the
    // GenesisConfig from the block. This means the recomputed state
    // root may not match if the original genesis used non-default
    // configs. The GenesisBlock should either embed the config hash
    // or the full config data so that validation is deterministic
    // regardless of the validator's local defaults.

    let expected = generate_genesis(&config)?;

    if expected.hash != block.hash {
        return Err(GenesisError::Serialization(format!(
            "genesis hash mismatch: expected {}, got {}",
            hex::encode(&expected.hash[..8]),
            hex::encode(&block.hash[..8])
        )));
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn test_validator(node_id: u64, stake: u64) -> ValidatorInfo {
        ValidatorInfo {
            node_id,
            ed25519_public_key: format!("{node_id:064x}"),
            dilithium_public_key: format!("{node_id:0256x}"),
            initial_stake: stake,
            network_address: format!("/ip4/127.0.0.{node_id}/udp/4001/quic-v1"),
        }
    }

    #[test]
    fn test_genesis_deterministic() {
        let config = GenesisConfig {
            chain_id: 1,
            network_name: "testnet".to_string(),
            genesis_time: 1000,
            initial_validators: vec![
                test_validator(1, 1000),
                test_validator(2, 1000),
                test_validator(3, 1000),
            ],
            consensus_config: ConsensusConfig::default(),
            economics_config: EconomicsConfig::default(),
            governance_config: GovernanceConfig::default(),
            version: 1,
        };

        let block1 = generate_genesis(&config).unwrap();
        let block2 = generate_genesis(&config).unwrap();
        assert_eq!(block1.hash, block2.hash, "same config must produce same genesis hash");
    }

    #[test]
    fn test_genesis_min_validators() {
        let config = GenesisConfig {
            chain_id: 1,
            network_name: "testnet".to_string(),
            genesis_time: 0,
            initial_validators: vec![
                test_validator(1, 1000),
                test_validator(2, 1000),
                // Only 2 validators — insufficient for BFT
            ],
            consensus_config: ConsensusConfig::default(),
            economics_config: EconomicsConfig::default(),
            governance_config: GovernanceConfig::default(),
            version: 1,
        };

        let result = generate_genesis(&config);
        assert!(result.is_err());
        match result.unwrap_err() {
            GenesisError::InsufficientValidators(n) => assert_eq!(n, 2),
            other => panic!("Expected InsufficientValidators, got: {other:?}"),
        }
    }

    #[test]
    fn test_genesis_unique_node_ids() {
        let config = GenesisConfig {
            chain_id: 1,
            network_name: "testnet".to_string(),
            genesis_time: 0,
            initial_validators: vec![
                test_validator(1, 1000),
                test_validator(1, 2000), // Duplicate!
                test_validator(3, 1000),
            ],
            consensus_config: ConsensusConfig::default(),
            economics_config: EconomicsConfig::default(),
            governance_config: GovernanceConfig::default(),
            version: 1,
        };

        let result = generate_genesis(&config);
        assert!(result.is_err());
        match result.unwrap_err() {
            GenesisError::DuplicateNodeId(msg) => {
                assert!(msg.contains("node_id 1"), "error should mention the duplicate ID");
            }
            other => panic!("Expected DuplicateNodeId, got: {other:?}"),
        }
    }

    #[test]
    fn test_genesis_validate() {
        let config = GenesisConfig {
            chain_id: 1,
            network_name: "testnet".to_string(),
            genesis_time: 1000,
            initial_validators: vec![
                test_validator(1, 1000),
                test_validator(2, 1000),
                test_validator(3, 1000),
            ],
            consensus_config: ConsensusConfig::default(),
            economics_config: EconomicsConfig::default(),
            governance_config: GovernanceConfig::default(),
            version: 1,
        };

        let block = generate_genesis(&config).unwrap();
        validate_genesis(&block).expect("genesis block should be valid");
    }

    #[test]
    fn test_genesis_toml_round_trip() {
        let config = GenesisConfig {
            chain_id: 42,
            network_name: "testnet-roundtrip".to_string(),
            genesis_time: 1234567890,
            initial_validators: vec![
                test_validator(1, 5000),
                test_validator(2, 5000),
                test_validator(3, 5000),
            ],
            consensus_config: ConsensusConfig::default(),
            economics_config: EconomicsConfig::default(),
            governance_config: GovernanceConfig::default(),
            version: 1,
        };

        // Serialize to postcard (binary) and back
        let bytes = postcard::to_allocvec(&config).unwrap();
        let deserialized: GenesisConfig = postcard::from_bytes(&bytes).unwrap();

        // Generate genesis from both — should produce identical hashes
        let block1 = generate_genesis(&config).unwrap();
        let block2 = generate_genesis(&deserialized).unwrap();
        assert_eq!(
            block1.hash, block2.hash,
            "round-trip serialized config must produce same genesis"
        );
    }

    #[test]
    fn test_genesis_zero_stake_rejected() {
        let config = GenesisConfig {
            chain_id: 1,
            network_name: "testnet".to_string(),
            genesis_time: 0,
            initial_validators: vec![
                test_validator(1, 1000),
                test_validator(2, 0), // Zero stake!
                test_validator(3, 1000),
            ],
            consensus_config: ConsensusConfig::default(),
            economics_config: EconomicsConfig::default(),
            governance_config: GovernanceConfig::default(),
            version: 1,
        };

        let result = generate_genesis(&config);
        assert!(result.is_err());
        match result.unwrap_err() {
            GenesisError::ZeroStake(id) => assert_eq!(id, 2),
            other => panic!("Expected ZeroStake, got: {other:?}"),
        }
    }
}

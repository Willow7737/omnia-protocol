//! PQC Key Rotation — Quantum-Resistant Key Lifecycle Management.
//!
//! This module manages the lifecycle of post-quantum cryptographic (PQC)
//! keys, including key generation, rotation scheduling, and transition
//! periods where both old and new keys are accepted.

use omnia_substrate::vector_clock::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A request to rotate a node's PQC key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqcKeyRotationRequest {
    /// The node requesting the key rotation.
    pub node: NodeId,
    /// The new PQC public key (Dilithium3, 1952 bytes).
    #[serde(with = "serde_bytes")]
    pub new_public_key: Vec<u8>,
    /// The block height at which the rotation was requested.
    pub requested_at_height: u64,
    /// The block height at which the new key becomes effective.
    pub effective_at_height: u64,
    /// A signature from the current (old) key authorizing the rotation.
    #[serde(with = "serde_bytes")]
    pub authorization_signature: Vec<u8>,
}

/// The state of a key rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RotationState {
    /// The rotation request has been submitted but the new key is not yet effective.
    Pending,
    /// The new key is effective. The old key is still accepted but deprecated.
    Effective,
    /// The old key has been fully deprecated. Only the new key is accepted.
    Deprecated,
}

/// Manager for PQC key rotation operations.
pub struct PqcKeyRotationManager {
    /// Number of blocks between request and effective height.
    transition_blocks: u64,
    /// Number of blocks between effective height and deprecation.
    deprecation_blocks: u64,
    /// Current key for each node (node → public key bytes).
    current_keys: HashMap<NodeId, Vec<u8>>,
    /// Pending rotation requests (node → request).
    pending_rotations: HashMap<NodeId, PqcKeyRotationRequest>,
    /// Rotation state for each node.
    rotation_states: HashMap<NodeId, RotationState>,
}

impl PqcKeyRotationManager {
    /// Create a new key rotation manager.
    pub fn new(transition_blocks: u64, deprecation_blocks: u64) -> Self {
        Self {
            transition_blocks,
            deprecation_blocks,
            current_keys: HashMap::new(),
            pending_rotations: HashMap::new(),
            rotation_states: HashMap::new(),
        }
    }

    /// Create a key rotation manager with default timing parameters.
    pub fn default_timing() -> Self {
        Self::new(1000, 5000)
    }

    /// Register a node's initial PQC public key.
    pub fn register_key(&mut self, node: NodeId, public_key: Vec<u8>) {
        self.current_keys.insert(node, public_key);
        self.rotation_states.insert(node, RotationState::Deprecated);
    }

    /// Submit a key rotation request.
    pub fn submit_rotation(&mut self, request: PqcKeyRotationRequest) -> Result<(), String> {
        if !self.current_keys.contains_key(&request.node) {
            return Err(format!("Node {:?} is not registered", &request.node[..4]));
        }

        if self.pending_rotations.contains_key(&request.node) {
            return Err(format!("Node {:?} already has a pending rotation", &request.node[..4]));
        }

        tracing::info!(
            node = ?&request.node[..4],
            effective_at = request.effective_at_height,
            "PQC key rotation request submitted"
        );

        let node = request.node;
        self.pending_rotations.insert(node, request);
        self.rotation_states.insert(node, RotationState::Pending);
        Ok(())
    }

    /// Process all pending rotations and advance their state.
    pub fn process_effective(&mut self, current_height: u64) -> usize {
        let mut advanced = 0;

        let nodes_to_activate: Vec<NodeId> = self
            .pending_rotations
            .iter()
            .filter(|(_, req)| current_height >= req.effective_at_height)
            .map(|(&node, _)| node)
            .collect();

        for node in nodes_to_activate {
            if let Some(request) = self.pending_rotations.remove(&node) {
                self.current_keys
                    .insert(node, request.new_public_key.clone());
                self.rotation_states.insert(node, RotationState::Effective);
                advanced += 1;

                tracing::info!(
                    node = ?&node[..4],
                    height = current_height,
                    "PQC key rotation became effective"
                );
            }
        }

        advanced
    }

    /// Check whether a key is currently in transition for a node.
    pub fn is_key_in_transition(&self, node: &NodeId) -> bool {
        matches!(
            self.rotation_states.get(node),
            Some(RotationState::Pending) | Some(RotationState::Effective)
        )
    }

    /// Get the current public key for a node.
    pub fn current_key(&self, node: &NodeId) -> Option<&Vec<u8>> {
        self.current_keys.get(node)
    }

    /// Get the pending rotation request for a node, if any.
    pub fn pending_rotation(&self, node: &NodeId) -> Option<&PqcKeyRotationRequest> {
        self.pending_rotations.get(node)
    }

    /// Get the rotation state for a node.
    pub fn rotation_state(&self, node: &NodeId) -> Option<RotationState> {
        self.rotation_states.get(node).copied()
    }

    /// Get the number of registered nodes.
    pub fn registered_count(&self) -> usize {
        self.current_keys.len()
    }

    /// Get the number of pending rotations.
    pub fn pending_count(&self) -> usize {
        self.pending_rotations.len()
    }

    /// Get the transition blocks parameter.
    pub fn transition_blocks(&self) -> u64 {
        self.transition_blocks
    }

    /// Get the deprecation blocks parameter.
    pub fn deprecation_blocks(&self) -> u64 {
        self.deprecation_blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    fn make_request(node: NodeId, current_height: u64, transition: u64) -> PqcKeyRotationRequest {
        PqcKeyRotationRequest {
            node,
            new_public_key: vec![2u8; 1952],
            requested_at_height: current_height,
            effective_at_height: current_height + transition,
            authorization_signature: vec![0u8; 64],
        }
    }

    #[test]
    fn test_register_and_get_key() {
        let mut mgr = PqcKeyRotationManager::default_timing();
        let n = node(1);
        let key = vec![1u8; 1952];
        mgr.register_key(n, key.clone());
        assert_eq!(mgr.current_key(&n), Some(&key));
        assert_eq!(mgr.registered_count(), 1);
    }

    #[test]
    fn test_submit_rotation() {
        let mut mgr = PqcKeyRotationManager::new(100, 500);
        let n = node(2);
        mgr.register_key(n, vec![1u8; 1952]);
        let request = make_request(n, 1000, 100);
        mgr.submit_rotation(request).unwrap();
        assert!(mgr.is_key_in_transition(&n));
        assert_eq!(mgr.rotation_state(&n), Some(RotationState::Pending));
        assert_eq!(mgr.pending_count(), 1);
    }

    #[test]
    fn test_submit_rotation_unregistered() {
        let mut mgr = PqcKeyRotationManager::default_timing();
        let n = node(99);
        let request = make_request(n, 1000, 100);
        let result = mgr.submit_rotation(request);
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_rotation_already_pending() {
        let mut mgr = PqcKeyRotationManager::new(100, 500);
        let n = node(3);
        mgr.register_key(n, vec![1u8; 1952]);
        let request1 = make_request(n, 1000, 100);
        mgr.submit_rotation(request1).unwrap();
        let request2 = make_request(n, 1001, 100);
        let result = mgr.submit_rotation(request2);
        assert!(result.is_err());
    }

    #[test]
    fn test_process_effective_rotation() {
        let mut mgr = PqcKeyRotationManager::new(100, 500);
        let n = node(4);
        mgr.register_key(n, vec![1u8; 1952]);
        let request = make_request(n, 1000, 100);
        mgr.submit_rotation(request).unwrap();

        let advanced = mgr.process_effective(1050);
        assert_eq!(advanced, 0);
        assert_eq!(mgr.rotation_state(&n), Some(RotationState::Pending));
        assert_eq!(mgr.current_key(&n), Some(&vec![1u8; 1952]));

        let advanced = mgr.process_effective(1100);
        assert_eq!(advanced, 1);
        assert_eq!(mgr.rotation_state(&n), Some(RotationState::Effective));
        assert_eq!(mgr.current_key(&n), Some(&vec![2u8; 1952]));
        assert_eq!(mgr.pending_count(), 0);
    }

    #[test]
    fn test_key_in_transition() {
        let mut mgr = PqcKeyRotationManager::new(100, 500);
        let n = node(5);
        mgr.register_key(n, vec![1u8; 1952]);
        assert!(!mgr.is_key_in_transition(&n));
        let request = make_request(n, 1000, 100);
        mgr.submit_rotation(request).unwrap();
        assert!(mgr.is_key_in_transition(&n));
        mgr.process_effective(1100);
        assert!(mgr.is_key_in_transition(&n));
    }

    #[test]
    fn test_default_timing() {
        let mgr = PqcKeyRotationManager::default_timing();
        assert_eq!(mgr.transition_blocks(), 1000);
        assert_eq!(mgr.deprecation_blocks(), 5000);
    }

    #[test]
    fn test_unregistered_node() {
        let mgr = PqcKeyRotationManager::default_timing();
        let n = node(99);
        assert!(mgr.current_key(&n).is_none());
        assert!(mgr.pending_rotation(&n).is_none());
        assert!(mgr.rotation_state(&n).is_none());
        assert!(!mgr.is_key_in_transition(&n));
    }
}

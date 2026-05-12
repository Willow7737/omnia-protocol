//! ZK circuit for proving valid L2 state transitions.
//!
//! The circuit is L1-agnostic: it proves that a batch of events was
//! processed correctly, regardless of where the proof is verified.
//!
//! Phase 0: Stub. Full implementation requires arkworks R1CS gadgets.

use omnia_substrate::Event;

/// ZK circuit for proving valid L2 state transitions.
///
/// The circuit takes an old state root, a new state root, and a list of
/// events, and produces a proof that applying the events to the old state
/// produces the new state. The proof can be verified on any L1 that
/// supports the chosen proof system (Groth16, PLONK, STARK).
///
/// Phase 0 uses a hash-chain stub instead of a real ZK proof.
pub struct RollupCircuit {
    /// The state root before the batch was applied.
    pub old_state_root: [u8; 32],
    /// The state root after the batch was applied.
    pub new_state_root: [u8; 32],
    /// The events in the batch.
    pub events: Vec<Event>,
}

impl RollupCircuit {
    /// Create a new rollup circuit with the given state roots and events.
    pub fn new(old_state_root: [u8; 32], new_state_root: [u8; 32], events: Vec<Event>) -> Self {
        Self {
            old_state_root,
            new_state_root,
            events,
        }
    }

    /// Phase 0 stub: computes a hash chain instead of a real ZK proof.
    ///
    /// This provides a deterministic commitment to the state transition
    /// without the overhead of actual zero-knowledge proof generation.
    /// Production will use `ark_groth16::create_random_proof()` with
    /// a trusted setup.
    pub fn prove_stub(&self) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.old_state_root);
        for event in &self.events {
            hasher.update(&event.to_bytes());
        }
        hasher.update(&self.new_state_root);
        hasher.finalize().as_bytes().to_vec()
    }

    /// Phase 0 stub: verifies the hash chain.
    ///
    /// Production will use `ark_groth16::verify_proof()` with the
    /// verifying key.
    pub fn verify_stub(&self, proof: &[u8]) -> bool {
        let expected = self.prove_stub();
        proof == expected.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_substrate::crypto::generate_keypair;

    fn test_node(id: u8) -> [u8; 32] {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    #[test]
    fn test_circuit_prove_and_verify() {
        let creator = test_node(1);
        let keypair = generate_keypair();
        let mut event = Event::genesis(creator, vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);

        let circuit = RollupCircuit::new([0u8; 32], [1u8; 32], vec![event]);
        let proof = circuit.prove_stub();

        assert!(circuit.verify_stub(&proof));
        assert!(!circuit.verify_stub(&[0u8; 32])); // Wrong proof
    }

    #[test]
    fn test_circuit_empty_events() {
        let circuit = RollupCircuit::new([0u8; 32], [0u8; 32], vec![]);
        let proof = circuit.prove_stub();

        assert!(circuit.verify_stub(&proof));
    }
}

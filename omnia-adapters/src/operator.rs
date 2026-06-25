//! L2 batch operator — L1-agnostic.
//!
//! The [`RollupOperator`] collects finalized events from the causal graph,
//! builds batches, generates Groth16 proofs, and posts them to the
//! configured settlement layer. The operator is completely L1-agnostic —
//! it works with any [`SettlementLayer`] implementor.

use crate::circuit::ExpandedRollupCircuit;
use crate::prover::{self, ProverError, ProvingKey, VerifyingKey};
use crate::settlement::{SettlementError, SettlementLayer};
use omnia_consensus::CausalGraph;
use omnia_primitives::Event;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Errors that can occur during rollup operations.
#[derive(Debug, Error)]
pub enum RollupError {
    /// An error from the settlement layer.
    #[error("Settlement error: {0}")]
    Settlement(#[from] SettlementError),
    /// A serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),
    /// An error from the ZK prover.
    #[error("Prover error: {0}")]
    Prover(#[from] ProverError),
}

/// Cached Groth16 trusted setup keys.
struct TrustedSetupCache {
    proving_key: ProvingKey,
    verifying_key: VerifyingKey,
}

/// L2 batch operator. Collects finalized events, builds batches,
/// generates Groth16 proofs, and posts to the configured settlement layer.
///
/// The operator is L1-agnostic — it works with any [`SettlementLayer`]
/// implementor. Swapping Ethereum for Bitcoin or Solana requires only
/// changing the adapter passed to [`RollupOperator::new`].
pub struct RollupOperator {
    graph: Arc<RwLock<CausalGraph>>,
    settlement: Box<dyn SettlementLayer>,
    batch_size: usize,
    last_batched_index: usize,
    /// Cached trusted setup keys, initialized on first use.
    setup_cache: Option<TrustedSetupCache>,
}

impl RollupOperator {
    /// Create a new rollup operator.
    ///
    /// # Arguments
    /// * `graph` — The causal graph (wrapped in `Arc<RwLock<>>` for async access)
    /// * `settlement` — The L1 settlement adapter (any `SettlementLayer` implementor)
    /// * `batch_size` — Maximum number of events per batch
    pub fn new(graph: Arc<RwLock<CausalGraph>>, settlement: Box<dyn SettlementLayer>, batch_size: usize) -> Self {
        Self {
            graph,
            settlement,
            batch_size,
            last_batched_index: 0,
            setup_cache: None,
        }
    }

    /// Run one batch cycle: collect → build → prove → post.
    pub async fn run_batch(&mut self) -> Result<(), RollupError> {
        // 1. Collect finalized events
        let events = self.collect_events().await?;
        if events.is_empty() {
            tracing::info!("No new events to batch");
            return Ok(());
        }

        // 2. Compute old_root and apply events within the same write lock
        //    to prevent TOCTOU race condition (old_root must be read while
        //    holding the write lock to guarantee atomicity).
        let mut graph = self.graph.write().await;
        let old_root = graph.state_root();
        // Apply events to compute new state root
        for event in &events {
            graph
                .insert(event.clone())
                .map_err(|e| RollupError::Serialization(format!("Failed to insert event into causal graph: {e}")))?;
        }
        let new_root = graph.state_root();
        drop(graph);
        if old_root == new_root {
            tracing::warn!("operator: old_root == new_root after batch application — state may not have changed");
        }

        // 3. Build batch data
        let batch_data = self.build_batch_data(&events)?;

        // 5. Generate real Groth16 proof
        let proof_bytes = self.generate_proof(&old_root, &new_root, events.len())?;

        tracing::info!(
            "[{}] Generated Groth16 proof: {} bytes",
            self.settlement.chain_id(),
            proof_bytes.len()
        );

        // 6. Post to settlement layer
        let tx_ref = self.settlement.post_batch(&batch_data).await?;
        tracing::info!(
            "[{}] Batch posted: {} events, tx: {}",
            self.settlement.chain_id(),
            events.len(),
            &tx_ref[..tx_ref.len().min(16)]
        );

        // 7. Verify proof on L1 (for monitoring)
        let valid = self.settlement.verify_proof(&old_root, &new_root, &proof_bytes).await?;
        if !valid {
            tracing::warn!("[{}] Proof verification returned false", self.settlement.chain_id());
        }

        self.last_batched_index += events.len();
        Ok(())
    }

    /// Collect events from the causal graph that haven't been batched yet.
    async fn collect_events(&self) -> Result<Vec<Event>, RollupError> {
        let graph = self.graph.read().await;
        let events = Self::collect_from_graph(&graph, self.last_batched_index, self.batch_size);
        Ok(events)
    }

    /// Extract events from a causal graph starting at a given index.
    fn collect_from_graph(graph: &CausalGraph, start_index: usize, limit: usize) -> Vec<Event> {
        let all_ids = graph.event_ids();
        all_ids
            .iter()
            .skip(start_index)
            .take(limit)
            .filter_map(|id| graph.get(id).cloned())
            .collect()
    }

    /// Serialize events into batch data.
    fn build_batch_data(&self, events: &[Event]) -> Result<Vec<u8>, RollupError> {
        postcard::to_allocvec(events).map_err(|e| RollupError::Serialization(e.to_string()))
    }

    /// Generate a Groth16 proof for a state transition.
    ///
    /// Creates or retrieves the cached trusted setup, builds the circuit,
    /// creates the proof, and serializes it to bytes.
    fn generate_proof(
        &mut self,
        _old_root: &[u8; 32],
        _new_root: &[u8; 32],
        event_count: usize,
    ) -> Result<Vec<u8>, RollupError> {
        // Initialize trusted setup on first use.
        // Use ExpandedRollupCircuit for the setup since it provides stronger
        // security guarantees (Merkle path verification, state transition constraints).
        let merkle_depth = 8; // Default Merkle tree depth for setup
        if self.setup_cache.is_none() {
            tracing::info!("Generating trusted setup for ExpandedRollupCircuit (first use)");
            let (pk, vk) = prover::generate_trusted_setup_expanded(event_count.max(1), merkle_depth)?;
            tracing::info!("Trusted setup complete");
            self.setup_cache = Some(TrustedSetupCache {
                proving_key: pk,
                verifying_key: vk,
            });
        }

        // Build ExpandedRollupCircuit from actual state roots.
        //
        // P0-3 fix: from_state_roots() uses placeholder zero witnesses for
        // all per-event constraints (Merkle paths, state transitions, etc.).
        // For event_count > 0, the circuit is UNSATISFIABLE — proof creation
        // will fail. We guard against this here and return a clear error
        // instead of producing a proof that can never succeed.
        //
        // TODO: Wire ExpandedRollupCircuit::from_batch() with real Merkle
        // proofs (from build_poseidon_merkle_tree) and intermediate state
        // roots. This requires the operator to track per-event witness data
        // during batch accumulation. See circuit.rs:589-687 for the
        // constructor and circuit.rs:420-496 for from_batch().
        if event_count > 0 {
            tracing::error!(
                event_count = event_count,
                "ZK rollup: non-empty batch proof not yet implemented — \
                 ExpandedRollupCircuit::from_state_roots uses placeholder \
                 zero witnesses that make the circuit unsatisfiable. \
                 Wire ExpandedRollupCircuit::from_batch with real witness data."
            );
            return Err(RollupError::Prover(ProverError::CircuitError(
                "Non-empty batch proof not yet implemented — witness data is \
                 placeholder zeros. Use ExpandedRollupCircuit::from_batch with \
                 real Merkle proofs and intermediate state roots."
                    .to_string(),
            )));
        }

        let circuit = ExpandedRollupCircuit::from_state_roots(
            *_old_root,
            *_new_root,
            event_count as u64,
            *_old_root, // expected_old = old (verifier independently knows this)
            *_new_root, // expected_new = new (verifier checks this matches on-chain)
        );
        let pub_input = circuit
            .public_input()
            .map_err(|e| RollupError::Prover(ProverError::CircuitError(e.to_string())))?;

        // SAFETY: setup_cache is guaranteed Some by the initialization block above.
        // Using ok_or_else avoids unwrap/expect while the error path is unreachable.
        let cache = self.setup_cache.as_ref().ok_or_else(|| {
            RollupError::Prover(ProverError::SetupFailed(
                "trusted setup cache not initialized (logic error)".into(),
            ))
        })?;

        // Create proof
        tracing::debug!("Creating Groth16 proof for {} events", event_count);
        let proof_obj = prover::create_expanded_proof(circuit, &cache.proving_key)?;

        // Serialize proof to bytes
        let proof_bytes = prover::serialize_proof(&proof_obj)?;

        // Self-verify for sanity check
        let valid = prover::verify_proof(&cache.verifying_key, &pub_input, &proof_obj)?;

        if valid {
            tracing::info!("Self-verification of proof succeeded");
        } else {
            tracing::warn!("Self-verification of proof FAILED");
        }

        Ok(proof_bytes)
    }

    /// Get the chain ID of the current settlement adapter.
    pub fn chain_id(&self) -> &'static str {
        self.settlement.chain_id()
    }

    /// Get the number of events that have been batched so far.
    pub fn last_batched_index(&self) -> usize {
        self.last_batched_index
    }

    /// Get a reference to the cached verifying key, if available.
    ///
    /// Returns `None` if the trusted setup has not been performed yet.
    pub fn verifying_key(&self) -> Option<&VerifyingKey> {
        self.setup_cache.as_ref().map(|c| &c.verifying_key)
    }
}

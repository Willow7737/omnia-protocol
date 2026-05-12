//! L2 batch operator — L1-agnostic.
//!
//! The [`RollupOperator`] collects finalized events from the substrate's
//! causal graph, builds batches, generates proofs, and posts them to the
//! configured settlement layer. The operator is completely L1-agnostic —
//! it works with any [`SettlementLayer`] implementor.

use crate::settlement::{SettlementError, SettlementLayer};
use omnia_substrate::{CausalGraph, Event, Substrate};
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
}

/// L2 batch operator. Collects finalized events, builds batches,
/// generates proofs, and posts to the configured settlement layer.
///
/// The operator is L1-agnostic — it works with any [`SettlementLayer`]
/// implementor. Swapping Ethereum for Bitcoin or Solana requires only
/// changing the adapter passed to [`RollupOperator::new`].
pub struct RollupOperator {
    substrate: Arc<RwLock<Substrate>>,
    settlement: Box<dyn SettlementLayer>,
    batch_size: usize,
    last_batched_index: usize,
}

impl RollupOperator {
    /// Create a new rollup operator.
    ///
    /// # Arguments
    /// * `substrate` — The L2 substrate (wrapped in `Arc<RwLock<>>` for async access)
    /// * `settlement` — The L1 settlement adapter (any `SettlementLayer` implementor)
    /// * `batch_size` — Maximum number of events per batch
    pub fn new(
        substrate: Arc<RwLock<Substrate>>,
        settlement: Box<dyn SettlementLayer>,
        batch_size: usize,
    ) -> Self {
        Self {
            substrate,
            settlement,
            batch_size,
            last_batched_index: 0,
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

        // 2. Get old state root
        let old_root = {
            let sub = self.substrate.read().await;
            let graph = sub.graph().await;
            graph.state_root()
        };

        // 3. Build batch data
        let batch_data = self.build_batch_data(&events);

        // 4. Get new state root (events already processed by consensus)
        let new_root = {
            let sub = self.substrate.read().await;
            let graph = sub.graph().await;
            graph.state_root()
        };

        // 5. Generate proof (stub for Phase 0)
        let proof = self.generate_proof_stub(&old_root, &new_root, &batch_data);

        // 6. Post to settlement layer
        let tx_ref = self.settlement.post_batch(&batch_data).await?;
        tracing::info!(
            "[{}] Batch posted: {} events, tx: {}",
            self.settlement.chain_id(),
            events.len(),
            &tx_ref[..tx_ref.len().min(16)]
        );

        // 7. Verify proof on L1 (for monitoring)
        let valid = self
            .settlement
            .verify_proof(&old_root, &new_root, &proof)
            .await?;
        if !valid {
            tracing::warn!(
                "[{}] Proof verification returned false",
                self.settlement.chain_id()
            );
        }

        self.last_batched_index += events.len();
        Ok(())
    }

    /// Collect events from the causal graph that haven't been batched yet.
    async fn collect_events(&self) -> Result<Vec<Event>, RollupError> {
        let sub = self.substrate.read().await;
        let graph = sub.graph().await;
        let events = Self::collect_from_graph(&graph, self.last_batched_index, self.batch_size);
        Ok(events)
    }

    /// Extract events from a causal graph starting at a given index.
    fn collect_from_graph(
        graph: &CausalGraph,
        start_index: usize,
        limit: usize,
    ) -> Vec<Event> {
        let all_ids = graph.event_ids();
        all_ids
            .iter()
            .skip(start_index)
            .take(limit)
            .filter_map(|id| graph.get(id).cloned())
            .collect()
    }

    /// Serialize events into batch data.
    fn build_batch_data(&self, events: &[Event]) -> Vec<u8> {
        bincode::serialize(events).expect("Serialization cannot fail")
    }

    /// Generate a dummy proof for Phase 0.
    ///
    /// In production, this runs the ZK prover (Groth16, PLONK, or STARK).
    fn generate_proof_stub(
        &self,
        _old_root: &[u8; 32],
        _new_root: &[u8; 32],
        _batch_data: &[u8],
    ) -> Vec<u8> {
        vec![0xAB; 192]
    }

    /// Get the chain ID of the current settlement adapter.
    pub fn chain_id(&self) -> &'static str {
        self.settlement.chain_id()
    }

    /// Get the number of events that have been batched so far.
    pub fn last_batched_index(&self) -> usize {
        self.last_batched_index
    }
}

//! L2 batch operator — L1-agnostic.
//!
//! The [`RollupOperator`] collects finalized events from the causal graph,
//! builds batches, generates Groth16 proofs, and posts them to the
//! configured settlement layer. The operator is completely L1-agnostic —
//! it works with any [`SettlementLayer`] implementor.

use crate::circuit::ExpandedRollupCircuit;
use crate::merkle::{build_poseidon_merkle_tree, fr_to_hash, hash_to_fr};
use crate::poseidon::poseidon_hash_offchain;
use crate::prover::{self, ProverError, ProvingKey, VerifyingKey};
use crate::settlement::{SettlementError, SettlementLayer};
use ark_bn254::Fr;
use ark_ff::Zero;
use omnia_consensus::CausalGraph;
use omnia_primitives::Event;
use std::collections::HashMap;
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

/// The output of proving one batch: the serialized Groth16 proof plus
/// the public values it binds (AUDIT-2026-07 C3, #341).
pub struct ProvenBatch {
    /// Serialized Groth16 proof.
    pub proof_bytes: Vec<u8>,
    /// ZK state root before the batch (32 big-endian bytes).
    pub zk_old_root: [u8; 32],
    /// ZK state root after the batch (32 big-endian bytes).
    pub zk_new_root: [u8; 32],
    /// ZK state root after the batch, as a field element.
    pub zk_new_root_fr: Fr,
    /// Poseidon Merkle event commitment (32 bytes).
    pub event_commitment: [u8; 32],
}

/// Cached Groth16 trusted setup keys for one circuit shape.
struct TrustedSetupCache {
    proving_key: ProvingKey,
    verifying_key: VerifyingKey,
}

/// Circuit shape: (number of events, Merkle proof depth). The trusted
/// setup is shape-specific — a key generated for one shape cannot prove
/// a circuit of another shape (AUDIT-2026-07 C3, #341: the old code
/// cached a single setup for `event_count.max(1)` and reused it for
/// every batch size).
type CircuitShape = (usize, usize);

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
    /// Cached trusted setup keys, one per circuit shape.
    setup_cache: HashMap<CircuitShape, TrustedSetupCache>,
    /// The rollup's ZK state root: a Poseidon fold-chain over event
    /// hashes, matching the circuit's state-transition constraint
    /// `root[i+1] = Poseidon(root[i], event_hash[i])` (AUDIT-2026-07 C3,
    /// #341). Starts at `Fr::zero()` — the rollup genesis root, which
    /// must equal the L1 contract's `_initialStateRoot`. This is the
    /// root the settlement layer tracks; the causal graph's BLAKE3
    /// state root is a different (unprovable) construction and is used
    /// for logging only.
    zk_state_root: Fr,
    /// Shape of the last generated proof (events, depth) — exposed so
    /// callers can locate the matching verifying key.
    last_proof_shape: Option<CircuitShape>,
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
            setup_cache: HashMap::new(),
            zk_state_root: Fr::zero(),
            last_proof_shape: None,
        }
    }

    /// The current ZK state root as 32 big-endian bytes (the value the
    /// L1 contract tracks).
    pub fn zk_state_root_bytes(&self) -> [u8; 32] {
        fr_to_hash(&self.zk_state_root)
    }

    /// Run one batch cycle: collect → build → prove → post.
    pub async fn run_batch(&mut self) -> Result<(), RollupError> {
        // 1. Collect finalized events
        let events = self.collect_events().await?;
        if events.is_empty() {
            tracing::info!("No new events to batch");
            return Ok(());
        }

        // 2. Apply events to the causal graph (its BLAKE3 state root is a
        //    different construction from the provable ZK root — logged for
        //    diagnostics only; see `zk_state_root`).
        // Events collected by `collect_events` already live in the graph
        // (they were read FROM it) — re-inserting them is a duplicate.
        // The old code errored out here on every non-empty batch; it was
        // masked because proof generation rejected non-empty batches
        // first (AUDIT-2026-07 C3, #341). Insert only events the graph
        // does not already contain.
        let mut graph = self.graph.write().await;
        for event in &events {
            if graph.get(&event.id).is_none() {
                graph.insert(event.clone()).map_err(|e| {
                    RollupError::Serialization(format!("Failed to insert event into causal graph: {e}"))
                })?;
            }
        }
        drop(graph);

        // 3. Build batch data
        let batch_data = self.build_batch_data(&events)?;

        // 4. Generate a real Groth16 proof over the batch (AUDIT-2026-07
        //    C3, #341): the circuit's public inputs are the Poseidon
        //    fold-chain roots (zk_old, zk_new) and the Poseidon Merkle
        //    event commitment — all computed from real witnesses.
        let proven = self.generate_proof(&events)?;

        tracing::info!(
            "[{}] Generated Groth16 proof: {} bytes ({} events)",
            self.settlement.chain_id(),
            proven.proof_bytes.len(),
            events.len()
        );

        // 5. Post to settlement layer
        let tx_ref = self.settlement.post_batch(&batch_data).await?;
        tracing::info!(
            "[{}] Batch posted: {} events, tx: {}",
            self.settlement.chain_id(),
            events.len(),
            &tx_ref[..tx_ref.len().min(16)]
        );

        // 6. Verify proof on L1 (for monitoring) against the ZK roots the
        //    proof actually binds.
        let valid = self
            .settlement
            .verify_proof(&proven.zk_old_root, &proven.zk_new_root, &proven.proof_bytes)
            .await?;
        if !valid {
            tracing::warn!("[{}] Proof verification returned false", self.settlement.chain_id());
        }

        // 7. Advance the rollup state only after the batch is posted.
        self.zk_state_root = proven.zk_new_root_fr;
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

    /// Generate a Groth16 proof for a batch of events (AUDIT-2026-07 C3,
    /// #341 — the pipeline can now prove NON-EMPTY batches).
    ///
    /// Builds the full witness set the `ExpandedRollupCircuit` requires:
    ///
    /// - **event hashes**: `hash_to_fr(event.id)` for each event;
    /// - **operation types**: a fixed in-range tag (0) — the circuit
    ///   enforces the range and the payload binding; semantic per-shard
    ///   operation tagging is tracked follow-up;
    /// - **payload hashes**: `Poseidon(event_hash, op_type)`, the exact
    ///   binding the circuit enforces;
    /// - **event commitment + Merkle proofs**: from
    ///   [`build_poseidon_merkle_tree`] over the event IDs (the same
    ///   Poseidon the circuit verifies paths with);
    /// - **intermediate roots**: the Poseidon fold chain
    ///   `root[i+1] = Poseidon(root[i], event_hash[i])` starting from the
    ///   operator's current `zk_state_root`.
    ///
    /// The trusted setup is cached **per circuit shape** (events, depth):
    /// a Groth16 key proves exactly one constraint-system shape.
    fn generate_proof(&mut self, events: &[Event]) -> Result<ProvenBatch, RollupError> {
        let zk_err = |e: crate::poseidon::ZkError| RollupError::Prover(ProverError::CircuitError(e.to_string()));

        // Witnesses: event hashes and payload bindings.
        let event_hashes: Vec<Fr> = events.iter().map(|e| hash_to_fr(&e.id)).collect();
        let operation_types: Vec<Fr> = events.iter().map(|_| Fr::zero()).collect();
        let payload_hashes: Vec<Fr> = event_hashes
            .iter()
            .map(|h| poseidon_hash_offchain(*h, Fr::zero()).map_err(zk_err))
            .collect::<Result<_, _>>()?;

        // Event commitment: Poseidon Merkle tree over event IDs.
        let ids: Vec<[u8; 32]> = events.iter().map(|e| e.id).collect();
        let (commitment_bytes, merkle_proofs) = build_poseidon_merkle_tree(&ids)
            .map_err(|e| RollupError::Prover(ProverError::CircuitError(e.to_string())))?;
        let event_commitment = hash_to_fr(&commitment_bytes);

        // Intermediate roots: the Poseidon fold chain from the current
        // rollup state root.
        let mut intermediate_roots = Vec::with_capacity(events.len() + 1);
        intermediate_roots.push(self.zk_state_root);
        for h in &event_hashes {
            let prev = *intermediate_roots
                .last()
                .ok_or_else(|| RollupError::Prover(ProverError::CircuitError("empty fold chain".into())))?;
            intermediate_roots.push(poseidon_hash_offchain(prev, *h).map_err(zk_err)?);
        }
        let zk_old_root_fr = self.zk_state_root;
        let zk_new_root_fr = *intermediate_roots
            .last()
            .ok_or_else(|| RollupError::Prover(ProverError::CircuitError("empty fold chain".into())))?;

        // Trusted setup for this exact circuit shape.
        let merkle_depth = merkle_proofs.first().map(|p| p.siblings.len()).unwrap_or(0);
        let shape: CircuitShape = (events.len(), merkle_depth);
        if let std::collections::hash_map::Entry::Vacant(entry) = self.setup_cache.entry(shape) {
            tracing::info!(
                events = shape.0,
                depth = shape.1,
                "Generating trusted setup for ExpandedRollupCircuit shape"
            );
            let (pk, vk) = prover::generate_trusted_setup_expanded(shape.0, shape.1)?;
            entry.insert(TrustedSetupCache {
                proving_key: pk,
                verifying_key: vk,
            });
        }

        let circuit = ExpandedRollupCircuit::from_batch(
            zk_old_root_fr,
            zk_new_root_fr,
            event_hashes,
            operation_types,
            payload_hashes,
            event_commitment,
            merkle_proofs,
            intermediate_roots,
        );
        let pub_input = circuit
            .public_input()
            .map_err(|e| RollupError::Prover(ProverError::CircuitError(e.to_string())))?;

        let cache = self.setup_cache.get(&shape).ok_or_else(|| {
            RollupError::Prover(ProverError::SetupFailed(
                "trusted setup cache not initialized (logic error)".into(),
            ))
        })?;

        tracing::debug!("Creating Groth16 proof for {} events", events.len());
        let proof_obj = prover::create_expanded_proof(circuit, &cache.proving_key)?;
        let proof_bytes = prover::serialize_proof(&proof_obj)?;

        // Self-verify before posting anything: a proof that fails here
        // must never advance state or reach the settlement layer.
        let valid = prover::verify_proof(&cache.verifying_key, &pub_input, &proof_obj)?;
        if !valid {
            return Err(RollupError::Prover(ProverError::CircuitError(
                "self-verification of generated proof failed".into(),
            )));
        }
        tracing::info!("Self-verification of proof succeeded");
        self.last_proof_shape = Some(shape);

        Ok(ProvenBatch {
            proof_bytes,
            zk_old_root: fr_to_hash(&zk_old_root_fr),
            zk_new_root: fr_to_hash(&zk_new_root_fr),
            zk_new_root_fr,
            event_commitment: commitment_bytes,
        })
    }

    /// Get the chain ID of the current settlement adapter.
    pub fn chain_id(&self) -> &'static str {
        self.settlement.chain_id()
    }

    /// Get the number of events that have been batched so far.
    pub fn last_batched_index(&self) -> usize {
        self.last_batched_index
    }

    /// Get a reference to the verifying key for the most recent proof's
    /// circuit shape, if a proof has been generated.
    pub fn verifying_key(&self) -> Option<&VerifyingKey> {
        let shape = self.last_proof_shape?;
        self.setup_cache.get(&shape).map(|c| &c.verifying_key)
    }
}

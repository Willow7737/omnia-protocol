//! AUDIT-2026-07 C3 (#341) regression tests: the ZK rollup pipeline must
//! prove NON-EMPTY batches.
//!
//! Before the fix, `RollupOperator::generate_proof` returned `Err` for any
//! batch with `event_count > 0` — `ExpandedRollupCircuit::from_state_roots`
//! used placeholder zero witnesses, making the circuit unsatisfiable, so
//! the entire L2 → L1 settlement path could only "prove" empty batches.
//! These tests exercise the full collect → witness → prove → self-verify →
//! post cycle with real events and real Groth16 proofs.
//!
//! Requires the `arkworks` feature (CI runs it in the feature matrix):
//!     cargo test -p omnia-adapters --features arkworks --test rollup_nonempty_batch
#![cfg(feature = "arkworks")]
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use omnia_adapters::operator::RollupOperator;
use omnia_adapters::proof_bundle::ProofBundle;
use omnia_adapters::settlement::{SettlementError, SettlementLayer};
use omnia_consensus::CausalGraph;
use omnia_primitives::{Event, VectorClock};
use tokio::sync::RwLock;

/// A test settlement layer that accepts everything and records calls.
struct RecordingSettlement {
    posted: AtomicUsize,
    verified: AtomicUsize,
}

#[async_trait::async_trait]
impl SettlementLayer for RecordingSettlement {
    fn chain_id(&self) -> &'static str {
        "test-recording"
    }

    async fn post_batch(&self, batch_data: &[u8]) -> Result<String, SettlementError> {
        assert!(!batch_data.is_empty(), "batch data must not be empty");
        self.posted.fetch_add(1, Ordering::SeqCst);
        Ok(format!("tx-{}", self.posted.load(Ordering::SeqCst)))
    }

    async fn verify_proof(
        &self,
        old_root: &[u8; 32],
        new_root: &[u8; 32],
        proof: &[u8],
    ) -> Result<bool, SettlementError> {
        assert_ne!(old_root, new_root, "a non-empty batch must advance the ZK root");
        assert!(!proof.is_empty(), "proof bytes must not be empty");
        self.verified.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }

    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError> {
        Ok([0u8; 32])
    }

    async fn deposit(&self, _did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented("test adapter".into()))
    }

    async fn request_withdrawal(&self, _did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented("test adapter".into()))
    }

    async fn submit_batch(&self, _bundle: &ProofBundle) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented("test adapter".into()))
    }
}

/// Build a graph pre-loaded with `n` chained events from one creator.
fn graph_with_events(n: u64) -> Arc<RwLock<CausalGraph>> {
    let creator = [7u8; 32];
    let mut graph = CausalGraph::new();
    let mut prev: Option<[u8; 32]> = None;
    for seq in 0..n {
        let mut vc = VectorClock::new();
        for _ in 0..=seq {
            vc.increment(creator).unwrap();
        }
        let event = Event::new(creator, seq, vc, prev, None, format!("payload-{seq}").into_bytes()).unwrap();
        prev = Some(event.id);
        graph.insert(event).unwrap();
    }
    Arc::new(RwLock::new(graph))
}

/// THE C3 regression: a batch of real events produces a real Groth16
/// proof that self-verifies, is posted, and advances the ZK state root.
/// On the old code this failed with "Non-empty batch proof not yet
/// implemented".
#[tokio::test]
async fn nonempty_batch_proves_and_posts() {
    let graph = graph_with_events(3);
    let settlement = Box::new(RecordingSettlement {
        posted: AtomicUsize::new(0),
        verified: AtomicUsize::new(0),
    });

    let mut operator = RollupOperator::new(graph, settlement, 8);
    let genesis_root = operator.zk_state_root_bytes();

    operator.run_batch().await.expect("non-empty batch must prove and post");

    assert_eq!(operator.last_batched_index(), 3, "all 3 events must be batched");
    assert_ne!(
        operator.zk_state_root_bytes(),
        genesis_root,
        "ZK state root must advance after a batch"
    );
    assert!(operator.verifying_key().is_some(), "setup must be cached for the shape");
}

/// Consecutive batches chain: batch 2 starts from batch 1's ZK root, and
/// each advances the root (the fold chain is stateful across batches).
#[tokio::test]
async fn consecutive_batches_chain_the_zk_root() {
    let graph = graph_with_events(4);
    let settlement = Box::new(RecordingSettlement {
        posted: AtomicUsize::new(0),
        verified: AtomicUsize::new(0),
    });

    // batch_size 2 → two batches of two events.
    let mut operator = RollupOperator::new(graph, settlement, 2);

    operator.run_batch().await.expect("batch 1");
    let root_after_1 = operator.zk_state_root_bytes();

    operator.run_batch().await.expect("batch 2");
    let root_after_2 = operator.zk_state_root_bytes();

    assert_eq!(operator.last_batched_index(), 4);
    assert_ne!(root_after_1, root_after_2, "each batch must advance the root");
}

/// Empty graph → run_batch is a clean no-op (no proof, no post, no root
/// movement).
#[tokio::test]
async fn empty_batch_is_a_noop() {
    let graph = Arc::new(RwLock::new(CausalGraph::new()));
    let settlement = Box::new(RecordingSettlement {
        posted: AtomicUsize::new(0),
        verified: AtomicUsize::new(0),
    });

    let mut operator = RollupOperator::new(graph, settlement, 8);
    let genesis_root = operator.zk_state_root_bytes();

    operator.run_batch().await.expect("empty batch is a no-op");
    assert_eq!(operator.last_batched_index(), 0);
    assert_eq!(operator.zk_state_root_bytes(), genesis_root);
}

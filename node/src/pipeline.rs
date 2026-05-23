//! Pipeline Router — Splits node work into hot, warm, and cold paths.
//!
//! The pipeline router is responsible for dispatching work to different
//! priority channels based on expected latency:
//!
//! - **Hot path** (<500µs): Event validation, signature checks, graph inserts.
//!   These operations must never block or perform I/O.
//!
//! - **Warm path** (<5ms): Mempool inserts, consensus processing, shard routing.
//!   Light I/O is acceptable but no CPU-heavy operations.
//!
//! - **Cold path** (<2s): ZK proof generation, snapshot compression, settlement
//!   submissions. These are CPU-heavy or network-bound operations that run on
//!   blocking threads via `spawn_blocking`.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────┐
//! │  HTTP API /  │
//! │  CLI input   │
//! └──────┬───────┘
//!        │ submit_event()
//!        ▼
//! ┌──────────────┐
//! │  Pipeline    │──► hot_tx ──► Hot Worker  (validation, graph insert)
//! │  Router      │──► warm_tx ──► Warm Worker (consensus, mempool, shards)
//! └──────────────┘──► cold_tx ──► Cold Worker (ZK, snapshot, settlement)
//! ```
//!
//! The router uses bounded channels for hot and warm paths to provide
//! back-pressure, and an unbounded channel for cold paths since they
//! are inherently slow and should not block the hot path.

use tokio::sync::mpsc;

/// Work item for the hot path — must complete in <500µs.
///
/// Hot work includes event validation, signature verification,
/// and causal graph insertion. These operations must never
/// perform I/O or block the async runtime.
#[derive(Debug)]
pub struct HotWork {
    /// The event payload to process (serialized).
    pub event_bytes: Vec<u8>,
}

/// Work item for the warm path — must complete in <5ms.
///
/// Warm work includes consensus processing, mempool insertion,
/// shard routing, and gossip broadcast. Light I/O is acceptable.
#[derive(Debug)]
pub struct WarmWork {
    /// The event ID that was validated and inserted.
    pub event_id: [u8; 32],
}

/// Work item for the cold path — may take up to <2s.
///
/// Cold work includes ZK proof generation, snapshot compression,
/// settlement layer submissions, and other CPU-heavy or
/// network-bound operations.
#[derive(Debug)]
pub enum ColdWork {
    /// Generate a ZK proof for a batch of events.
    GenerateProof {
        /// The event IDs in this batch.
        event_ids: Vec<[u8; 32]>,
    },
    /// Compress and replicate a state snapshot.
    SnapshotReplication {
        /// The snapshot round.
        round: u64,
    },
    /// Submit a batch to the settlement layer.
    SettlementSubmit {
        /// The serialized batch data.
        batch_data: Vec<u8>,
    },
}

/// Pipeline router that splits work into hot, warm, and cold channels.
///
/// The router provides bounded senders for hot and warm paths, and
/// an unbounded sender for cold paths. Workers are spawned separately
/// via `PipelineRouter::spawn_workers`.
pub struct PipelineRouter {
    /// Sender for hot-path work items (bounded, provides back-pressure).
    hot_tx: mpsc::Sender<HotWork>,
    /// Sender for warm-path work items (bounded, provides back-pressure).
    warm_tx: mpsc::Sender<WarmWork>,
    /// Sender for cold-path work items (unbounded, never blocks sender).
    cold_tx: mpsc::Sender<ColdWork>,
}

impl PipelineRouter {
    /// Create a new pipeline router with default channel sizes.
    ///
    /// Returns the router and the three receivers for worker tasks.
    /// The hot channel has a capacity of 1024, the warm channel 256,
    /// and the cold channel is unbounded.
    pub fn new() -> (
        Self,
        mpsc::Receiver<HotWork>,
        mpsc::Receiver<WarmWork>,
        mpsc::Receiver<ColdWork>,
    ) {
        let (hot_tx, hot_rx) = mpsc::channel(1024);
        let (warm_tx, warm_rx) = mpsc::channel(256);
        let (cold_tx, cold_rx) = mpsc::channel(1024);

        (
            Self {
                hot_tx,
                warm_tx,
                cold_tx,
            },
            hot_rx,
            warm_rx,
            cold_rx,
        )
    }

    /// Create a new pipeline router with custom channel sizes.
    pub fn with_capacity(
        hot_capacity: usize,
        warm_capacity: usize,
        cold_capacity: usize,
    ) -> (
        Self,
        mpsc::Receiver<HotWork>,
        mpsc::Receiver<WarmWork>,
        mpsc::Receiver<ColdWork>,
    ) {
        let (hot_tx, hot_rx) = mpsc::channel(hot_capacity);
        let (warm_tx, warm_rx) = mpsc::channel(warm_capacity);
        let (cold_tx, cold_rx) = mpsc::channel(cold_capacity);

        (
            Self {
                hot_tx,
                warm_tx,
                cold_tx,
            },
            hot_rx,
            warm_rx,
            cold_rx,
        )
    }

    /// Submit hot-path work.
    ///
    /// Returns `Err` if the hot channel is full (back-pressure).
    pub async fn submit_hot(&self, work: HotWork) -> Result<(), mpsc::error::SendError<HotWork>> {
        self.hot_tx.send(work).await
    }

    /// Try to submit hot-path work without waiting.
    ///
    /// Returns `Err` if the channel is full or closed.
    pub fn try_submit_hot(&self, work: HotWork) -> Result<(), mpsc::error::TrySendError<HotWork>> {
        self.hot_tx.try_send(work)
    }

    /// Submit warm-path work.
    pub async fn submit_warm(&self, work: WarmWork) -> Result<(), mpsc::error::SendError<WarmWork>> {
        self.warm_tx.send(work).await
    }

    /// Submit cold-path work.
    pub async fn submit_cold(&self, work: ColdWork) -> Result<(), mpsc::error::SendError<ColdWork>> {
        self.cold_tx.send(work).await
    }

    /// Get a clone of the hot sender (for passing to HTTP handlers).
    pub fn hot_sender(&self) -> mpsc::Sender<HotWork> {
        self.hot_tx.clone()
    }

    /// Get a clone of the warm sender.
    pub fn warm_sender(&self) -> mpsc::Sender<WarmWork> {
        self.warm_tx.clone()
    }

    /// Get a clone of the cold sender.
    pub fn cold_sender(&self) -> mpsc::Sender<ColdWork> {
        self.cold_tx.clone()
    }
}

impl Default for PipelineRouter {
    fn default() -> Self {
        // Create without receivers (for API compatibility)
        let (hot_tx, _) = mpsc::channel(1024);
        let (warm_tx, _) = mpsc::channel(256);
        let (cold_tx, _) = mpsc::channel(1024);
        Self {
            hot_tx,
            warm_tx,
            cold_tx,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pipeline_router_creation() {
        let (router, hot_rx, warm_rx, cold_rx) = PipelineRouter::new();
        assert!(router.try_submit_hot(HotWork { event_bytes: vec![] }).is_ok());
        drop(hot_rx);
        drop(warm_rx);
        drop(cold_rx);
    }

    #[tokio::test]
    async fn test_pipeline_router_submit_warm() {
        let (router, _hot_rx, mut warm_rx, _cold_rx) = PipelineRouter::new();
        router.submit_warm(WarmWork { event_id: [1u8; 32] }).await.unwrap();
        let work = warm_rx.recv().await.unwrap();
        assert_eq!(work.event_id, [1u8; 32]);
    }

    #[tokio::test]
    async fn test_pipeline_router_submit_cold() {
        let (router, _hot_rx, _warm_rx, mut cold_rx) = PipelineRouter::new();
        router
            .submit_cold(ColdWork::SnapshotReplication { round: 42 })
            .await
            .unwrap();
        let work = cold_rx.recv().await.unwrap();
        match work {
            ColdWork::SnapshotReplication { round } => assert_eq!(round, 42),
            _ => panic!("Expected SnapshotReplication"),
        }
    }

    #[test]
    fn test_hot_work_debug() {
        let work = HotWork {
            event_bytes: vec![1, 2, 3],
        };
        assert!(format!("{work:?}").contains("HotWork"));
    }

    #[test]
    fn test_cold_work_variants() {
        let proof = ColdWork::GenerateProof {
            event_ids: vec![[0u8; 32]],
        };
        let snapshot = ColdWork::SnapshotReplication { round: 100 };
        let settlement = ColdWork::SettlementSubmit {
            batch_data: vec![4, 5, 6],
        };
        assert!(format!("{proof:?}").contains("GenerateProof"));
        assert!(format!("{snapshot:?}").contains("SnapshotReplication"));
        assert!(format!("{settlement:?}").contains("SettlementSubmit"));
    }
}

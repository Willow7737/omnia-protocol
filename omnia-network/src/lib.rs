//! Omnia Network — P2P networking layer
//!
//! Provides gossip protocol, fast-sync, batch gossip, and snapshot replication
//! behind feature gates for optional networking dependencies.
//!
//! # Sprint 4: Network-Optimized Gossip
//!
//! This crate includes gossip optimizations for achieving ≤500ms p99
//! propagation latency in 3-node testnets:
//!
//! - **Compact event encoding** ([`compact_event_encoding`]): Delta-compressed
//!   vector clocks and truncated event IDs reduce wire size by ~40%.
//! - **Bloom filter dedup** ([`gossip_bloom_filter`]): O(1) duplicate event
//!   suppression with a rotating bloom filter pair and bounded FPR.
//! - **Priority gossip queue** ([`priority_gossip_queue`]): Finality-critical
//!   events (witness, fame votes) jump the queue for faster propagation.

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod blake3_domain;
pub mod compact_event_encoding;
pub mod fast_sync;
#[cfg(feature = "network")]
pub mod gossip;
pub mod gossip_batch;
pub mod gossip_bloom_filter;
#[cfg(feature = "network")]
pub mod network;
pub mod priority_gossip_queue;

// Re-export commonly used types
pub use compact_event_encoding::{
    CompactEncoder, CompactEvent, DeltaClock, EncodingError, DEFAULT_ID_TRUNCATION_BYTES, DEFAULT_MAX_DELTA_CLOCK_SIZE,
};
pub use fast_sync::{
    select_target_checkpoint, FastSyncManager, SyncCheckpoint, SyncError, SyncNetwork, SyncRequest, SyncResponse,
    SyncResult, SyncSnapshot,
};
#[cfg(feature = "network")]
pub use gossip::{
    deserialize_compressed, serialize_compressed, GossipConfig, GossipDigest, GossipError, GossipEvent, GossipMessage,
    GossipProtocol, GossipStats,
};
pub use gossip_batch::{
    deserialize_batch_message, serialize_batch_message, validate_batch_message, BatchGossipStats, GossipBatchError,
    GossipBatchMessage, GOSSIP_BATCH_TOPIC, MAX_BATCH_GOSSIP_SIZE,
};
pub use gossip_bloom_filter::GossipBloomFilter;
#[cfg(feature = "network")]
pub use libp2p::Multiaddr;
#[cfg(feature = "network")]
pub use network::{
    check_version_compatibility, configure_gossipsub_scoring, NetworkCommand, NetworkConfig, NetworkEvent,
    OmniaBehaviour, OmniaNetwork, PeerScoreTracker, VersionCompatibility, VersionHandshake,
};
pub use priority_gossip_queue::{GossipPriority, PriorityGossipQueue, PriorityQueueConfig};

/// Protocol version identifier
pub const PROTOCOL_VERSION: &str = "4.0.0";
/// libp2p protocol identifier
pub const PROTOCOL_IDENTIFIER: &str = "/omnia/4.0.0";

//! Omnia Network — P2P networking layer
//!
//! Provides gossip protocol, fast-sync, and snapshot replication
//! behind feature gates for optional networking dependencies.

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod blake3_domain;
pub mod fast_sync;
#[cfg(feature = "network")]
pub mod gossip;
#[cfg(feature = "network")]
pub mod network;

// Re-export commonly used types
pub use fast_sync::{
    select_target_checkpoint, FastSyncManager, SyncCheckpoint, SyncError, SyncNetwork, SyncRequest,
    SyncResponse, SyncResult, SyncSnapshot,
};
#[cfg(feature = "network")]
pub use gossip::{
    deserialize_compressed, serialize_compressed, GossipConfig, GossipDigest, GossipError,
    GossipEvent, GossipMessage, GossipProtocol, GossipStats,
};
#[cfg(feature = "network")]
pub use network::{
    check_version_compatibility, configure_gossipsub_scoring, NetworkCommand, NetworkConfig,
    NetworkEvent, OmniaBehaviour, OmniaNetwork, PeerScoreTracker, VersionCompatibility,
    VersionHandshake,
};

/// Protocol version identifier
pub const PROTOCOL_VERSION: &str = "4.0.0";
/// libp2p protocol identifier
pub const PROTOCOL_IDENTIFIER: &str = "/omnia/4.0.0";

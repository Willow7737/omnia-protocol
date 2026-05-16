//! Snapshot Replication — Fast State Synchronization Across Nodes.
//!
//! This module provides utilities for replicating state snapshots between
//! nodes, enabling fast synchronization without replaying the entire event
//! history from genesis. A new node can import a recent snapshot from a
//! trusted peer and only needs to sync events after the snapshot height.
//!
//! # Replication Strategy
//!
//! 1. A new node queries peers for their latest snapshot.
//! 2. The node selects the snapshot with the highest height from trusted peers.
//! 3. The snapshot is verified (integrity check via BLAKE3 state root).
//! 4. Events after the snapshot height are synced via the gossip protocol.
//!
//! # Security Considerations
//!
//! - Snapshots must be verified before import (integrity hash check).
//! - Only accept snapshots from trusted peers (authenticated connections).
//! - The snapshot height must be below the current network tip.
//! - Multiple snapshots from different peers can be cross-validated.

use crate::snapshot::{SnapshotError, StateSnapshot};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for snapshot replication.
///
/// Controls how snapshots are discovered, verified, and transferred
/// between nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationConfig {
    /// Maximum number of snapshots to keep on disk (rolling window).
    ///
    /// Older snapshots are pruned when this limit is exceeded.
    /// Default: 5.
    pub max_snapshots: usize,
    /// Directory where snapshots are stored.
    ///
    /// Each snapshot is stored as a file named `snapshot-{height}.bin`.
    pub snapshot_dir: PathBuf,
    /// Whether to verify snapshot integrity before accepting.
    ///
    /// Should always be `true` in production. Set to `false` only
    /// in testing to allow intentionally corrupted snapshots.
    pub verify_on_import: bool,
    /// Minimum number of peer confirmations required before trusting
    /// a snapshot from an unknown source.
    ///
    /// Default: 1 (at least one other peer must have the same snapshot).
    pub min_peer_confirmations: usize,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            max_snapshots: 5,
            snapshot_dir: PathBuf::from("./data/snapshots"),
            verify_on_import: true,
            min_peer_confirmations: 1,
        }
    }
}

impl ReplicationConfig {
    /// Create a new replication config with the given snapshot directory.
    pub fn new(snapshot_dir: PathBuf) -> Self {
        Self {
            snapshot_dir,
            ..Default::default()
        }
    }
}

/// Replicate a snapshot by writing it to the configured snapshot directory.
///
/// The snapshot is written to `{snapshot_dir}/snapshot-{height}.bin`.
/// If `verify_on_import` is set in the config, the snapshot's integrity
/// is verified before writing.
///
/// # Arguments
///
/// * `config` — The replication configuration.
/// * `snapshot` — The [`StateSnapshot`] to replicate.
///
/// # Returns
///
/// The path to the written snapshot file on success, or a [`SnapshotError`]
/// on failure.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::snapshot_replication::{replicate_snapshot, ReplicationConfig};
/// use std::path::PathBuf;
///
/// let config = ReplicationConfig::new(PathBuf::from("./data/snapshots"));
/// let path = replicate_snapshot(&config, &snapshot)?;
/// ```
pub fn replicate_snapshot(
    config: &ReplicationConfig,
    snapshot: &StateSnapshot,
) -> Result<PathBuf, SnapshotError> {
    // Verify integrity before writing (if configured)
    if config.verify_on_import {
        snapshot.verify()?;
    }

    // Ensure the snapshot directory exists
    if let Err(e) = std::fs::create_dir_all(&config.snapshot_dir) {
        return Err(SnapshotError::Io(e));
    }

    // Write snapshot to file
    let filename = format!("snapshot-{}.bin", snapshot.height);
    let path = config.snapshot_dir.join(&filename);
    snapshot.write_to_file(&path)?;

    tracing::info!(
        height = snapshot.height,
        path = %path.display(),
        "Snapshot replicated to disk"
    );

    // Prune old snapshots if we exceed the limit
    if let Err(e) = prune_old_snapshots(config) {
        tracing::warn!(error = %e, "Failed to prune old snapshots");
    }

    Ok(path)
}

/// Find the latest snapshot in the configured snapshot directory.
///
/// Scans the snapshot directory for files matching the pattern
/// `snapshot-{height}.bin` and returns the one with the highest height.
///
/// # Arguments
///
/// * `config` — The replication configuration (specifies the snapshot dir).
///
/// # Returns
///
/// The latest [`StateSnapshot`] on success, or `None` if no snapshots
/// exist in the directory.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::snapshot_replication::{find_latest_snapshot, ReplicationConfig};
///
/// let config = ReplicationConfig::default();
/// if let Some(snapshot) = find_latest_snapshot(&config)? {
///     println!("Latest snapshot at height {}", snapshot.height);
/// }
/// ```
pub fn find_latest_snapshot(
    config: &ReplicationConfig,
) -> Result<Option<StateSnapshot>, SnapshotError> {
    let dir = &config.snapshot_dir;

    if !dir.exists() {
        return Ok(None);
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => return Err(SnapshotError::Io(e)),
    };

    let mut latest: Option<(u64, StateSnapshot)> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "bin").unwrap_or(false) {
            match StateSnapshot::read_from_file(&path) {
                Ok(snapshot) => {
                    // Verify if configured
                    if config.verify_on_import {
                        if let Err(e) = snapshot.verify() {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "Skipping corrupted snapshot"
                            );
                            continue;
                        }
                    }
                    match &latest {
                        Some((max_height, _)) if snapshot.height <= *max_height => {}
                        _ => latest = Some((snapshot.height, snapshot)),
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to read snapshot file"
                    );
                }
            }
        }
    }

    Ok(latest.map(|(_, snapshot)| snapshot))
}

/// Prune old snapshots to stay within the configured limit.
///
/// Keeps the `max_snapshots` most recent snapshots (by height) and deletes
/// the rest.
fn prune_old_snapshots(config: &ReplicationConfig) -> Result<(), SnapshotError> {
    let dir = &config.snapshot_dir;

    if !dir.exists() {
        return Ok(());
    }

    let entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(SnapshotError::Io)?
        .filter_map(|e| e.ok())
        .collect();

    if entries.len() <= config.max_snapshots {
        return Ok(());
    }

    // Parse heights from filenames and sort
    let mut snapshots: Vec<(u64, PathBuf)> = entries
        .iter()
        .filter_map(|entry| {
            let path = entry.path();
            let filename = path.file_name()?.to_str()?;
            // Parse "snapshot-{height}.bin"
            let height: u64 = filename
                .strip_prefix("snapshot-")?
                .strip_suffix(".bin")?
                .parse()
                .ok()?;
            Some((height, path))
        })
        .collect();

    // Sort by height descending — keep the newest
    snapshots.sort_by_key(|b| std::cmp::Reverse(b.0));

    // Delete all but the newest max_snapshots
    for (_, path) in snapshots.into_iter().skip(config.max_snapshots) {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(path = %path.display(), error = %e, "Failed to prune old snapshot");
        } else {
            tracing::debug!(path = %path.display(), "Pruned old snapshot");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::causal_graph::CausalGraph;
    use crate::crypto::generate_keypair;
    use crate::event::Event;
    use crate::slashing::SlashingState;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn test_node(id: u8) -> [u8; 32] {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    fn make_snapshot(height: u64) -> StateSnapshot {
        let mut graph = CausalGraph::new();
        let keypair = generate_keypair();
        let mut event = Event::genesis(test_node(1), vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);
        graph.insert(event).unwrap();

        StateSnapshot::take(&graph, &SlashingState::default(), &HashMap::new(), height)
            .expect("snapshot creation should succeed")
    }

    #[test]
    fn test_replicate_and_find_snapshot() {
        let tmp = TempDir::new().unwrap();
        let config = ReplicationConfig::new(tmp.path().to_path_buf());

        let snapshot = make_snapshot(100);
        let path = replicate_snapshot(&config, &snapshot).unwrap();

        assert!(path.exists());
        assert!(path.to_str().unwrap().contains("snapshot-100.bin"));

        let found = find_latest_snapshot(&config).unwrap().unwrap();
        assert_eq!(found.height, 100);
    }

    #[test]
    fn test_find_latest_with_multiple_snapshots() {
        let tmp = TempDir::new().unwrap();
        let config = ReplicationConfig::new(tmp.path().to_path_buf());

        let s1 = make_snapshot(100);
        let s2 = make_snapshot(200);
        let s3 = make_snapshot(300);

        replicate_snapshot(&config, &s1).unwrap();
        replicate_snapshot(&config, &s2).unwrap();
        replicate_snapshot(&config, &s3).unwrap();

        let latest = find_latest_snapshot(&config).unwrap().unwrap();
        assert_eq!(latest.height, 300);
    }

    #[test]
    fn test_find_latest_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let config = ReplicationConfig::new(tmp.path().to_path_buf());

        let result = find_latest_snapshot(&config).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_latest_nonexistent_dir() {
        let config = ReplicationConfig::new(PathBuf::from("/tmp/nonexistent-snapshots-test-xyz"));
        let result = find_latest_snapshot(&config).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_replication_config_default() {
        let config = ReplicationConfig::default();
        assert_eq!(config.max_snapshots, 5);
        assert!(config.verify_on_import);
        assert_eq!(config.min_peer_confirmations, 1);
    }

    #[test]
    fn test_snapshot_pruning() {
        let tmp = TempDir::new().unwrap();
        let mut config = ReplicationConfig::new(tmp.path().to_path_buf());
        config.max_snapshots = 2;

        // Create 4 snapshots
        for height in [100u64, 200, 300, 400] {
            let snapshot = make_snapshot(height);
            replicate_snapshot(&config, &snapshot).unwrap();
        }

        // Only the 2 newest should remain
        let files: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 2);

        // The latest should be the highest
        let latest = find_latest_snapshot(&config).unwrap().unwrap();
        assert_eq!(latest.height, 400);
    }
}

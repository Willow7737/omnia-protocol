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
/// Controls how snapshots are discovered, verified, and replicated
/// across multiple directories for redundancy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationConfig {
    /// List of directories where snapshots are replicated for redundancy.
    ///
    /// Each directory receives a copy of every snapshot, providing
    /// protection against disk failure. At least one directory is
    /// required. If multiple are provided, snapshots are written to
    /// all of them.
    pub replica_dirs: Vec<PathBuf>,
    /// Maximum number of snapshots to keep per replica directory.
    ///
    /// Older snapshots are pruned when this limit is exceeded.
    /// Default: 5.
    pub max_snapshots_per_replica: usize,
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
            replica_dirs: vec![PathBuf::from("./data/snapshots")],
            max_snapshots_per_replica: 5,
            verify_on_import: true,
            min_peer_confirmations: 1,
        }
    }
}

impl ReplicationConfig {
    /// Create a new replication config with a single replica directory.
    pub fn new(snapshot_dir: PathBuf) -> Self {
        Self {
            replica_dirs: vec![snapshot_dir],
            ..Default::default()
        }
    }

    /// Create a new replication config with multiple replica directories.
    pub fn with_replica_dirs(replica_dirs: Vec<PathBuf>) -> Self {
        Self {
            replica_dirs,
            ..Default::default()
        }
    }

    /// Returns the primary snapshot directory (first replica dir).
    ///
    /// Used by [`find_latest_snapshot`] as the default search location.
    pub fn primary_dir(&self) -> &PathBuf {
        self.replica_dirs
            .first()
            .expect("replica_dirs must not be empty")
    }
}

/// Replicate a snapshot by writing it to all configured replica directories.
///
/// The snapshot is written to each directory in `replica_dirs` as
/// `snapshot-{height}.bin`. If `verify_on_import` is set in the config,
/// the snapshot's integrity is verified before writing.
///
/// # Arguments
///
/// * `snapshot` — The [`StateSnapshot`] to replicate.
/// * `config` — The replication configuration.
///
/// # Returns
///
/// The path to the written snapshot file in the primary directory on success,
/// or a [`SnapshotError`] on failure.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::snapshot_replication::{replicate_snapshot, ReplicationConfig};
/// use std::path::PathBuf;
///
/// let config = ReplicationConfig::new(PathBuf::from("./data/snapshots"));
/// let path = replicate_snapshot(&snapshot, &config)?;
/// ```
pub fn replicate_snapshot(
    snapshot: &StateSnapshot,
    config: &ReplicationConfig,
) -> Result<PathBuf, SnapshotError> {
    // Verify integrity before writing (if configured)
    if config.verify_on_import {
        snapshot.verify()?;
    }

    let mut primary_path = None;

    for dir in &config.replica_dirs {
        // Ensure the replica directory exists
        if let Err(e) = std::fs::create_dir_all(dir) {
            return Err(SnapshotError::Io(e));
        }

        // Write snapshot to file
        let filename = format!("snapshot-{}.bin", snapshot.height);
        let path = dir.join(&filename);
        snapshot.write_to_file(&path)?;

        tracing::info!(
            height = snapshot.height,
            path = %path.display(),
            "Snapshot replicated to disk"
        );

        if primary_path.is_none() {
            primary_path = Some(path);
        }
    }

    // Prune old snapshots in each replica directory
    for dir in &config.replica_dirs {
        if let Err(e) = prune_old_snapshots(dir, config.max_snapshots_per_replica) {
            tracing::warn!(dir = %dir.display(), error = %e, "Failed to prune old snapshots");
        }
    }

    Ok(primary_path.expect("at least one replica dir must exist"))
}

/// Find the latest snapshot in the configured snapshot directories.
///
/// Scans the primary replica directory for files matching the pattern
/// `snapshot-{height}.bin` and returns the one with the highest height.
/// If no snapshot is found in the primary directory, falls back to
/// scanning the other replica directories.
///
/// # Arguments
///
/// * `config` — The replication configuration (specifies the replica dirs).
///
/// # Returns
///
/// The latest [`StateSnapshot`] on success, or `None` if no snapshots
/// exist in any directory.
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
    for dir in &config.replica_dirs {
        if let Some(snapshot) = find_latest_in_dir(dir, config.verify_on_import)? {
            return Ok(Some(snapshot));
        }
    }
    Ok(None)
}

/// Find the latest snapshot in a single directory.
fn find_latest_in_dir(dir: &PathBuf, verify: bool) -> Result<Option<StateSnapshot>, SnapshotError> {
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
                    if verify {
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
fn prune_old_snapshots(dir: &PathBuf, max_snapshots: usize) -> Result<(), SnapshotError> {
    if !dir.exists() {
        return Ok(());
    }

    let entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(SnapshotError::Io)?
        .filter_map(|e| e.ok())
        .collect();

    if entries.len() <= max_snapshots {
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
    for (_, path) in snapshots.into_iter().skip(max_snapshots) {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(path = %path.display(), error = %e, "Failed to prune old snapshot");
        } else {
            tracing::debug!(path = %path.display(), "Pruned old snapshot");
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
        let path = replicate_snapshot(&snapshot, &config).unwrap();

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

        replicate_snapshot(&s1, &config).unwrap();
        replicate_snapshot(&s2, &config).unwrap();
        replicate_snapshot(&s3, &config).unwrap();

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
        assert_eq!(config.replica_dirs.len(), 1);
        assert_eq!(config.max_snapshots_per_replica, 5);
        assert!(config.verify_on_import);
        assert_eq!(config.min_peer_confirmations, 1);
    }

    #[test]
    fn test_replication_config_primary_dir() {
        let config = ReplicationConfig::new(PathBuf::from("/tmp/test-snapshots"));
        assert_eq!(config.primary_dir(), &PathBuf::from("/tmp/test-snapshots"));
    }

    #[test]
    fn test_replication_config_with_replica_dirs() {
        let dirs = vec![PathBuf::from("/tmp/repl1"), PathBuf::from("/tmp/repl2")];
        let config = ReplicationConfig::with_replica_dirs(dirs.clone());
        assert_eq!(config.replica_dirs.len(), 2);
    }

    #[test]
    fn test_snapshot_pruning() {
        let tmp = TempDir::new().unwrap();
        let mut config = ReplicationConfig::new(tmp.path().to_path_buf());
        config.max_snapshots_per_replica = 2;

        // Create 4 snapshots
        for height in [100u64, 200, 300, 400] {
            let snapshot = make_snapshot(height);
            replicate_snapshot(&snapshot, &config).unwrap();
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

    #[test]
    fn test_replicate_to_multiple_dirs() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();

        let config = ReplicationConfig {
            replica_dirs: vec![tmp1.path().to_path_buf(), tmp2.path().to_path_buf()],
            ..Default::default()
        };

        let snapshot = make_snapshot(100);
        let path = replicate_snapshot(&snapshot, &config).unwrap();

        // Primary path should be in first dir
        assert!(path.exists());

        // Both dirs should have the snapshot
        let found1 = find_latest_in_dir(&tmp1.path().to_path_buf(), true)
            .unwrap()
            .unwrap();
        let found2 = find_latest_in_dir(&tmp2.path().to_path_buf(), true)
            .unwrap()
            .unwrap();
        assert_eq!(found1.height, 100);
        assert_eq!(found2.height, 100);
    }
}

//! Sled-to-Redb Migration Utility
//!
//! One-time migration utility that reads existing sled databases and converts
//! them to the new redb format. A node with existing sled data would lose all
//! persisted state on upgrade (including slashing records and nonce store)
//! without this migration.
//!
//! # Usage
//!
//! Enable the `migration` feature and call [`migrate_sled_to_redb`] on startup.
//! If a sled database exists at the configured data directory, all key-value
//! pairs are read from sled trees and written to the new redb database. The
//! sled directory is renamed to `<path>.sled.bak` after successful migration.

use std::path::Path;

#[cfg(feature = "migration")]
use redb::TableDefinition;

/// Result type for migration operations.
pub type MigrationResult<T> = Result<T, MigrationError>;

/// Errors that can occur during sled-to-redb migration.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    /// Failed to open the sled database.
    #[error("Failed to open sled database: {0}")]
    SledOpen(String),

    /// Failed to read from sled.
    #[error("Failed to read from sled: {0}")]
    SledRead(String),

    /// Failed to open the redb database.
    #[error("Failed to open redb database: {0}")]
    RedbOpen(String),

    /// Failed to write to redb.
    #[error("Failed to write to redb: {0}")]
    RedbWrite(String),

    /// Failed to rename sled backup directory.
    #[error("Failed to rename sled backup: {0}")]
    RenameFailed(String),

    /// I/O error during migration.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Migrate an existing sled database to redb.
///
/// This function:
/// 1. Checks if a sled database exists at `sled_path`
/// 2. If it does, reads all key-value pairs from sled trees
/// 3. Writes them to the redb database at `redb_path`
/// 4. Renames the sled directory to `<sled_path>.sled.bak` after success
///
/// Returns the number of key-value pairs migrated, or 0 if no sled database
/// was found.
///
/// # Arguments
///
/// * `sled_path` — Path to the sled database directory
/// * `redb_path` — Path to the redb database file
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::migration::migrate_sled_to_redb;
///
/// let migrated = migrate_sled_to_redb(
///     "/var/lib/omnia/sled",
///     "/var/lib/omnia/omnia.redb",
/// ).expect("migration failed");
///
/// if migrated > 0 {
///     tracing::info!(migrated, "Successfully migrated sled data to redb");
/// }
/// ```
#[cfg(feature = "migration")]
pub fn migrate_sled_to_redb(sled_path: &Path, redb_path: &Path) -> MigrationResult<usize> {
    // Check if sled database exists
    if !sled_path.exists() {
        tracing::debug!(path = %sled_path.display(), "No sled database found, skipping migration");
        return Ok(0);
    }

    tracing::info!(
        sled_path = %sled_path.display(),
        redb_path = %redb_path.display(),
        "Starting sled-to-redb migration"
    );

    // Open sled database (read-only)
    let sled_db = sled::open(sled_path).map_err(|e| MigrationError::SledOpen(e.to_string()))?;

    let mut total_migrated = 0usize;

    // Open redb database
    let redb_db = redb::Database::create(redb_path).map_err(|e| MigrationError::RedbOpen(e.to_string()))?;

    // Iterate over all sled trees
    let tree_names: Vec<String> = sled_db
        .tree_names()
        .into_iter()
        .map(|name| String::from_utf8_lossy(name.as_ref()).to_string())
        .collect();

    tracing::info!(trees = tree_names.len(), "Found sled trees to migrate");

    for tree_name in &tree_names {
        let tree = sled_db
            .open_tree(tree_name)
            .map_err(|e| MigrationError::SledRead(e.to_string()))?;

        // Create a redb table definition for this tree
        let table_key = format!("migrated_{}", tree_name.replace(|c: char| !c.is_alphanumeric(), "_"));
        let table_def: TableDefinition<&[u8], &[u8]> = TableDefinition::new(&table_key);

        let write_txn = redb_db
            .begin_write()
            .map_err(|e| MigrationError::RedbWrite(e.to_string()))?;

        {
            let mut table = write_txn
                .open_table(table_def)
                .map_err(|e| MigrationError::RedbWrite(e.to_string()))?;

            let mut tree_count = 0usize;
            for item in tree.iter() {
                let (key, value) = item.map_err(|e| MigrationError::SledRead(e.to_string()))?;
                let key_bytes: &[u8] = &key;
                let value_bytes: &[u8] = &value;

                table
                    .insert(key_bytes, value_bytes)
                    .map_err(|e| MigrationError::RedbWrite(e.to_string()))?;

                tree_count += 1;
            }

            total_migrated += tree_count;
            tracing::info!(
                tree = tree_name,
                table = %table_key,
                entries = tree_count,
                "Migrated sled tree to redb table"
            );
        }

        write_txn
            .commit()
            .map_err(|e| MigrationError::RedbWrite(e.to_string()))?;
    }

    // Close sled database
    drop(sled_db);

    // Rename sled directory to .sled.bak
    let backup_path = {
        let base = sled_path.to_string_lossy().to_string();
        format!("{base}.sled.bak")
    };
    let backup_path = Path::new(&backup_path);

    std::fs::rename(sled_path, backup_path).map_err(|e| MigrationError::RenameFailed(e.to_string()))?;

    tracing::info!(
        total_migrated,
        backup = %backup_path.display(),
        "Migration complete — sled directory backed up"
    );

    Ok(total_migrated)
}

/// Stub function when migration feature is not enabled.
///
/// Returns 0 (no migration performed) and logs that the migration feature
/// is not enabled.
#[cfg(not(feature = "migration"))]
pub fn migrate_sled_to_redb(sled_path: &Path, _redb_path: &Path) -> MigrationResult<usize> {
    if sled_path.exists() {
        tracing::warn!(
            path = %sled_path.display(),
            "Sled database found but migration feature is not enabled. \
             Enable the 'migration' feature to migrate sled data to redb."
        );
    }
    Ok(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_no_sled_database_returns_zero() {
        let dir = TempDir::new().unwrap();
        let sled_path = dir.path().join("nonexistent_sled");
        let redb_path = dir.path().join("omnia.redb");

        let result = migrate_sled_to_redb(&sled_path, &redb_path).unwrap();
        assert_eq!(result, 0);
    }

    #[cfg(feature = "migration")]
    #[test]
    fn test_migrate_empty_sled_database() {
        let dir = TempDir::new().unwrap();
        let sled_path = dir.path().join("sled_db");
        let redb_path = dir.path().join("omnia.redb");

        // Create an empty sled database
        let db = sled::open(&sled_path).unwrap();
        drop(db);

        let result = migrate_sled_to_redb(&sled_path, &redb_path).unwrap();
        assert_eq!(result, 0);

        // Sled directory should be renamed
        assert!(!sled_path.exists());
        let backup = format!("{}.sled.bak", sled_path.display());
        assert!(Path::new(&backup).exists());
    }

    #[cfg(feature = "migration")]
    #[test]
    fn test_migrate_sled_with_data() {
        let dir = TempDir::new().unwrap();
        let sled_path = dir.path().join("sled_db");
        let redb_path = dir.path().join("omnia.redb");

        // Create sled database with data
        let db = sled::open(&sled_path).unwrap();
        let tree = db.open_tree("test_tree").unwrap();
        tree.insert(b"key1", b"value1").unwrap();
        tree.insert(b"key2", b"value2").unwrap();
        tree.insert(b"key3", b"value3").unwrap();
        drop(tree);
        drop(db);

        let result = migrate_sled_to_redb(&sled_path, &redb_path).unwrap();
        assert_eq!(result, 3);

        // Verify redb database was created
        assert!(redb_path.exists());
    }
}

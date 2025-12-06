//! Snapshot service for managing Iceberg table snapshots
//!
//! This service handles snapshot operations for Iceberg tables:
//! - Listing snapshots
//! - Expiring snapshots
//! - Time-travel (setting current snapshot)
//! - Creating metadata backups
//! - Cherry-pick (stub for future implementation)

use std::sync::Arc;

use chrono::{DateTime, Utc};
use iceberg::spec::{MAIN_BRANCH, SnapshotReference, SnapshotRetention, TableMetadata};

use crate::core::metadata::IcebergMetadataService;
use crate::core::storage::StorageBackendFactory;
use crate::core::storage::traits::PutOptions;
use crate::core::utils::snapshot::{
    ExpirationConfig, SnapshotItem, determine_cutoff_timestamp, determine_snapshots_to_expire,
};
use crate::error::{Error, Result};
use crate::utils::parse_timestamp;

/// Information about a snapshot for display purposes
#[derive(Debug, Clone)]
pub struct SnapshotDetails {
    /// Snapshot ID
    pub id: i64,
    /// Timestamp when the snapshot was created
    pub timestamp: Option<DateTime<Utc>>,
    /// Parent snapshot ID (if any)
    pub parent_id: Option<i64>,
    /// Whether this is the current snapshot
    pub is_current: bool,
    /// Operation that created this snapshot (append, overwrite, replace, delete, etc.)
    pub operation: Option<String>,
}

impl SnapshotDetails {
    /// Create from an iceberg Snapshot
    pub fn from_iceberg(
        snapshot: &iceberg::spec::Snapshot,
        current_snapshot_id: Option<i64>,
    ) -> Self {
        use iceberg::spec::Operation;

        let operation = match snapshot.summary().operation {
            Operation::Append => "append",
            Operation::Replace => "replace",
            Operation::Overwrite => "overwrite",
            Operation::Delete => "delete",
        };

        Self {
            id: snapshot.snapshot_id(),
            timestamp: DateTime::from_timestamp_millis(snapshot.timestamp_ms()),
            parent_id: snapshot.parent_snapshot_id(),
            is_current: Some(snapshot.snapshot_id()) == current_snapshot_id,
            operation: Some(operation.to_string()),
        }
    }
}

/// Result of listing snapshots
#[derive(Debug)]
pub struct ListSnapshotsResult {
    /// List of snapshot details
    pub snapshots: Vec<SnapshotDetails>,
    /// Total number of snapshots in the table
    pub total_count: usize,
}

/// Result of expiring snapshots
#[derive(Debug)]
pub struct ExpireSnapshotsResult {
    /// Number of snapshots expired
    pub expired_count: usize,
    /// IDs of expired snapshots
    pub expired_ids: Vec<i64>,
    /// Cutoff timestamp used for expiration
    pub cutoff_timestamp: DateTime<Utc>,
    /// New metadata version (if changes were made)
    pub new_version: Option<i64>,
    /// Whether this was a dry run
    pub dry_run: bool,
}

/// Result of setting current snapshot (time-travel)
#[derive(Debug)]
pub struct SetSnapshotResult {
    /// Previous current snapshot ID
    pub previous_id: Option<i64>,
    /// New current snapshot ID
    pub current_id: i64,
    /// New metadata version
    pub new_version: Option<i64>,
    /// Whether this was a dry run
    pub dry_run: bool,
}

/// Result of creating a metadata backup
#[derive(Debug)]
pub struct CreateBackupResult {
    /// Metadata version that was backed up
    pub version: i64,
    /// Path to the backup file
    pub backup_path: String,
    /// Size of the backup in bytes
    pub size_bytes: u64,
}

/// Configuration for the snapshot service
#[derive(Debug, Clone, Default)]
pub struct SnapshotConfig {
    /// Whether to run in dry-run mode
    pub dry_run: bool,
}

/// Service for managing Iceberg table snapshots
pub struct SnapshotService {
    config: SnapshotConfig,
}

impl SnapshotService {
    /// Create a new snapshot service with default configuration
    pub fn new() -> Self {
        Self {
            config: SnapshotConfig::default(),
        }
    }

    /// Create a new snapshot service with custom configuration
    pub fn with_config(config: SnapshotConfig) -> Self {
        Self { config }
    }

    /// List snapshots from an Iceberg table
    ///
    /// Returns snapshots sorted by timestamp descending (most recent first).
    pub async fn list_snapshots(
        &self,
        service: &IcebergMetadataService,
        limit: Option<usize>,
    ) -> Result<ListSnapshotsResult> {
        let (metadata, _) = service.load_metadata().await?;

        let mut snapshots: Vec<_> = metadata.snapshots().collect();
        let total_count = snapshots.len();

        // Sort by timestamp descending (most recent first)
        #[allow(clippy::unnecessary_sort_by)]
        snapshots.sort_by(|a, b| b.timestamp_ms().cmp(&a.timestamp_ms()));

        let current_snapshot_id = metadata.current_snapshot_id();

        let limit = limit.unwrap_or(snapshots.len()).min(snapshots.len());

        let snapshot_details: Vec<SnapshotDetails> = snapshots
            .iter()
            .take(limit)
            .map(|snap| SnapshotDetails::from_iceberg(snap, current_snapshot_id))
            .collect();

        Ok(ListSnapshotsResult {
            snapshots: snapshot_details,
            total_count,
        })
    }

    /// Expire snapshots based on configuration
    ///
    /// This removes snapshots from metadata but does NOT delete data files.
    /// Use `icetable vacuum` to remove orphaned data files after expiring snapshots.
    pub async fn expire_snapshots(
        &self,
        service: &IcebergMetadataService,
        table_path: &str,
        older_than: Option<String>,
        retain_last: Option<usize>,
        ids: Option<Vec<i64>>,
    ) -> Result<ExpireSnapshotsResult> {
        let (metadata, current_version) = service.load_metadata().await?;

        let snapshots: Vec<_> = metadata.snapshots().collect();

        // Get the current snapshot ID for the target branch
        // This ensures we protect the branch's current snapshot from expiration
        let target_branch = service.target_branch();
        let current_id = if target_branch == "main" {
            metadata.current_snapshot_id()
        } else {
            metadata.snapshot_for_ref(target_branch).map(|s| s.snapshot_id())
        };

        let to_expire = self.determine_snapshots_to_expire(
            &snapshots,
            older_than.clone(),
            retain_last,
            ids,
            current_id,
        )?;

        let cutoff_timestamp =
            determine_cutoff_timestamp(&snapshots, older_than.as_deref(), retain_last)?;

        if to_expire.is_empty() {
            return Ok(ExpireSnapshotsResult {
                expired_count: 0,
                expired_ids: Vec::new(),
                cutoff_timestamp,
                new_version: None,
                dry_run: self.config.dry_run,
            });
        }

        if self.config.dry_run {
            return Ok(ExpireSnapshotsResult {
                expired_count: to_expire.len(),
                expired_ids: to_expire,
                cutoff_timestamp,
                new_version: None,
                dry_run: true,
            });
        }

        // Build new metadata with snapshots removed
        let metadata_file_path = service.current_metadata_path().await?;
        let metadata_clone = (*metadata).clone();
        let build_result = metadata_clone
            .into_builder(Some(metadata_file_path.clone()))
            .remove_snapshots(&to_expire)
            .build()
            .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        let new_metadata = build_result.metadata;

        // Write new metadata
        let new_version = self
            .write_metadata(table_path, &new_metadata, current_version)
            .await?;

        Ok(ExpireSnapshotsResult {
            expired_count: to_expire.len(),
            expired_ids: to_expire,
            cutoff_timestamp,
            new_version: Some(new_version),
            dry_run: false,
        })
    }

    /// Set the current snapshot (time-travel)
    ///
    /// Changes the table's current snapshot to an existing snapshot,
    /// either by ID, timestamp (as-of), branch name, or tag name.
    pub async fn set_current_snapshot(
        &self,
        service: &IcebergMetadataService,
        table_path: &str,
        id: Option<i64>,
        as_of: Option<String>,
        branch: Option<String>,
        tag: Option<String>,
    ) -> Result<SetSnapshotResult> {
        let (metadata, current_version) = service.load_metadata().await?;

        let target_id = self.resolve_target_snapshot(&metadata, id, as_of, branch, tag)?;

        // Verify snapshot exists
        let _ = metadata
            .snapshot_by_id(target_id)
            .ok_or_else(|| Error::General(format!("Snapshot {} not found", target_id)))?;

        let previous_id = metadata.current_snapshot_id();

        if self.config.dry_run {
            return Ok(SetSnapshotResult {
                previous_id,
                current_id: target_id,
                new_version: None,
                dry_run: true,
            });
        }

        // Build new metadata with target snapshot as current
        let metadata_file_path = service.current_metadata_path().await?;
        let metadata_clone = (*metadata).clone();

        let branch_ref = SnapshotReference {
            snapshot_id: target_id,
            retention: SnapshotRetention::Branch {
                min_snapshots_to_keep: None,
                max_snapshot_age_ms: None,
                max_ref_age_ms: None,
            },
        };

        let build_result = metadata_clone
            .into_builder(Some(metadata_file_path.clone()))
            .set_ref(MAIN_BRANCH, branch_ref)
            .map_err(|e| Error::General(format!("Failed to set snapshot: {}", e)))?
            .build()
            .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        let new_metadata = build_result.metadata;

        // Write new metadata
        let new_version = self
            .write_metadata(table_path, &new_metadata, current_version)
            .await?;

        Ok(SetSnapshotResult {
            previous_id,
            current_id: target_id,
            new_version: Some(new_version),
            dry_run: false,
        })
    }

    /// Create a backup of the current metadata
    ///
    /// This creates a timestamped backup of the current metadata file,
    /// which can be useful before destructive operations.
    pub async fn create_metadata_backup(&self, table_path: &str) -> Result<CreateBackupResult> {
        let table_path_obj = std::path::Path::new(table_path);
        let metadata_dir = table_path_obj.join("metadata");

        // Get current version
        let version_hint = metadata_dir.join("version-hint.text");
        let current_version: i32 = std::fs::read_to_string(&version_hint)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);

        let metadata_file = metadata_dir.join(format!("v{}.metadata.json", current_version));

        if !metadata_file.exists() {
            return Err(Error::General(format!(
                "Metadata file not found: {}",
                metadata_file.display()
            )));
        }

        // Create backup with timestamp
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let backup_file = metadata_dir.join(format!(
            "v{}.metadata.{}.backup.json",
            current_version, timestamp
        ));

        std::fs::copy(&metadata_file, &backup_file)
            .map_err(|e| Error::General(format!("Failed to create backup: {}", e)))?;

        let backup_size = std::fs::metadata(&backup_file)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(CreateBackupResult {
            version: current_version as i64,
            backup_path: backup_file.to_string_lossy().to_string(),
            size_bytes: backup_size,
        })
    }

    /// Cherry-pick a snapshot (stub - not yet implemented)
    ///
    /// Cherry-pick in Iceberg creates a new snapshot that applies changes
    /// from a specific snapshot onto the current table state.
    pub async fn cherry_pick_snapshot(&self, _table_path: &str, snapshot_id: i64) -> Result<()> {
        // FUTURE: Implement cherry-pick using iceberg-rs Transaction API
        // Cherry-pick in Iceberg creates a new snapshot that applies the changes
        // from a specific snapshot onto the current table state. This requires:
        // 1. Reading the manifest files from the source snapshot
        // 2. Applying those data file additions/deletions to current state
        // 3. Creating a new snapshot with the merged changes
        //
        // This is a complex operation that needs careful handling of:
        // - Conflict detection with current snapshot
        // - Schema compatibility validation
        // - Partition spec compatibility
        //
        // For now, users should use catalog tools (e.g., Spark, Trino) for cherry-pick.
        Err(Error::General(format!(
            "Cherry-pick is not yet implemented. Snapshot {} cannot be cherry-picked.\n\
             This feature requires the iceberg-rs Transaction API.\n\
             Workaround: Use Spark or Trino SQL: \
             CALL system.cherrypick_snapshot('table', {})",
            snapshot_id, snapshot_id
        )))
    }

    /// Find snapshot at or before a given timestamp
    pub fn find_snapshot_at_timestamp(
        &self,
        metadata: &Arc<TableMetadata>,
        timestamp: &str,
    ) -> Result<i64> {
        let cutoff = parse_timestamp(timestamp)?;
        let cutoff_ms = cutoff.timestamp_millis();

        let snapshots: Vec<_> = metadata.snapshots().collect();
        let mut best: Option<i64> = None;
        let mut best_ts = i64::MIN;

        for snap in &snapshots {
            if snap.timestamp_ms() <= cutoff_ms && snap.timestamp_ms() > best_ts {
                best = Some(snap.snapshot_id());
                best_ts = snap.timestamp_ms();
            }
        }

        best.ok_or_else(|| Error::General(format!("No snapshot found before {}", timestamp)))
    }

    // =========================================================================
    // Private helper methods
    // =========================================================================

    /// Determine which snapshots to expire based on provided criteria
    fn determine_snapshots_to_expire<I>(
        &self,
        snapshots: &[I],
        older_than: Option<String>,
        retain_last: Option<usize>,
        ids: Option<Vec<i64>>,
        current_id: Option<i64>,
    ) -> Result<Vec<i64>>
    where
        I: SnapshotItem,
    {
        if let Some(explicit_ids) = ids {
            // Filter out current snapshot and non-existent IDs
            let mut valid_ids = Vec::new();
            for id in explicit_ids {
                if Some(id) == current_id {
                    continue; // Skip current snapshot
                }
                if snapshots.iter().any(|s| s.id() == id) {
                    valid_ids.push(id);
                }
            }

            if valid_ids.is_empty() {
                return Ok(Vec::new());
            }

            let config = ExpirationConfig {
                ids: Some(valid_ids),
                skip_current: true,
                ..Default::default()
            };
            determine_snapshots_to_expire(snapshots, &config, current_id)
        } else {
            let config = ExpirationConfig {
                older_than,
                retain_last,
                ids: None,
                skip_current: true,
            };
            determine_snapshots_to_expire(snapshots, &config, current_id)
        }
    }

    /// Resolve target snapshot ID from direct ID, as-of timestamp, branch, or tag
    fn resolve_target_snapshot(
        &self,
        metadata: &Arc<TableMetadata>,
        id: Option<i64>,
        as_of: Option<String>,
        branch: Option<String>,
        tag: Option<String>,
    ) -> Result<i64> {
        match (id, as_of, branch, tag) {
            (Some(id), _, _, _) => Ok(id),
            (None, Some(timestamp), _, _) => self.find_snapshot_at_timestamp(metadata, &timestamp),
            (None, None, Some(branch_name), _) => {
                metadata
                    .snapshot_for_ref(&branch_name)
                    .map(|s| s.snapshot_id())
                    .ok_or_else(|| Error::General(format!("Branch '{}' not found", branch_name)))
            }
            (None, None, None, Some(tag_name)) => {
                metadata
                    .snapshot_for_ref(&tag_name)
                    .map(|s| s.snapshot_id())
                    .ok_or_else(|| Error::General(format!("Tag '{}' not found", tag_name)))
            }
            (None, None, None, None) => Err(Error::General(
                "Must specify --id, --as-of, --branch, or --tag".to_string(),
            )),
        }
    }

    /// Write new metadata file and update version hint
    async fn write_metadata(
        &self,
        table_path: &str,
        metadata: &TableMetadata,
        current_version: i32,
    ) -> Result<i64> {
        let storage = StorageBackendFactory::create_backend(table_path).await?;
        let metadata_dir = format!("{}/metadata", table_path.trim_end_matches('/'));
        let new_version = (current_version + 1) as i64;
        let new_metadata_filename = format!("v{}.metadata.json", new_version);
        let new_metadata_path = format!("{}/{}", metadata_dir, new_metadata_filename);

        let new_metadata_bytes = serde_json::to_vec_pretty(metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        storage
            .put(
                &new_metadata_path,
                bytes::Bytes::from(new_metadata_bytes),
                &PutOptions::default(),
            )
            .await?;

        // Update version-hint.text
        let version_hint_path = format!("{}/version-hint.text", metadata_dir);
        storage
            .put(
                &version_hint_path,
                bytes::Bytes::from(new_version.to_string()),
                &PutOptions::default(),
            )
            .await?;

        Ok(new_version)
    }
}

impl Default for SnapshotService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SnapshotConfig::default();
        assert!(!config.dry_run);
    }

    #[test]
    fn test_service_creation() {
        let service = SnapshotService::new();
        assert!(!service.config.dry_run);

        let config = SnapshotConfig { dry_run: true };
        let service_with_config = SnapshotService::with_config(config);
        assert!(service_with_config.config.dry_run);
    }
}

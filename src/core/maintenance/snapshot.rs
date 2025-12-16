//! Snapshot service for managing Iceberg table snapshots
//!
//! This service handles snapshot operations for Iceberg tables:
//! - Listing snapshots
//! - Expiring snapshots
//! - Time-travel (setting current snapshot)
//! - Creating metadata backups
//! - Cherry-pick (stub for future implementation)
//!
//! Uses `MetadataServiceReader` for read operations and `MetadataServiceWriter`
//! for write operations that commit via catalog.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use iceberg::spec::{MAIN_BRANCH, SnapshotReference, SnapshotRetention, TableMetadata};

use crate::core::metadata::{MetadataServiceReader, MetadataServiceWriter};
use crate::error::{Error, Result};
use crate::utils::core::snapshot::{
    ExpirationConfig, SnapshotItem, determine_cutoff_timestamp, determine_snapshots_to_expire,
};
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

/// Entry in a snapshot lineage chain
#[derive(Debug, Clone)]
pub struct LineageEntry {
    /// Snapshot ID
    pub snapshot_id: i64,
    /// Parent snapshot ID (None for root)
    pub parent_id: Option<i64>,
    /// Timestamp when the snapshot was created (milliseconds since epoch)
    pub timestamp_ms: i64,
    /// Operation that created this snapshot
    pub operation: String,
    /// Whether this is the current snapshot
    pub is_current: bool,
    /// Whether this is the root snapshot (no parent)
    pub is_root: bool,
}

/// Result of getting snapshot lineage
#[derive(Debug)]
pub struct LineageResult {
    /// Lineage entries from start to root
    pub entries: Vec<LineageEntry>,
    /// Total count of snapshots in the lineage
    pub total_count: usize,
    /// ID of the current snapshot
    pub current_snapshot_id: Option<i64>,
}

/// Configuration for the snapshot service
#[derive(Debug, Clone, Default)]
pub struct SnapshotConfig {
    /// Whether to run in dry-run mode
    pub dry_run: bool,
}

/// Service for managing Iceberg table snapshots
///
/// Uses `MetadataServiceReader` for read operations and `MetadataServiceWriter`
/// for write operations. The committer comes from the service, not stored in the struct.
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
    ///
    /// Uses `MetadataServiceReader` trait to access table metadata.
    pub async fn list_snapshots<S: MetadataServiceReader>(
        &self,
        service: &S,
        limit: Option<usize>,
    ) -> Result<ListSnapshotsResult> {
        let (metadata, _) = service.load_metadata().await?;

        let mut snapshots: Vec<_> = metadata.snapshots().collect();
        let total_count = snapshots.len();

        // Sort by timestamp descending (most recent first)
        snapshots.sort_by_key(|s| std::cmp::Reverse(s.timestamp_ms()));

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
    ///
    /// Uses `MetadataServiceWriter` trait to access metadata and commit changes.
    pub async fn expire_snapshots<S: MetadataServiceWriter>(
        &self,
        service: &S,
        older_than: Option<String>,
        retain_last: Option<usize>,
        ids: Option<Vec<i64>>,
    ) -> Result<ExpireSnapshotsResult> {
        let (metadata, current_version) = service.load_metadata().await?;
        let table_path = service.table_path();

        let snapshots: Vec<_> = metadata.snapshots().collect();

        // Get the current snapshot ID for the target branch
        // This ensures we protect the branch's current snapshot from expiration
        let target_branch = service.target_branch();
        let current_id = if target_branch == "main" {
            metadata.current_snapshot_id()
        } else {
            metadata
                .snapshot_for_ref(target_branch)
                .map(|s| s.snapshot_id())
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

        // Commit the snapshot removal
        let new_version = if let Some(committer) = service.committer() {
            // Use catalog committer for multi-writer safety
            committer
                .commit_remove_snapshots(table_path, &metadata, &to_expire, current_version)
                .await?
        } else {
            // Direct write to storage (single-writer mode)
            let metadata_file_path = service.current_metadata_path().await?;
            let metadata_clone = (*metadata).clone();
            let build_result = metadata_clone
                .into_builder(Some(metadata_file_path.clone()))
                .remove_snapshots(&to_expire)
                .build()
                .map_err(|e| Error::Metadata {
                    message: format!("Failed to build metadata: {}", e),
                })?;

            let new_metadata = build_result.metadata;

            super::write_metadata_direct(table_path, &new_metadata).await?
        };

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
    ///
    /// Uses `MetadataServiceWriter` trait to access metadata and commit changes.
    pub async fn set_current_snapshot<S: MetadataServiceWriter>(
        &self,
        service: &S,
        id: Option<i64>,
        as_of: Option<String>,
        branch: Option<String>,
        tag: Option<String>,
    ) -> Result<SetSnapshotResult> {
        let (metadata, current_version) = service.load_metadata().await?;
        let table_path = service.table_path();

        let target_id = self.resolve_target_snapshot(&metadata, id, as_of, branch, tag)?;

        // Verify snapshot exists
        let _ = metadata
            .snapshot_by_id(target_id)
            .ok_or_else(|| Error::SnapshotNotFound {
                snapshot_id: target_id,
            })?;

        let previous_id = metadata.current_snapshot_id();

        if self.config.dry_run {
            return Ok(SetSnapshotResult {
                previous_id,
                current_id: target_id,
                new_version: None,
                dry_run: true,
            });
        }

        // Commit the snapshot ref change
        let new_version = if let Some(committer) = service.committer() {
            // Use catalog committer for multi-writer safety
            committer
                .commit_set_snapshot_ref(
                    table_path,
                    &metadata,
                    MAIN_BRANCH,
                    target_id,
                    current_version,
                )
                .await?
        } else {
            // Direct write to storage (single-writer mode)
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
                .map_err(|e| Error::Metadata {
                    message: format!("Failed to set snapshot: {}", e),
                })?
                .build()
                .map_err(|e| Error::Metadata {
                    message: format!("Failed to build metadata: {}", e),
                })?;

            let new_metadata = build_result.metadata;

            super::write_metadata_direct(table_path, &new_metadata).await?
        };

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
        use crate::core::storage::create_object_store;
        use crate::utils::core::{extract_version_from_path, find_latest_metadata};

        let storage = create_object_store(table_path).await?;

        // Find the current metadata file using standard format
        let current_metadata_path = find_latest_metadata(table_path, &storage).await?;
        let current_version = extract_version_from_path(&current_metadata_path).unwrap_or(0);

        let metadata_filename = current_metadata_path
            .split('/')
            .next_back()
            .unwrap_or(&current_metadata_path);

        // For local storage, do filesystem backup
        let table_path_obj = std::path::Path::new(table_path);
        let metadata_dir = table_path_obj.join("metadata");
        let metadata_file = metadata_dir.join(metadata_filename);

        if !metadata_file.exists() {
            return Err(Error::FileNotFound {
                path: metadata_file.clone(),
            });
        }

        // Create backup with timestamp
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let backup_file = metadata_dir.join(format!("{}.{}.backup", metadata_filename, timestamp));

        std::fs::copy(&metadata_file, &backup_file).map_err(|e| Error::Storage {
            message: format!("Failed to create backup: {}", e),
        })?;

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
        Err(Error::UnsupportedFeature {
            feature: format!(
                "Cherry-pick is not yet implemented. Snapshot {} cannot be cherry-picked. \
                 This feature requires the iceberg-rs Transaction API. \
                 Workaround: Use Spark or Trino SQL: \
                 CALL system.cherrypick_snapshot('table', {})",
                snapshot_id, snapshot_id
            ),
        })
    }

    /// Get the lineage of a snapshot (chain of ancestors to root)
    ///
    /// Returns the lineage from the specified snapshot (or current if none specified)
    /// back to the root snapshot.
    ///
    /// Uses `MetadataServiceReader` trait to access table metadata.
    pub async fn get_lineage<S: MetadataServiceReader>(
        &self,
        service: &S,
        snapshot_id: Option<i64>,
    ) -> Result<LineageResult> {
        use std::collections::HashMap;

        let (metadata, _) = service.load_metadata().await?;
        let current_id = metadata.current_snapshot_id();

        // Get starting snapshot
        let start_id = snapshot_id
            .or(current_id)
            .ok_or_else(|| Error::MissingArgument {
                argument: "snapshot_id".to_string(),
                description: "No snapshot specified and table has no current snapshot".to_string(),
            })?;

        // Build parent map for quick lookup
        let parent_map: HashMap<i64, Option<i64>> = metadata
            .snapshots()
            .map(|s| (s.snapshot_id(), s.parent_snapshot_id()))
            .collect();

        // Build snapshot info map
        let snapshot_map: HashMap<i64, _> =
            metadata.snapshots().map(|s| (s.snapshot_id(), s)).collect();

        // Walk the lineage from start to root
        let mut entries = Vec::new();
        let mut current = Some(start_id);

        while let Some(id) = current {
            let parent = parent_map.get(&id).copied().flatten();
            let (timestamp_ms, operation) = snapshot_map
                .get(&id)
                .map(|s| {
                    let ts = s.timestamp_ms();
                    let op = format!("{:?}", s.summary().operation).to_lowercase();
                    (ts, op)
                })
                .unwrap_or((0, "unknown".to_string()));

            entries.push(LineageEntry {
                snapshot_id: id,
                parent_id: parent,
                timestamp_ms,
                operation,
                is_current: Some(id) == current_id,
                is_root: parent.is_none(),
            });

            current = parent;
        }

        let total_count = entries.len();

        Ok(LineageResult {
            entries,
            total_count,
            current_snapshot_id: current_id,
        })
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

        best.ok_or_else(|| Error::Metadata {
            message: format!("No snapshot found before {}", timestamp),
        })
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
            (None, None, Some(branch_name), _) => metadata
                .snapshot_for_ref(&branch_name)
                .map(|s| s.snapshot_id())
                .ok_or_else(|| Error::Metadata {
                    message: format!("Branch '{}' not found", branch_name),
                }),
            (None, None, None, Some(tag_name)) => metadata
                .snapshot_for_ref(&tag_name)
                .map(|s| s.snapshot_id())
                .ok_or_else(|| Error::Metadata {
                    message: format!("Tag '{}' not found", tag_name),
                }),
            (None, None, None, None) => Err(Error::MissingArgument {
                argument: "id, as-of, branch, or tag".to_string(),
                description: "Must specify --id, --as-of, --branch, or --tag".to_string(),
            }),
        }
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

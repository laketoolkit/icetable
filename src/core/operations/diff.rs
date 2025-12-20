//! Diff service for comparing snapshots within a table
//!
//! Provides functionality to compare snapshots, branches, or tags
//! within the same Iceberg table. Compares schema, partitions modified,
//! and data files between two points in time.

use std::collections::HashSet;
use std::sync::Arc;

use iceberg::spec::ManifestStatus;

use crate::core::metadata::IcebergMetadataService;
use crate::core::{Snapshot, TableMetadata};
use crate::error::{Error, Result};

/// Configuration for diff operation
#[derive(Debug, Clone, Default)]
pub struct DiffConfig {
    /// From reference (snapshot ID, branch, or tag). Defaults to parent of current.
    pub from: Option<String>,
    /// To reference (snapshot ID, branch, or tag). Defaults to current snapshot.
    pub to: Option<String>,
}

/// Result of comparing two snapshots
#[derive(Debug, Clone)]
pub struct SnapshotDiffResult {
    /// From snapshot (older)
    pub from: SnapshotRef,
    /// To snapshot (newer)
    pub to: SnapshotRef,
    /// Whether both references point to the same snapshot
    pub is_identical: bool,
    /// Schema changes between snapshots
    pub schema: SchemaDiff,
    /// Partitions that were modified
    pub partitions_modified: Vec<String>,
    /// Data files diff (added vs removed)
    pub data_files: DataFileDiff,
}

/// Schema changes between snapshots
#[derive(Debug, Clone, Default)]
pub struct SchemaDiff {
    /// Columns added (name, type)
    pub columns_added: Vec<(String, String)>,
    /// Columns removed (name, type)
    pub columns_removed: Vec<(String, String)>,
}

impl SchemaDiff {
    /// Returns true if there are no schema changes
    pub fn is_empty(&self) -> bool {
        self.columns_added.is_empty() && self.columns_removed.is_empty()
    }
}

/// Summary of data file changes between snapshots
#[derive(Debug, Clone, Default)]
pub struct DataFileDiff {
    /// Number of data files added
    pub files_added: usize,
    /// Number of data files removed
    pub files_removed: usize,
    /// Total bytes added
    pub bytes_added: u64,
    /// Total bytes removed
    pub bytes_removed: u64,
}

/// Information about a snapshot reference
#[derive(Debug, Clone)]
pub struct SnapshotRef {
    /// Label (branch name, tag name, or "current"/"parent")
    pub label: String,
    /// Snapshot ID
    pub snapshot_id: i64,
    /// Timestamp in milliseconds
    pub timestamp_ms: i64,
}

/// Service for comparing snapshots within a table
pub struct DiffService;

impl DiffService {
    /// Compare two snapshots within the same table
    ///
    /// Behavior:
    /// - `diff` → current vs parent
    /// - `diff --from <ref>` → from vs current
    /// - `diff --from <ref> --to <ref>` → from vs to
    pub async fn compare_snapshots(
        service: &IcebergMetadataService,
        config: &DiffConfig,
    ) -> Result<SnapshotDiffResult> {
        let (metadata, _) = service.load_metadata().await?;

        let current_id = metadata
            .current_snapshot_id()
            .ok_or_else(|| Error::DataValidation {
                message: "Table has no current snapshot".to_string(),
            })?;

        // Resolve to (defaults to current)
        let to_id = if let Some(ref to_ref) = config.to {
            Self::resolve_ref(&metadata, to_ref)?
        } else {
            current_id
        };

        // Resolve from (defaults to parent of 'to')
        let from_id = if let Some(ref from_ref) = config.from {
            Self::resolve_ref(&metadata, from_ref)?
        } else {
            let to_snapshot = metadata
                .snapshot_by_id(to_id)
                .ok_or(Error::SnapshotNotFound { snapshot_id: to_id })?;
            to_snapshot
                .parent_snapshot_id()
                .ok_or_else(|| Error::DataValidation {
                    message: "No parent snapshot. Specify --from reference.".to_string(),
                })?
        };

        // Check if they're the same
        if from_id == to_id {
            return Ok(SnapshotDiffResult {
                from: SnapshotRef {
                    label: config.from.clone().unwrap_or_else(|| "parent".to_string()),
                    snapshot_id: from_id,
                    timestamp_ms: 0,
                },
                to: SnapshotRef {
                    label: config.to.clone().unwrap_or_else(|| "current".to_string()),
                    snapshot_id: to_id,
                    timestamp_ms: 0,
                },
                is_identical: true,
                schema: SchemaDiff::default(),
                partitions_modified: Vec::new(),
                data_files: DataFileDiff::default(),
            });
        }

        // Get snapshots
        let from_snapshot = metadata
            .snapshot_by_id(from_id)
            .ok_or(Error::SnapshotNotFound {
                snapshot_id: from_id,
            })?;
        let to_snapshot = metadata
            .snapshot_by_id(to_id)
            .ok_or(Error::SnapshotNotFound {
                snapshot_id: to_id,
            })?;

        // Compare schemas
        let schema_diff = Self::compare_schemas(&metadata, from_snapshot, to_snapshot);

        // Get data files for both snapshots
        let from_files = Self::get_data_files(service, from_snapshot).await?;
        let to_files = Self::get_data_files(service, to_snapshot).await?;

        // Build sets for comparison (by path)
        let from_paths: HashSet<_> = from_files.iter().map(|(path, _)| path.as_str()).collect();
        let to_paths: HashSet<_> = to_files.iter().map(|(path, _)| path.as_str()).collect();

        // Calculate file diff and collect modified partitions
        let mut files_added = 0usize;
        let mut bytes_added = 0u64;
        let mut partitions: HashSet<String> = HashSet::new();

        for (path, size) in &to_files {
            if !from_paths.contains(path.as_str()) {
                files_added += 1;
                bytes_added += size;
                if let Some(partition) = extract_partition_from_path(path) {
                    partitions.insert(partition);
                }
            }
        }

        let mut files_removed = 0usize;
        let mut bytes_removed = 0u64;
        for (path, size) in &from_files {
            if !to_paths.contains(path.as_str()) {
                files_removed += 1;
                bytes_removed += size;
                if let Some(partition) = extract_partition_from_path(path) {
                    partitions.insert(partition);
                }
            }
        }

        // Sort partitions for consistent output
        let mut partitions_modified: Vec<_> = partitions.into_iter().collect();
        partitions_modified.sort();

        Ok(SnapshotDiffResult {
            from: SnapshotRef {
                label: config.from.clone().unwrap_or_else(|| "parent".to_string()),
                snapshot_id: from_id,
                timestamp_ms: from_snapshot.timestamp_ms(),
            },
            to: SnapshotRef {
                label: config.to.clone().unwrap_or_else(|| "current".to_string()),
                snapshot_id: to_id,
                timestamp_ms: to_snapshot.timestamp_ms(),
            },
            is_identical: false,
            schema: schema_diff,
            partitions_modified,
            data_files: DataFileDiff {
                files_added,
                files_removed,
                bytes_added,
                bytes_removed,
            },
        })
    }

    /// Compare schemas between two snapshots
    fn compare_schemas(
        metadata: &Arc<TableMetadata>,
        from_snapshot: &Snapshot,
        to_snapshot: &Snapshot,
    ) -> SchemaDiff {
        // Get schema IDs for each snapshot (fallback to current if not set)
        let from_schema_id = from_snapshot.schema_id().unwrap_or(metadata.current_schema_id());
        let to_schema_id = to_snapshot.schema_id().unwrap_or(metadata.current_schema_id());

        // If same schema, no changes
        if from_schema_id == to_schema_id {
            return SchemaDiff::default();
        }

        // Get schemas
        let from_schema = metadata
            .schema_by_id(from_schema_id)
            .unwrap_or_else(|| metadata.current_schema());
        let to_schema = metadata
            .schema_by_id(to_schema_id)
            .unwrap_or_else(|| metadata.current_schema());

        // Build field maps (name -> type string)
        let from_fields: HashSet<_> = from_schema
            .as_struct()
            .fields()
            .iter()
            .map(|f| (f.name.clone(), format!("{}", f.field_type)))
            .collect();

        let to_fields: HashSet<_> = to_schema
            .as_struct()
            .fields()
            .iter()
            .map(|f| (f.name.clone(), format!("{}", f.field_type)))
            .collect();

        // Find added and removed columns
        let columns_added: Vec<_> = to_fields.difference(&from_fields).cloned().collect();
        let columns_removed: Vec<_> = from_fields.difference(&to_fields).cloned().collect();

        SchemaDiff {
            columns_added,
            columns_removed,
        }
    }

    /// Resolve a reference (snapshot ID, branch name, or tag name) to a snapshot ID
    pub fn resolve_ref(metadata: &Arc<TableMetadata>, reference: &str) -> Result<i64> {
        // Try parsing as snapshot ID first
        if let Ok(id) = reference.parse::<i64>() {
            if metadata.snapshot_by_id(id).is_some() {
                return Ok(id);
            }
            return Err(Error::SnapshotNotFound { snapshot_id: id });
        }

        // Try as branch/tag name
        if let Some(snapshot) = metadata.snapshot_for_ref(reference) {
            return Ok(snapshot.snapshot_id());
        }

        Err(Error::DataValidation {
            message: format!(
                "Reference '{}' not found (not a valid snapshot ID, branch, or tag)",
                reference
            ),
        })
    }

    /// Get data files from a snapshot (path, size)
    async fn get_data_files(
        service: &IcebergMetadataService,
        snapshot: &Snapshot,
    ) -> Result<Vec<(String, u64)>> {
        let file_io = service.file_io();
        let metadata = service.table().metadata();

        let manifest_list = snapshot
            .load_manifest_list(file_io, &metadata)
            .await
            .map_err(|e| Error::Manifest {
                message: format!("Failed to load manifest list: {}", e),
            })?;

        let mut files = Vec::new();

        for manifest_entry in manifest_list.entries() {
            let manifest = manifest_entry.load_manifest(file_io).await.map_err(|e| {
                Error::Manifest {
                    message: format!("Failed to load manifest: {}", e),
                }
            })?;

            for entry in manifest.entries() {
                // Only include existing (non-deleted) files
                if entry.status() != ManifestStatus::Deleted {
                    let data_file = entry.data_file();
                    files.push((
                        data_file.file_path().to_string(),
                        data_file.file_size_in_bytes(),
                    ));
                }
            }
        }

        Ok(files)
    }
}

/// Extract partition key from a file path
///
/// Iceberg partition paths look like:
/// - `s3://bucket/table/data/date=2024-01-15/file.parquet`
/// - `s3://bucket/table/data/year=2024/month=01/file.parquet`
///
/// Returns the partition portion (e.g., "date=2024-01-15" or "year=2024/month=01")
fn extract_partition_from_path(path: &str) -> Option<String> {
    // Find the "data/" segment and extract partition directories after it
    let data_idx = path.find("/data/")?;
    let after_data = &path[data_idx + 6..]; // Skip "/data/"

    // Find the last "/" which separates partition dirs from filename
    let last_slash = after_data.rfind('/')?;
    let partition_part = &after_data[..last_slash];

    // Only return if it looks like a partition (contains "=")
    if partition_part.contains('=') {
        Some(partition_part.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_diff_result() {
        let result = SnapshotDiffResult {
            from: SnapshotRef {
                label: "parent".to_string(),
                snapshot_id: 123,
                timestamp_ms: 1000,
            },
            to: SnapshotRef {
                label: "current".to_string(),
                snapshot_id: 456,
                timestamp_ms: 2000,
            },
            is_identical: false,
            schema: SchemaDiff {
                columns_added: vec![("new_col".to_string(), "string".to_string())],
                columns_removed: vec![],
            },
            partitions_modified: vec!["date=2024-01-15".to_string()],
            data_files: DataFileDiff {
                files_added: 3,
                files_removed: 1,
                bytes_added: 45_000_000,
                bytes_removed: 12_000_000,
            },
        };

        assert!(!result.is_identical);
        assert_eq!(result.data_files.files_added, 3);
        assert_eq!(result.data_files.files_removed, 1);
        assert_eq!(result.schema.columns_added.len(), 1);
        assert_eq!(result.partitions_modified.len(), 1);
    }

    #[test]
    fn test_data_file_diff_default() {
        let diff = DataFileDiff::default();
        assert_eq!(diff.files_added, 0);
        assert_eq!(diff.files_removed, 0);
        assert_eq!(diff.bytes_added, 0);
        assert_eq!(diff.bytes_removed, 0);
    }

    #[test]
    fn test_schema_diff_is_empty() {
        let empty = SchemaDiff::default();
        assert!(empty.is_empty());

        let with_added = SchemaDiff {
            columns_added: vec![("col".to_string(), "int".to_string())],
            columns_removed: vec![],
        };
        assert!(!with_added.is_empty());
    }

    #[test]
    fn test_extract_partition_from_path() {
        // Single partition
        assert_eq!(
            extract_partition_from_path("s3://bucket/table/data/date=2024-01-15/file.parquet"),
            Some("date=2024-01-15".to_string())
        );

        // Multiple partitions
        assert_eq!(
            extract_partition_from_path("s3://bucket/table/data/year=2024/month=01/file.parquet"),
            Some("year=2024/month=01".to_string())
        );

        // No partition (unpartitioned table)
        assert_eq!(
            extract_partition_from_path("s3://bucket/table/data/file.parquet"),
            None
        );

        // No data directory
        assert_eq!(
            extract_partition_from_path("s3://bucket/table/file.parquet"),
            None
        );
    }
}

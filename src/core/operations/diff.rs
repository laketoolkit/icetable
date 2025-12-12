//! Diff service for comparing snapshots within a table
//!
//! Provides functionality to compare snapshots, branches, or tags
//! within the same Iceberg table. This compares manifest files
//! to show what changed between two points in time.

use std::sync::Arc;

use crate::core::metadata::IcebergMetadataService;
use crate::core::{Snapshot, TableMetadata};
use crate::error::{Error, Result};

/// Configuration for diff operation
#[derive(Debug, Clone, Default)]
pub struct DiffConfig {
    /// Reference to compare (snapshot ID, branch, or tag). Defaults to current.
    pub reference: Option<String>,
    /// Base reference to compare against. Defaults to parent of reference.
    pub base: Option<String>,
}

/// Result of comparing two snapshots
#[derive(Debug, Clone)]
pub struct SnapshotDiffResult {
    /// Base snapshot info
    pub base: SnapshotRef,
    /// Reference snapshot info
    pub reference: SnapshotRef,
    /// Whether both references point to the same snapshot
    pub is_identical: bool,
    /// Manifest files added in reference (not in base)
    pub manifests_added: Vec<String>,
    /// Manifest files removed from base (not in reference)
    pub manifests_removed: Vec<String>,
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
    /// Number of manifest files
    pub manifest_count: usize,
}

/// Service for comparing snapshots within a table
pub struct DiffService;

impl DiffService {
    /// Compare two snapshots within the same table
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

        // Resolve reference (default to current)
        let ref_id = if let Some(ref reference) = config.reference {
            Self::resolve_ref(&metadata, reference)?
        } else {
            current_id
        };

        // Resolve base (default to parent of reference)
        let base_id = if let Some(ref base_ref) = config.base {
            Self::resolve_ref(&metadata, base_ref)?
        } else {
            let ref_snapshot = metadata
                .snapshot_by_id(ref_id)
                .ok_or(Error::SnapshotNotFound {
                    snapshot_id: ref_id,
                })?;
            ref_snapshot
                .parent_snapshot_id()
                .ok_or_else(|| Error::DataValidation {
                    message: "No parent snapshot. Use --base to specify a base reference."
                        .to_string(),
                })?
        };

        // Check if they're the same
        if ref_id == base_id {
            return Ok(SnapshotDiffResult {
                base: SnapshotRef {
                    label: config.base.clone().unwrap_or_else(|| "parent".to_string()),
                    snapshot_id: base_id,
                    timestamp_ms: 0,
                    manifest_count: 0,
                },
                reference: SnapshotRef {
                    label: config
                        .reference
                        .clone()
                        .unwrap_or_else(|| "current".to_string()),
                    snapshot_id: ref_id,
                    timestamp_ms: 0,
                    manifest_count: 0,
                },
                is_identical: true,
                manifests_added: Vec::new(),
                manifests_removed: Vec::new(),
            });
        }

        // Get snapshots
        let base_snapshot = metadata
            .snapshot_by_id(base_id)
            .ok_or(Error::SnapshotNotFound {
                snapshot_id: base_id,
            })?;
        let ref_snapshot = metadata
            .snapshot_by_id(ref_id)
            .ok_or(Error::SnapshotNotFound {
                snapshot_id: ref_id,
            })?;

        // Get manifest files for both
        let base_manifests = Self::get_manifest_files(service, base_snapshot).await?;
        let ref_manifests = Self::get_manifest_files(service, ref_snapshot).await?;

        // Calculate diff
        let manifests_added: Vec<_> = ref_manifests
            .iter()
            .filter(|m| !base_manifests.contains(m))
            .cloned()
            .collect();
        let manifests_removed: Vec<_> = base_manifests
            .iter()
            .filter(|m| !ref_manifests.contains(m))
            .cloned()
            .collect();

        Ok(SnapshotDiffResult {
            base: SnapshotRef {
                label: config.base.clone().unwrap_or_else(|| "parent".to_string()),
                snapshot_id: base_id,
                timestamp_ms: base_snapshot.timestamp_ms(),
                manifest_count: base_manifests.len(),
            },
            reference: SnapshotRef {
                label: config
                    .reference
                    .clone()
                    .unwrap_or_else(|| "current".to_string()),
                snapshot_id: ref_id,
                timestamp_ms: ref_snapshot.timestamp_ms(),
                manifest_count: ref_manifests.len(),
            },
            is_identical: false,
            manifests_added,
            manifests_removed,
        })
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

    /// Get manifest file paths from a snapshot
    async fn get_manifest_files(
        service: &IcebergMetadataService,
        snapshot: &Snapshot,
    ) -> Result<Vec<String>> {
        let file_io = service.file_io();
        let metadata = service.table().metadata();

        let manifest_list = snapshot
            .load_manifest_list(file_io, &metadata)
            .await
            .map_err(|e| Error::Manifest {
                message: format!("Failed to load manifest list: {}", e),
            })?;

        Ok(manifest_list
            .entries()
            .iter()
            .map(|e| e.manifest_path.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_ref_default() {
        let result = SnapshotDiffResult {
            base: SnapshotRef {
                label: "parent".to_string(),
                snapshot_id: 123,
                timestamp_ms: 1000,
                manifest_count: 5,
            },
            reference: SnapshotRef {
                label: "current".to_string(),
                snapshot_id: 456,
                timestamp_ms: 2000,
                manifest_count: 7,
            },
            is_identical: false,
            manifests_added: vec!["manifest1.avro".to_string()],
            manifests_removed: vec![],
        };

        assert!(!result.is_identical);
        assert_eq!(result.manifests_added.len(), 1);
        assert!(result.manifests_removed.is_empty());
    }
}

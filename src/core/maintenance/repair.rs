//! Repair service for fixing table metadata
//!
//! This service handles metadata repair operations for both Delta Lake and Iceberg tables
//! by detecting orphan files and missing metadata entries.

use std::collections::{HashMap, HashSet};

use super::MaintenanceConfig;
use crate::core::metadata::{
    DataFileChanges, DataFileInfo, MaintenanceResult, MetadataService, OperationType,
};
use crate::error::Result;

/// Extract filename from a path (handles both local and cloud paths)
fn extract_filename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// Service for repairing table metadata
pub struct RepairService {
    config: MaintenanceConfig,
}

impl RepairService {
    /// Create a new repair service with default config
    pub fn new() -> Self {
        Self {
            config: MaintenanceConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: MaintenanceConfig) -> Self {
        Self { config }
    }

    /// Analyze the table and find issues
    ///
    /// Uses `get_all_referenced_files()` to check ALL snapshots, not just current.
    /// A file is only truly orphaned if it's not referenced by ANY snapshot.
    pub async fn analyze<M: MetadataService>(
        &self,
        metadata_service: &M,
    ) -> Result<RepairAnalysis> {
        // Get files referenced by ANY snapshot (for orphan detection)
        let all_referenced = metadata_service.get_all_referenced_files().await?;

        // Build reference set with both full path and filename for flexible matching
        let mut reference_set: HashSet<String> = HashSet::new();
        for path in &all_referenced {
            reference_set.insert(path.clone());
            reference_set.insert(extract_filename(path));
        }

        // Scan storage for parquet files
        let storage_files = metadata_service.scan_data_files_on_storage().await?;

        // Find orphan files (on storage but not referenced by ANY snapshot)
        let orphan_files: Vec<DataFileInfo> = storage_files
            .iter()
            .filter(|f| {
                let is_referenced = reference_set
                    .iter()
                    .any(|referenced| f.path.ends_with(referenced) || f.path == *referenced);
                !is_referenced
            })
            .cloned()
            .collect();

        // For missing files, we check against current snapshot only
        // (files missing from current but present in old snapshots are not "missing")
        let current_files = metadata_service.list_data_files().await?;

        let mut storage_set: HashSet<String> = HashSet::new();
        for f in &storage_files {
            storage_set.insert(f.path.clone());
            storage_set.insert(extract_filename(&f.path));
        }

        let missing_files: Vec<DataFileInfo> = current_files
            .iter()
            .filter(|f| {
                let is_on_storage = storage_set
                    .iter()
                    .any(|stored| f.path.ends_with(stored) || f.path == *stored);
                !is_on_storage
            })
            .cloned()
            .collect();

        Ok(RepairAnalysis {
            orphan_files,
            missing_files,
            total_tracked: current_files.len(),
            total_on_disk: storage_files.len(),
        })
    }

    /// Run the repair operation
    pub async fn execute<M: MetadataService>(
        &self,
        metadata_service: &M,
    ) -> Result<MaintenanceResult> {
        let analysis = self.analyze(metadata_service).await?;

        if analysis.orphan_files.is_empty() && analysis.missing_files.is_empty() {
            return Ok(MaintenanceResult::no_changes("No issues found"));
        }

        if self.config.dry_run {
            let mut details = HashMap::new();
            details.insert("mode".to_string(), "dry-run".to_string());
            details.insert(
                "orphan_files".to_string(),
                analysis.orphan_files.len().to_string(),
            );
            details.insert(
                "missing_files".to_string(),
                analysis.missing_files.len().to_string(),
            );

            return Ok(MaintenanceResult {
                files_added: analysis.orphan_files.len(),
                files_removed: analysis.missing_files.len(),
                bytes_added: analysis.orphan_files.iter().map(|f| f.size).sum(),
                bytes_removed: analysis.missing_files.iter().map(|f| f.size).sum(),
                records_affected: 0,
                operation: "repair (dry-run)".to_string(),
                details,
            });
        }

        let mut changes = DataFileChanges::new();

        // Add orphan files to metadata
        changes.added.extend(analysis.orphan_files);

        // Remove missing files from metadata
        changes.removed.extend(analysis.missing_files);

        if changes.is_empty() {
            return Ok(MaintenanceResult::no_changes("No changes to apply"));
        }

        // Build summary
        let mut summary = HashMap::new();
        summary.insert("orphans_added".to_string(), changes.added.len().to_string());
        summary.insert(
            "missing_removed".to_string(),
            changes.removed.len().to_string(),
        );

        // Commit the changes
        let snapshot_info = metadata_service
            .write_snapshot(changes.clone(), OperationType::Repair, summary)
            .await?;

        let mut details = HashMap::new();
        details.insert("snapshot_id".to_string(), snapshot_info.id.to_string());

        Ok(MaintenanceResult {
            files_added: changes.added.len(),
            files_removed: changes.removed.len(),
            bytes_added: changes.bytes_added(),
            bytes_removed: changes.bytes_removed(),
            records_affected: changes.records_added() + changes.records_removed(),
            operation: "repair".to_string(),
            details,
        })
    }
}

impl Default for RepairService {
    fn default() -> Self {
        Self::new()
    }
}

/// Analysis result from repair service
#[derive(Debug, Clone)]
pub struct RepairAnalysis {
    /// Files on disk but not in metadata
    pub orphan_files: Vec<DataFileInfo>,
    /// Files in metadata but not on disk
    pub missing_files: Vec<DataFileInfo>,
    /// Total tracked files in metadata
    pub total_tracked: usize,
    /// Total files on disk
    pub total_on_disk: usize,
}

impl RepairAnalysis {
    /// Check if there are any issues
    pub fn has_issues(&self) -> bool {
        !self.orphan_files.is_empty() || !self.missing_files.is_empty()
    }

    /// Get total orphan bytes
    pub fn orphan_bytes(&self) -> u64 {
        self.orphan_files.iter().map(|f| f.size).sum()
    }

    /// Get total missing bytes
    pub fn missing_bytes(&self) -> u64 {
        self.missing_files.iter().map(|f| f.size).sum()
    }
}

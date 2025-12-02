//! Repair service for fixing table metadata
//!
//! This service handles metadata repair operations for both Delta Lake and Iceberg tables
//! by detecting orphan files and missing metadata entries.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::MaintenanceConfig;
use crate::core::metadata::{
    DataFileChanges, DataFileInfo, MaintenanceResult, MetadataService, OperationType,
};
use crate::core::utils::fs::{normalize_path, scan_parquet_files, ScanConfig};
use crate::core::utils::parquet::read_parquet_record_count_from_file;
use crate::error::Result;

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
    pub async fn analyze<M: MetadataService>(
        &self,
        metadata_service: &M,
    ) -> Result<RepairAnalysis> {
        let data_dir = metadata_service.data_directory();
        let tracked_files = metadata_service.list_data_files().await?;

        // Get set of tracked file paths
        let tracked_paths: HashSet<String> = tracked_files
            .iter()
            .map(|f| normalize_path(&f.path))
            .collect();

        // Scan filesystem for parquet files
        let fs_files = self.scan_data_files(&data_dir)?;

        // Find orphan files (on disk but not in metadata)
        let orphan_files: Vec<DataFileInfo> = fs_files
            .iter()
            .filter(|f| !tracked_paths.contains(&normalize_path(&f.path)))
            .cloned()
            .collect();

        // Find missing files (in metadata but not on disk)
        let fs_paths: HashSet<String> = fs_files
            .iter()
            .map(|f| normalize_path(&f.path))
            .collect();

        let missing_files: Vec<DataFileInfo> = tracked_files
            .iter()
            .filter(|f| !fs_paths.contains(&normalize_path(&f.path)))
            .cloned()
            .collect();

        Ok(RepairAnalysis {
            orphan_files,
            missing_files,
            total_tracked: tracked_files.len(),
            total_on_disk: fs_files.len(),
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

    /// Scan data directory for parquet files
    fn scan_data_files(&self, data_dir: &Path) -> Result<Vec<DataFileInfo>> {
        let scan_config = ScanConfig::parquet();
        let scanned_files = scan_parquet_files(data_dir, &scan_config)?;

        Ok(scanned_files
            .into_iter()
            .filter_map(|f| self.to_data_file_info(&f.path))
            .collect())
    }

    /// Convert a file path to DataFileInfo
    fn to_data_file_info(&self, path: &str) -> Option<DataFileInfo> {
        let path_buf = Path::new(path);

        // Get file size
        let size = std::fs::metadata(path_buf).ok()?.len();

        // Try to read record count from parquet metadata
        let record_count = read_parquet_record_count_from_file(path_buf).unwrap_or(0);

        // Try to extract partition from path
        let partition = self.extract_partition_from_path(path_buf);

        Some(DataFileInfo {
            path: path.to_string(),
            size,
            record_count,
            partition,
        })
    }

    /// Extract partition information from file path
    fn extract_partition_from_path(&self, path: &Path) -> HashMap<String, String> {
        let mut partition = HashMap::new();

        for component in path.components() {
            let part = component.as_os_str().to_string_lossy();
            if let Some(eq_pos) = part.find('=') {
                let key = part[..eq_pos].to_string();
                let value = part[eq_pos + 1..].to_string();
                partition.insert(key, value);
            }
        }

        partition
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

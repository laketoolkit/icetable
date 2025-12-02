//! Repair service for fixing table metadata
//!
//! This service handles metadata repair operations for both Delta Lake and Iceberg tables
//! by detecting orphan files and missing metadata entries.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use super::MaintenanceConfig;
use crate::core::metadata::{
    DataFileChanges, DataFileInfo, MaintenanceResult, MetadataService, OperationType,
};
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
            .map(|f| self.normalize_path(&f.path))
            .collect();

        // Scan filesystem for parquet files
        let fs_files = self.scan_data_files(&data_dir)?;

        // Find orphan files (on disk but not in metadata)
        let orphan_files: Vec<DataFileInfo> = fs_files
            .iter()
            .filter(|f| !tracked_paths.contains(&self.normalize_path(&f.path)))
            .cloned()
            .collect();

        // Find missing files (in metadata but not on disk)
        let fs_paths: HashSet<String> = fs_files
            .iter()
            .map(|f| self.normalize_path(&f.path))
            .collect();

        let missing_files: Vec<DataFileInfo> = tracked_files
            .iter()
            .filter(|f| !fs_paths.contains(&self.normalize_path(&f.path)))
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
        let mut files = Vec::new();
        self.scan_directory_recursive(data_dir, &mut files);
        Ok(files)
    }

    /// Recursively scan a directory for parquet files
    fn scan_directory_recursive(&self, dir: &Path, files: &mut Vec<DataFileInfo>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip metadata directories
            if name == "_delta_log" || name == "metadata" {
                continue;
            }

            if path.is_dir() {
                self.scan_directory_recursive(&path, files);
            } else if path.extension().map_or(false, |ext| ext == "parquet") {
                if let Some(info) = self.read_file_info(&path) {
                    files.push(info);
                }
            }
        }
    }

    /// Read file information from a parquet file
    fn read_file_info(&self, path: &Path) -> Option<DataFileInfo> {
        let file_path = path.to_string_lossy().to_string();

        // Get file size
        let size = std::fs::metadata(path).ok()?.len();

        // Try to read record count from parquet metadata
        let record_count = self.read_parquet_record_count(path).unwrap_or(0);

        // Try to extract partition from path
        let partition = self.extract_partition_from_path(path);

        Some(DataFileInfo {
            path: file_path,
            size,
            record_count,
            partition,
        })
    }

    /// Read record count from parquet file footer
    fn read_parquet_record_count(&self, path: &Path) -> Option<u64> {
        let file = std::fs::File::open(path).ok()?;
        let reader = ParquetRecordBatchReaderBuilder::try_new(file).ok()?;
        Some(reader.metadata().file_metadata().num_rows() as u64)
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

    /// Normalize a file path for comparison
    fn normalize_path(&self, path: &str) -> String {
        path.strip_prefix("file://")
            .unwrap_or(path)
            .to_string()
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

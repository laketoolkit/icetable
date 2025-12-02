//! Vacuum service for cleaning up old files
//!
//! This service handles vacuum operations for both Delta Lake and Iceberg tables
//! by removing files that are no longer referenced by any snapshot.

use std::collections::{HashMap, HashSet};

use crate::core::metadata::{MaintenanceResult, MetadataService};
use crate::core::utils::fs::{normalize_path, scan_parquet_files, ScanConfig};
use crate::core::utils::{format_bytes, sizes};
use crate::error::Result;

/// Service for vacuuming tables (removing unreferenced files)
pub struct VacuumService {
    config: VacuumConfig,
}

/// Configuration specific to vacuum operations
#[derive(Debug, Clone)]
pub struct VacuumConfig {
    /// Retention period in hours (files older than this may be deleted)
    pub retention_hours: u64,
    /// Whether to run in dry-run mode
    pub dry_run: bool,
    /// Whether to delete metadata files as well
    pub include_metadata: bool,
}

impl Default for VacuumConfig {
    fn default() -> Self {
        Self {
            retention_hours: sizes::DEFAULT_RETENTION_HOURS,
            dry_run: false,
            include_metadata: false,
        }
    }
}

impl VacuumService {
    /// Create a new vacuum service with default config
    pub fn new() -> Self {
        Self {
            config: VacuumConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: VacuumConfig) -> Self {
        Self { config }
    }

    /// Analyze files that would be deleted
    pub async fn analyze<M: MetadataService>(
        &self,
        metadata_service: &M,
    ) -> Result<VacuumAnalysis> {
        let data_dir = metadata_service.data_directory();
        let current_files = metadata_service.list_data_files().await?;

        // Get set of currently referenced file paths
        let referenced_paths: HashSet<String> = current_files
            .iter()
            .map(|f| normalize_path(&f.path))
            .collect();

        // Calculate cutoff time
        let cutoff_time = chrono::Utc::now()
            - chrono::Duration::hours(self.config.retention_hours as i64);
        let cutoff_timestamp = cutoff_time.timestamp();

        // Configure scan
        let mut scan_config = ScanConfig::parquet().with_cutoff(cutoff_timestamp);
        if self.config.include_metadata {
            scan_config.skip_dirs.clear();
        }

        // Scan filesystem for all parquet files
        let scanned_files = scan_parquet_files(&data_dir, &scan_config)?;

        // Filter to orphan files (not referenced)
        let orphan_files: Vec<OrphanFile> = scanned_files
            .into_iter()
            .filter(|f| !referenced_paths.contains(&f.path))
            .map(|f| OrphanFile {
                path: f.path,
                size: f.size,
                mtime_seconds: f.mtime_seconds,
            })
            .collect();

        let orphan_bytes = orphan_files.iter().map(|f| f.size).sum();

        Ok(VacuumAnalysis {
            orphan_files,
            orphan_bytes,
            referenced_count: referenced_paths.len(),
            retention_hours: self.config.retention_hours,
        })
    }

    /// Run the vacuum operation
    pub async fn execute<M: MetadataService>(
        &self,
        metadata_service: &M,
    ) -> Result<MaintenanceResult> {
        let analysis = self.analyze(metadata_service).await?;

        if analysis.orphan_files.is_empty() {
            return Ok(MaintenanceResult::no_changes(
                "No unreferenced files to delete",
            ));
        }

        if self.config.dry_run {
            let mut details = HashMap::new();
            details.insert("mode".to_string(), "dry-run".to_string());
            details.insert(
                "files_to_delete".to_string(),
                analysis.orphan_files.len().to_string(),
            );
            details.insert(
                "bytes_to_free".to_string(),
                format_bytes(analysis.orphan_bytes),
            );

            return Ok(MaintenanceResult {
                files_added: 0,
                files_removed: analysis.orphan_files.len(),
                bytes_added: 0,
                bytes_removed: analysis.orphan_bytes,
                records_affected: 0,
                operation: "vacuum (dry-run)".to_string(),
                details,
            });
        }

        // Actually delete the files
        let mut deleted_count = 0;
        let mut deleted_bytes = 0u64;
        let mut errors = Vec::new();

        for file in &analysis.orphan_files {
            match std::fs::remove_file(&file.path) {
                Ok(_) => {
                    deleted_count += 1;
                    deleted_bytes += file.size;
                }
                Err(e) => {
                    errors.push(format!("{}: {}", file.path, e));
                }
            }
        }

        let mut details = HashMap::new();
        details.insert("deleted_files".to_string(), deleted_count.to_string());
        details.insert("freed_bytes".to_string(), format_bytes(deleted_bytes));

        if !errors.is_empty() {
            details.insert("errors".to_string(), errors.len().to_string());
        }

        Ok(MaintenanceResult {
            files_added: 0,
            files_removed: deleted_count,
            bytes_added: 0,
            bytes_removed: deleted_bytes,
            records_affected: 0,
            operation: "vacuum".to_string(),
            details,
        })
    }
}

impl Default for VacuumService {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about an orphan file
#[derive(Debug, Clone)]
pub struct OrphanFile {
    /// Full path to the file
    pub path: String,
    /// File size in bytes
    pub size: u64,
    /// Modification time as unix timestamp
    pub mtime_seconds: i64,
}

/// Analysis result from vacuum service
#[derive(Debug, Clone)]
pub struct VacuumAnalysis {
    /// Files that can be deleted
    pub orphan_files: Vec<OrphanFile>,
    /// Total bytes that can be freed
    pub orphan_bytes: u64,
    /// Number of currently referenced files
    pub referenced_count: usize,
    /// Retention period in hours
    pub retention_hours: u64,
}

impl VacuumAnalysis {
    /// Check if there are files to delete
    pub fn has_files_to_delete(&self) -> bool {
        !self.orphan_files.is_empty()
    }
}

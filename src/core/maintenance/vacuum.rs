//! Vacuum service for cleaning up old files
//!
//! This service handles vacuum operations for both Delta Lake and Iceberg tables
//! by removing files that are no longer referenced by any snapshot.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::core::metadata::{utils, MaintenanceResult, MetadataService};
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
            retention_hours: 168, // 7 days
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
            .map(|f| self.normalize_path(&f.path))
            .collect();

        // Calculate cutoff time
        let cutoff_time = chrono::Utc::now()
            - chrono::Duration::hours(self.config.retention_hours as i64);
        let cutoff_timestamp = cutoff_time.timestamp();

        // Scan filesystem for all parquet files
        let mut orphan_files = Vec::new();
        let mut orphan_bytes = 0u64;

        self.scan_directory_recursive(
            &data_dir,
            &referenced_paths,
            cutoff_timestamp,
            &mut orphan_files,
            &mut orphan_bytes,
        );

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
                utils::format_bytes(analysis.orphan_bytes),
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
        details.insert("freed_bytes".to_string(), utils::format_bytes(deleted_bytes));

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

    /// Normalize a file path for comparison
    fn normalize_path(&self, path: &str) -> String {
        path.strip_prefix("file://").unwrap_or(path).to_string()
    }

    /// Recursively scan directory for orphan parquet files
    fn scan_directory_recursive(
        &self,
        dir: &Path,
        referenced_paths: &HashSet<String>,
        cutoff_timestamp: i64,
        orphan_files: &mut Vec<OrphanFile>,
        orphan_bytes: &mut u64,
    ) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip metadata directories unless configured
            if !self.config.include_metadata {
                if name == "_delta_log" || name == "metadata" {
                    continue;
                }
            }

            if path.is_dir() {
                self.scan_directory_recursive(
                    &path,
                    referenced_paths,
                    cutoff_timestamp,
                    orphan_files,
                    orphan_bytes,
                );
            } else if path.extension().map_or(false, |ext| ext == "parquet") {
                let path_str = self.normalize_path(&path.to_string_lossy());

                // Skip if file is currently referenced
                if referenced_paths.contains(&path_str) {
                    continue;
                }

                // Check file modification time
                if let Ok(metadata) = std::fs::metadata(&path) {
                    let mtime = metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);

                    // Only include files older than retention period
                    if mtime < cutoff_timestamp {
                        *orphan_bytes += metadata.len();
                        orphan_files.push(OrphanFile {
                            path: path_str,
                            size: metadata.len(),
                            mtime_seconds: mtime,
                        });
                    }
                }
            }
        }
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

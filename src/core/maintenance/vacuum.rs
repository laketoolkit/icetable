//! Vacuum service for cleaning up old files
//!
//! This service handles vacuum operations for Iceberg tables
//! by removing files that are no longer referenced by any snapshot.
//!
//! Supports both local and cloud storage (S3, GCS, Azure).

use std::collections::{HashMap, HashSet};

use futures::TryStreamExt;

use crate::core::metadata::{IcebergMetadataService, MaintenanceResult};
use crate::core::storage::{ObjectStoreExt, create_object_store};
use crate::utils::core::{format_bytes, sizes};
use crate::error::{Error, Result};

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
    /// Maximum concurrent operations for scanning/deleting
    pub parallelism: usize,
}

impl Default for VacuumConfig {
    fn default() -> Self {
        Self {
            retention_hours: sizes::DEFAULT_RETENTION_HOURS,
            dry_run: false,
            parallelism: 32,
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

    /// Analyze files that would be deleted (works with any storage backend)
    /// If metadata_service is provided, uses it; otherwise creates one from table_path
    pub async fn analyze_with_service(
        &self,
        table_path: &str,
        metadata_service: Option<&IcebergMetadataService>,
    ) -> Result<VacuumAnalysis> {
        // Use provided service or create one
        let owned_service;
        let service = match metadata_service {
            Some(s) => s,
            None => {
                owned_service = IcebergMetadataService::new_async(table_path.to_string())
                    .await
                    .map_err(|_| Error::TableNotFound {
                        path: table_path.to_string(),
                    })?;
                &owned_service
            }
        };

        let table = service.table();
        let metadata = table.metadata();

        // Step 1: Collect all referenced files from ALL snapshots using native scan API
        let mut referenced_files: HashSet<String> = HashSet::new();

        for snapshot in metadata.snapshots() {
            let scan = match table.scan()
                .snapshot_id(snapshot.snapshot_id())
                .build()
            {
                Ok(s) => s,
                Err(_) => continue,
            };

            let tasks: Vec<_> = match scan.plan_files().await {
                Ok(stream) => match stream.try_collect().await {
                    Ok(t) => t,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            for task in tasks {
                referenced_files.insert(task.data_file_path().to_string());
            }
        }

        // Build filename lookup set for O(1) matching
        let referenced_filenames: HashSet<String> = referenced_files
            .iter()
            .filter_map(|r| r.rsplit('/').next().map(|s| s.to_string()))
            .collect();

        // Step 3: List all files in data directory
        let storage = create_object_store(table_path).await?;
        let base_path = table_path.trim_end_matches('/');
        let data_prefix = format!("{}/data/", base_path);

        let all_files = storage.list_prefix(&data_prefix).await?;

        // Step 4: Calculate cutoff time and find orphan files
        let cutoff_time =
            chrono::Utc::now() - chrono::Duration::hours(self.config.retention_hours as i64);
        let cutoff_ms = cutoff_time.timestamp_millis();

        let mut orphan_files: Vec<OrphanFile> = Vec::new();
        let mut orphan_bytes: u64 = 0;

        for obj in &all_files {
            let path_str = obj.location.to_string();
            let filename = path_str.rsplit('/').next().unwrap_or(&path_str);
            let is_referenced = referenced_filenames.contains(filename);

            if !is_referenced {
                let file_time_ms = obj.last_modified.timestamp_millis();
                if file_time_ms < cutoff_ms {
                    orphan_files.push(OrphanFile {
                        path: path_str.clone(),
                        size: obj.size,
                        mtime_ms: file_time_ms,
                    });
                    orphan_bytes += obj.size;
                }
            }
        }

        Ok(VacuumAnalysis {
            orphan_files,
            orphan_bytes,
            referenced_count: referenced_files.len(),
            retention_hours: self.config.retention_hours,
        })
    }

    /// Analyze files that would be deleted - convenience method using table path
    pub async fn analyze(&self, table_path: &str) -> Result<VacuumAnalysis> {
        self.analyze_with_service(table_path, None).await
    }

    /// Execute vacuum operation - deletes orphan files
    /// If metadata_service is provided, uses it; otherwise creates one from table_path
    pub async fn execute_with_service(
        &self,
        table_path: &str,
        metadata_service: Option<&IcebergMetadataService>,
    ) -> Result<VacuumResult> {
        let analysis = self.analyze_with_service(table_path, metadata_service).await?;

        if analysis.orphan_files.is_empty() {
            return Ok(VacuumResult {
                deleted_count: 0,
                deleted_bytes: 0,
                errors: Vec::new(),
                dry_run: self.config.dry_run,
                analysis,
            });
        }

        if self.config.dry_run {
            return Ok(VacuumResult {
                deleted_count: 0,
                deleted_bytes: 0,
                errors: Vec::new(),
                dry_run: true,
                analysis,
            });
        }

        // Actually delete the files
        let storage = create_object_store(table_path).await?;
        let mut deleted_count = 0;
        let mut deleted_bytes = 0u64;
        let mut errors = Vec::new();

        for file in &analysis.orphan_files {
            match storage.delete_str(&file.path).await {
                Ok(_) => {
                    deleted_count += 1;
                    deleted_bytes += file.size;
                }
                Err(e) => {
                    errors.push(format!("{}: {}", file.path, e));
                }
            }
        }

        Ok(VacuumResult {
            deleted_count,
            deleted_bytes,
            errors,
            dry_run: false,
            analysis,
        })
    }

    /// Execute vacuum operation - convenience method using table path
    pub async fn execute(&self, table_path: &str) -> Result<VacuumResult> {
        self.execute_with_service(table_path, None).await
    }

    /// Convert vacuum result to MaintenanceResult for consistent output
    pub fn to_maintenance_result(&self, result: &VacuumResult) -> MaintenanceResult {
        let mut details = HashMap::new();

        if result.dry_run {
            details.insert("mode".to_string(), "dry-run".to_string());
            details.insert(
                "files_to_delete".to_string(),
                result.analysis.orphan_files.len().to_string(),
            );
            details.insert(
                "bytes_to_free".to_string(),
                format_bytes(result.analysis.orphan_bytes),
            );
        } else {
            details.insert(
                "deleted_files".to_string(),
                result.deleted_count.to_string(),
            );
            details.insert(
                "freed_bytes".to_string(),
                format_bytes(result.deleted_bytes),
            );
            if !result.errors.is_empty() {
                details.insert("errors".to_string(), result.errors.len().to_string());
            }
        }

        MaintenanceResult {
            files_added: 0,
            files_removed: if result.dry_run {
                result.analysis.orphan_files.len()
            } else {
                result.deleted_count
            },
            bytes_added: 0,
            bytes_removed: if result.dry_run {
                result.analysis.orphan_bytes
            } else {
                result.deleted_bytes
            },
            records_affected: 0,
            operation: if result.dry_run {
                "vacuum (dry-run)".to_string()
            } else {
                "vacuum".to_string()
            },
            details,
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
    /// Modification time as unix timestamp in milliseconds
    pub mtime_ms: i64,
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

/// Result of vacuum operation
#[derive(Debug, Clone)]
pub struct VacuumResult {
    /// Number of files deleted
    pub deleted_count: usize,
    /// Total bytes freed
    pub deleted_bytes: u64,
    /// Errors encountered during deletion
    pub errors: Vec<String>,
    /// Whether this was a dry run
    pub dry_run: bool,
    /// The analysis that was performed
    pub analysis: VacuumAnalysis,
}

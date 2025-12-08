//! Vacuum service for cleaning up old files
//!
//! This service handles vacuum operations for Iceberg tables
//! by removing files that are no longer referenced by any snapshot.
//!
//! Supports both local and cloud storage (S3, GCS, Azure).

use std::collections::{HashMap, HashSet};

use futures::stream::{self, StreamExt};
use iceberg::spec::ManifestList;

use crate::core::metadata::{IcebergMetadataService, MaintenanceResult};
use crate::core::storage::{create_object_store, ObjectStoreExt};
use crate::core::utils::{format_bytes, sizes};
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
    pub async fn analyze(&self, table_path: &str) -> Result<VacuumAnalysis> {
        let service = IcebergMetadataService::new_async(table_path.to_string())
            .await
            .map_err(|_| Error::table_not_found(table_path))?;

        let (metadata, _) = service.load_metadata().await?;
        let file_io = service.file_io().clone();

        // Step 1: Collect all referenced files from ALL snapshots
        let snapshots: Vec<_> = metadata.snapshots().collect();
        let mut seen_manifest_paths: HashSet<String> = HashSet::new();
        let mut manifest_entries: Vec<iceberg::spec::ManifestFile> = Vec::new();

        for snapshot in &snapshots {
            let manifest_list_path = snapshot.manifest_list();

            let manifest_list_content = match file_io
                .new_input(manifest_list_path)
                .map_err(|e| Error::manifest(format!("Failed to open manifest list: {}", e)))?
                .read()
                .await
            {
                Ok(content) => content,
                Err(_) => continue,
            };

            let manifest_list = match ManifestList::parse_with_version(
                &manifest_list_content,
                metadata.format_version(),
            ) {
                Ok(ml) => ml,
                Err(_) => continue,
            };

            for entry in manifest_list.entries() {
                if !seen_manifest_paths.contains(&entry.manifest_path) {
                    seen_manifest_paths.insert(entry.manifest_path.clone());
                    manifest_entries.push(entry.clone());
                }
            }
        }

        // Step 2: Load all manifests in parallel to get referenced data files
        let manifest_results: Vec<Vec<String>> = stream::iter(manifest_entries.into_iter())
            .map(|manifest_entry| {
                let file_io = file_io.clone();
                async move {
                    if let Ok(manifest) = manifest_entry.load_manifest(&file_io).await {
                        manifest
                            .entries()
                            .iter()
                            .map(|e| e.file_path().to_string())
                            .collect()
                    } else {
                        Vec::new()
                    }
                }
            })
            .buffer_unordered(self.config.parallelism)
            .collect()
            .await;

        let mut referenced_files: HashSet<String> = HashSet::new();
        for paths in manifest_results {
            referenced_files.extend(paths);
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

    /// Execute vacuum operation - deletes orphan files
    pub async fn execute(&self, table_path: &str) -> Result<VacuumResult> {
        let analysis = self.analyze(table_path).await?;

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
            details.insert("deleted_files".to_string(), result.deleted_count.to_string());
            details.insert("freed_bytes".to_string(), format_bytes(result.deleted_bytes));
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

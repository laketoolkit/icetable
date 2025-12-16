//! Maintenance services module
//!
//! Provides high-level services for table maintenance operations.
//! These services use the `TableServiceWriter` trait to work with
//! Iceberg tables through a unified interface.

mod doctor;
mod manifest;
mod optimize;
mod partition_filter;
mod refs;
mod repair;
mod snapshot;
mod vacuum;

pub use doctor::{CheckResult, CheckStatus, CheckSummary, DoctorConfig, DoctorService};
pub use manifest::{ManifestAnalysis, ManifestConfig, ManifestRewriteResult, ManifestService};
pub use optimize::OptimizeService;
pub use partition_filter::{PartitionFilter, matches_partition_filter};
pub use refs::{BranchRetention, RefConfig, RefResult, RefService};
pub use repair::{RepairAnalysis, RepairService};
pub use snapshot::{
    CreateBackupResult, ExpireSnapshotsResult, LineageEntry, LineageResult, ListSnapshotsResult,
    SetSnapshotResult, SnapshotConfig, SnapshotDetails, SnapshotService,
};
pub use vacuum::{OrphanFile, VacuumAnalysis, VacuumConfig, VacuumResult, VacuumService};

use std::collections::HashMap;

use iceberg::spec::TableMetadata;

use crate::core::metadata::DataFileInfo;
use crate::core::storage::{Storage, create_object_store};
use crate::error::Result;
use crate::utils::core::sizes;

// =============================================================================
// Shared helpers for maintenance services
// =============================================================================

/// Result of writing metadata directly to storage
#[derive(Debug, Clone)]
pub struct DirectWriteResult {
    /// New metadata version
    pub version: i64,
    /// Path where metadata was written
    pub path: String,
}

/// Write metadata directly to storage (for single-writer/direct mode)
///
/// This is a shared helper for maintenance services that need to write
/// metadata when not using a catalog committer. It uses standard Iceberg
/// naming conventions for metadata files.
///
/// # Arguments
/// * `table_path` - Base path of the table
/// * `metadata` - The new table metadata to write
///
/// # Returns
/// The new metadata version number
///
/// # Example
/// ```ignore
/// let new_version = write_metadata_direct(table_path, &new_metadata).await?;
/// ```
pub async fn write_metadata_direct(
    table_path: &str,
    metadata: &TableMetadata,
) -> Result<i64> {
    let storage = create_object_store(table_path).await?;
    write_metadata_with_storage(table_path, metadata, &storage).await
}

/// Write metadata using an existing storage handle
///
/// This variant is useful when the caller already has a storage handle
/// and wants to avoid creating a new one.
pub async fn write_metadata_with_storage(
    table_path: &str,
    metadata: &TableMetadata,
    storage: &Storage,
) -> Result<i64> {
    let result = crate::utils::core::write_metadata_file(table_path, metadata, storage).await?;
    Ok(result.version)
}

/// Common configuration for maintenance operations
#[derive(Debug, Clone)]
pub struct MaintenanceConfig {
    /// Target file size in bytes (default: 256 MB)
    pub target_size: u64,
    /// Minimum file size to consider for compaction (default: 16 MB)
    pub min_size: u64,
    /// Maximum file size to consider for compaction (default: 512 MB)
    pub max_size: u64,
    /// Whether to run in dry-run mode
    pub dry_run: bool,
    /// Number of parallel operations
    pub parallelism: usize,
    /// Filter to specific partition (e.g., "day=2024-01-01/currency=USD")
    pub partition_filter: Option<String>,
    /// Maximum number of input files to process (for incremental compaction)
    pub max_files: Option<usize>,
    /// Maximum bytes to process (for incremental compaction)
    pub max_bytes: Option<u64>,
}

impl MaintenanceConfig {
    /// Validate the configuration values
    ///
    /// Returns an error if any configuration values are invalid:
    /// - `min_size` must be less than `target_size`
    /// - `target_size` must be less than `max_size`
    /// - `parallelism` must be greater than 0
    /// - `max_files` must be greater than 0 if set
    /// - `max_bytes` must be greater than 0 if set
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.min_size >= self.target_size {
            return Err(crate::error::Error::Configuration {
                message: format!(
                    "min_size ({}) must be less than target_size ({})",
                    self.min_size, self.target_size
                ),
            });
        }

        if self.target_size >= self.max_size {
            return Err(crate::error::Error::Configuration {
                message: format!(
                    "target_size ({}) must be less than max_size ({})",
                    self.target_size, self.max_size
                ),
            });
        }

        if self.parallelism == 0 {
            return Err(crate::error::Error::Configuration {
                message: "parallelism must be greater than 0".to_string(),
            });
        }

        if let Some(0) = self.max_files {
            return Err(crate::error::Error::Configuration {
                message: "max_files must be greater than 0 if set".to_string(),
            });
        }

        if let Some(0) = self.max_bytes {
            return Err(crate::error::Error::Configuration {
                message: "max_bytes must be greater than 0 if set".to_string(),
            });
        }

        Ok(())
    }
}

impl Default for MaintenanceConfig {
    fn default() -> Self {
        Self {
            target_size: sizes::DEFAULT_TARGET_SIZE,
            min_size: sizes::DEFAULT_MIN_SIZE,
            max_size: sizes::DEFAULT_MAX_SIZE,
            dry_run: false,
            parallelism: 16, // Higher default for I/O-bound operations
            partition_filter: None,
            max_files: None,
            max_bytes: None,
        }
    }
}

/// Result of file grouping for compaction
#[derive(Debug, Clone)]
pub struct FileGroup {
    /// Files to compact together
    pub files: Vec<DataFileInfo>,
    /// Total size of files in bytes
    pub total_size: u64,
    /// Total record count
    pub total_records: u64,
    /// Partition key (empty for non-partitioned)
    pub partition_key: String,
}

impl FileGroup {
    /// Create a new file group
    pub fn new(partition_key: String) -> Self {
        Self {
            files: Vec::new(),
            total_size: 0,
            total_records: 0,
            partition_key,
        }
    }

    /// Add a file to the group
    pub fn add(&mut self, file: DataFileInfo) {
        self.total_size += file.size;
        self.total_records += file.record_count;
        self.files.push(file);
    }

    /// Check if the group needs compaction (has multiple small files)
    ///
    /// A group needs compaction if:
    /// - It has at least 2 files
    /// - At least half of the files are smaller than `min_size`
    pub fn needs_compaction(&self, min_size: u64) -> bool {
        // Need at least 2 files to compact
        if self.files.len() < 2 {
            return false;
        }

        // Check if most files are undersized
        let small_files = self.files.iter().filter(|f| f.size < min_size).count();
        small_files >= self.files.len() / 2
    }
}

/// Utility for grouping files by partition
pub fn group_files_by_partition(files: Vec<DataFileInfo>) -> HashMap<String, FileGroup> {
    let mut groups: HashMap<String, FileGroup> = HashMap::new();

    for file in files {
        let partition_key = if file.partition.is_empty() {
            String::new()
        } else {
            let mut parts: Vec<String> = file
                .partition
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            parts.sort();
            parts.join("/")
        };

        groups
            .entry(partition_key.clone())
            .or_insert_with(|| FileGroup::new(partition_key))
            .add(file);
    }

    groups
}

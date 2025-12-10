//! Maintenance services module
//!
//! Provides high-level services for table maintenance operations.
//! These services use the MetadataService trait to work with both
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
    CreateBackupResult, ExpireSnapshotsResult, ListSnapshotsResult, SetSnapshotResult,
    SnapshotConfig, SnapshotDetails, SnapshotService,
};
pub use vacuum::{OrphanFile, VacuumAnalysis, VacuumConfig, VacuumResult, VacuumService};

use std::collections::HashMap;

use crate::core::metadata::DataFileInfo;
use crate::utils::core::sizes;

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

impl Default for MaintenanceConfig {
    fn default() -> Self {
        Self {
            target_size: sizes::DEFAULT_TARGET_SIZE,
            min_size: sizes::DEFAULT_MIN_SIZE,
            max_size: sizes::DEFAULT_MAX_SIZE,
            dry_run: false,
            parallelism: 4,
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

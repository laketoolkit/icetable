//! Maintenance services module
//!
//! Provides high-level services for table maintenance operations.
//! These services use the MetadataService trait to work with both
//! Delta Lake and Iceberg tables through a unified interface.

mod optimize;
mod repair;
mod vacuum;

pub use optimize::OptimizeService;
pub use repair::{RepairAnalysis, RepairService};
pub use vacuum::{VacuumAnalysis, VacuumConfig, VacuumService};

use std::collections::HashMap;

use crate::core::metadata::DataFileInfo;

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
}

impl Default for MaintenanceConfig {
    fn default() -> Self {
        Self {
            target_size: 256 * 1024 * 1024,  // 256 MB
            min_size: 16 * 1024 * 1024,      // 16 MB
            max_size: 512 * 1024 * 1024,     // 512 MB
            dry_run: false,
            parallelism: 4,
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
    pub fn needs_compaction(&self, min_size: u64, _target_size: u64) -> bool {
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

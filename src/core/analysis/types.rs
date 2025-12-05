//! Analysis result types
//!
//! Contains the data structures returned by analysis operations.
//! These are format-agnostic and used by the CLI for display.

use serde::Serialize;

/// Partition compaction info for detailed reporting
#[derive(Debug, Clone, Serialize)]
pub struct PartitionCompactionInfo {
    /// Partition key (e.g., "day=2024-01-01/currency=USD")
    pub partition: String,
    /// Total number of files in this partition
    pub files: usize,
    /// Number of files below the size threshold
    pub small_files: usize,
    /// Total size of all files in bytes
    pub size_bytes: u64,
    /// Total record count
    pub records: u64,
    /// Priority level (high, medium, low)
    pub priority: String,
    /// Score used for sorting (higher = more urgent)
    #[serde(skip_serializing)]
    pub priority_score: u64,
}

/// Data compaction analysis results
#[derive(Debug, Clone, Serialize)]
pub struct DataCompactionAnalysis {
    /// Total number of data files
    pub total_files: usize,
    /// Number of files below the size threshold
    pub small_files: usize,
    /// Number of partition groups that need compaction
    pub groups_needing_compaction: usize,
    /// Total size of all data files in bytes
    pub total_size: u64,
    /// Total size of small files in bytes
    pub small_files_size: u64,
    /// Minimum size threshold used for analysis
    pub min_size_threshold: u64,
    /// Partition-level details, sorted by priority
    pub partitions: Vec<PartitionCompactionInfo>,
}

impl DataCompactionAnalysis {
    /// Check if compaction is recommended
    pub fn needs_action(&self) -> bool {
        self.groups_needing_compaction > 0
    }
}

/// Manifest compaction analysis results
#[derive(Debug, Clone, Serialize)]
pub struct ManifestCompactionAnalysis {
    /// Total number of manifests in current snapshot
    pub total_manifests: usize,
    /// Recommended maximum number of manifests
    pub recommended_max: usize,
}

impl ManifestCompactionAnalysis {
    /// Check if manifest compaction is recommended
    pub fn needs_action(&self) -> bool {
        self.total_manifests > self.recommended_max
    }
}

/// Snapshot expiration analysis results
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotExpirationAnalysis {
    /// Total number of snapshots
    pub total_snapshots: usize,
    /// Number of snapshots older than 7 days
    pub snapshots_older_than_7d: usize,
    /// Number of snapshots older than 30 days
    pub snapshots_older_than_30d: usize,
    /// Age of the oldest snapshot in days
    pub oldest_snapshot_age_days: i64,
}

impl SnapshotExpirationAnalysis {
    /// Check if snapshot expiration is recommended
    pub fn needs_action(&self) -> bool {
        self.snapshots_older_than_7d > 0
    }
}

/// Orphan files analysis results
#[derive(Debug, Clone, Serialize)]
pub struct OrphanFilesAnalysis {
    /// Number of orphan files (on storage but not referenced)
    pub orphan_count: usize,
    /// Total size of orphan files in bytes
    pub orphan_size: u64,
    /// Number of missing files (referenced but not on storage)
    pub missing_count: usize,
}

impl OrphanFilesAnalysis {
    /// Check if orphan cleanup or repair is recommended
    pub fn needs_action(&self) -> bool {
        self.orphan_count > 0 || self.missing_count > 0
    }

    /// Check if there are missing files (integrity issue)
    pub fn has_missing_files(&self) -> bool {
        self.missing_count > 0
    }

    /// Check if there are orphan files
    pub fn has_orphan_files(&self) -> bool {
        self.orphan_count > 0
    }
}

/// Complete table health analysis
#[derive(Debug, Clone, Serialize)]
pub struct TableAnalysis {
    /// Path to the analyzed table
    pub table_path: String,
    /// Data compaction analysis
    pub data_compaction: DataCompactionAnalysis,
    /// Manifest compaction analysis
    pub manifest_compaction: ManifestCompactionAnalysis,
    /// Snapshot expiration analysis
    pub snapshot_expiration: SnapshotExpirationAnalysis,
    /// Orphan files analysis (optional, may be skipped)
    pub orphan_files: Option<OrphanFilesAnalysis>,
}

impl TableAnalysis {
    /// Check if any maintenance action is recommended
    pub fn needs_any_action(&self) -> bool {
        self.data_compaction.needs_action()
            || self.manifest_compaction.needs_action()
            || self.snapshot_expiration.needs_action()
            || self.orphan_files.as_ref().is_some_and(|o| o.needs_action())
    }
}

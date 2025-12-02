//! Common traits and types for metadata operations
//!
//! These abstractions allow maintenance operations to work with both
//! Delta Lake and Iceberg tables through a unified interface.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Information about a data file (format-agnostic)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFileInfo {
    /// Full path to the file
    pub path: String,
    /// File size in bytes
    pub size: u64,
    /// Number of records in the file
    pub record_count: u64,
    /// Partition values (key=value)
    pub partition: HashMap<String, String>,
}

/// Changes to data files in a transaction
#[derive(Debug, Clone, Default)]
pub struct DataFileChanges {
    /// Files to add to the table
    pub added: Vec<DataFileInfo>,
    /// Files to remove from the table
    pub removed: Vec<DataFileInfo>,
}

impl DataFileChanges {
    /// Create empty changes
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if there are any changes
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }

    /// Total bytes added
    pub fn bytes_added(&self) -> u64 {
        self.added.iter().map(|f| f.size).sum()
    }

    /// Total bytes removed
    pub fn bytes_removed(&self) -> u64 {
        self.removed.iter().map(|f| f.size).sum()
    }

    /// Total records added
    pub fn records_added(&self) -> u64 {
        self.added.iter().map(|f| f.record_count).sum()
    }

    /// Total records removed
    pub fn records_removed(&self) -> u64 {
        self.removed.iter().map(|f| f.record_count).sum()
    }
}

/// Type of operation being performed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationType {
    /// Append new data
    Append,
    /// Replace existing data (compaction, optimization)
    Replace,
    /// Delete data
    Delete,
    /// Overwrite entire table/partition
    Overwrite,
    /// Restore to previous version
    Restore,
    /// Repair metadata
    Repair,
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Append => write!(f, "APPEND"),
            Self::Replace => write!(f, "REPLACE"),
            Self::Delete => write!(f, "DELETE"),
            Self::Overwrite => write!(f, "OVERWRITE"),
            Self::Restore => write!(f, "RESTORE"),
            Self::Repair => write!(f, "REPAIR"),
        }
    }
}

/// Information about a snapshot/version (format-agnostic)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    /// Snapshot ID (version number for Delta, snapshot_id for Iceberg)
    pub id: i64,
    /// Timestamp in milliseconds
    pub timestamp_ms: i64,
    /// Operation type
    pub operation: String,
    /// Additional summary information
    pub summary: HashMap<String, String>,
    /// Parent snapshot ID (if any)
    pub parent_id: Option<i64>,
}

/// Trait for reading and writing table metadata transactionally
///
/// This trait abstracts the common operations needed for table maintenance:
/// - Reading current state (data files, snapshots)
/// - Writing new snapshots with file changes
/// - Listing historical snapshots
#[async_trait]
pub trait MetadataService: Send + Sync {
    /// Get information about the current snapshot
    async fn current_snapshot(&self) -> Result<Option<SnapshotInfo>>;

    /// List all data files in the current snapshot
    async fn list_data_files(&self) -> Result<Vec<DataFileInfo>>;

    /// List historical snapshots
    ///
    /// Returns snapshots ordered by timestamp (newest first)
    async fn list_snapshots(&self, limit: Option<usize>) -> Result<Vec<SnapshotInfo>>;

    /// Write a new snapshot with the given file changes
    ///
    /// This is the main transactional operation that:
    /// - Creates necessary metadata structures (manifests, etc.)
    /// - Records the file changes
    /// - Commits the new snapshot
    async fn write_snapshot(
        &self,
        changes: DataFileChanges,
        operation: OperationType,
        summary: HashMap<String, String>,
    ) -> Result<SnapshotInfo>;

    /// Get the table's data directory path
    fn data_directory(&self) -> std::path::PathBuf;

    /// Get the table's schema (as Arrow schema)
    async fn schema(&self) -> Result<std::sync::Arc<arrow::datatypes::Schema>>;
}

/// Result of a maintenance operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceResult {
    /// Number of files added
    pub files_added: usize,
    /// Number of files removed
    pub files_removed: usize,
    /// Bytes added
    pub bytes_added: u64,
    /// Bytes removed
    pub bytes_removed: u64,
    /// Records affected
    pub records_affected: u64,
    /// Operation name
    pub operation: String,
    /// Additional details
    pub details: HashMap<String, String>,
}

impl MaintenanceResult {
    /// Create a result indicating no changes were made
    pub fn no_changes(reason: &str) -> Self {
        let mut details = HashMap::new();
        details.insert("reason".to_string(), reason.to_string());
        Self {
            files_added: 0,
            files_removed: 0,
            bytes_added: 0,
            bytes_removed: 0,
            records_affected: 0,
            operation: "none".to_string(),
            details,
        }
    }

    /// Net change in bytes (positive = growth, negative = reduction)
    pub fn bytes_delta(&self) -> i64 {
        self.bytes_added as i64 - self.bytes_removed as i64
    }
}

/// Utility functions for formatting
pub mod utils {
    /// Format bytes to human-readable string
    pub fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} bytes", bytes)
        }
    }

    /// Generate a timestamp-based unique ID
    pub fn generate_unique_id() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }
}

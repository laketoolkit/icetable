//! Commit traits for Iceberg tables
//!
//! Defines the interface for committing changes to Iceberg tables.
//! Different implementations handle catalog vs static (no catalog) scenarios.

use std::sync::Arc;

use async_trait::async_trait;
use iceberg::spec::{Snapshot, TableMetadata};

use crate::error::Result;

/// Result of a successful commit operation
#[derive(Debug, Clone)]
pub struct CommitResult {
    /// The new table metadata after commit
    pub metadata: Arc<TableMetadata>,
    /// Path to the new metadata file
    pub metadata_location: String,
}

/// Trait for committing changes to Iceberg tables
///
/// This trait provides a unified interface for committing snapshots and metadata
/// changes, regardless of whether the table is managed by a catalog or accessed
/// directly (static table).
///
/// # Implementations
///
/// - `DirectCommitter`: Writes metadata directly to storage. No concurrency control.
/// - `CatalogCommitter`: Uses catalog API for atomic commits with concurrency control.
///
/// # Warning
///
/// `DirectCommitter` provides **no concurrency control**. Only use when you have
/// exclusive access to the table.
#[async_trait]
pub trait SnapshotCommitter: Send + Sync {
    /// Commit a new snapshot to the table
    ///
    /// This is the primary commit operation that:
    /// 1. Builds new metadata with the snapshot
    /// 2. Validates the metadata
    /// 3. Writes the new metadata file
    /// 4. (For catalogs) Updates the catalog atomically
    ///
    /// # Arguments
    ///
    /// * `current_metadata` - The current table metadata
    /// * `snapshot` - The new snapshot to commit
    /// * `branch` - The branch to commit to (usually "main")
    ///
    /// # Returns
    ///
    /// The new metadata and its location on success.
    async fn commit_snapshot(
        &self,
        current_metadata: Arc<TableMetadata>,
        snapshot: Snapshot,
        branch: &str,
    ) -> Result<CommitResult>;

    /// Commit updated metadata directly
    ///
    /// Use this for operations that don't involve snapshots,
    /// like schema evolution or property updates.
    async fn commit_metadata(
        &self,
        current_metadata: Arc<TableMetadata>,
        new_metadata: TableMetadata,
    ) -> Result<CommitResult>;

    /// Check if this committer supports concurrent writes
    ///
    /// Returns `false` for static tables (no concurrency control),
    /// `true` for catalog-managed tables.
    fn supports_concurrency(&self) -> bool;
}

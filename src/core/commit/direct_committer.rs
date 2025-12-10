//! Static table committer
//!
//! Commits changes directly to storage without a catalog.
//! This provides NO concurrency control - use only when you have exclusive access.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use iceberg::spec::{Snapshot, TableMetadata, TableMetadataBuilder};

use super::traits::{CommitResult, SnapshotCommitter};
use crate::core::storage::{ObjectStoreExt, Storage, to_path};
use crate::utils::core::{extract_version_from_path, find_latest_metadata, metadata_location_filename, next_metadata_location};
use crate::error::{Error, Result};

/// Committer for static tables (without catalog)
///
/// This writes metadata files directly to storage. It includes basic
/// conflict detection (checking if a new metadata file appeared), but
/// this is NOT atomic and does NOT prevent concurrent modifications.
///
/// # Warning
///
/// Only use this when:
/// - You have exclusive access to the table
/// - The table is not being written to by other processes
/// - You accept the risk of lost updates in concurrent scenarios
///
/// For production workloads with multiple writers, use a catalog.
pub struct DirectCommitter {
    /// Table location (e.g., "s3://bucket/table")
    table_path: String,
    /// Storage backend for writing metadata
    storage: Storage,
}

impl DirectCommitter {
    /// Create a new static committer
    pub fn new(table_path: String, storage: Storage) -> Self {
        Self {
            table_path: table_path.trim_end_matches('/').to_string(),
            storage,
        }
    }

    /// Get the metadata directory path (relative, for storage)
    fn metadata_dir_relative(&self) -> &'static str {
        "metadata"
    }

    /// Get the current metadata file path
    async fn current_metadata_path(&self) -> Result<String> {
        find_latest_metadata(&self.table_path, &self.storage).await
    }

    /// Check for conflicts before committing
    ///
    /// This is a basic optimistic concurrency check: if a new metadata file
    /// appeared since we read the current metadata, fail the commit.
    async fn check_conflicts(&self, expected_version: i32) -> Result<()> {
        let current_path = self.current_metadata_path().await?;
        let current_version = extract_version_from_path(&current_path).unwrap_or(0);

        if current_version != expected_version {
            return Err(Error::General(format!(
                "Conflict detected: expected version {}, but found {}. \
                 Another process may have modified the table.",
                expected_version, current_version
            )));
        }

        Ok(())
    }

    /// Generate the next metadata filename
    fn next_metadata_filename(&self, current_metadata_path: &str) -> Result<String> {
        let next_location = next_metadata_location(current_metadata_path)?;
        Ok(metadata_location_filename(&next_location))
    }

    /// Write metadata to storage using relative path
    async fn write_metadata(&self, metadata: &TableMetadata, filename: &str) -> Result<String> {
        let metadata_json = serde_json::to_string_pretty(metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        // Use relative path for storage (PrefixStore handles the rest)
        let storage_path = format!("{}/{}", self.metadata_dir_relative(), filename);

        self.storage
            .put_bytes(&to_path(&storage_path), Bytes::from(metadata_json))
            .await?;

        // Return absolute path for result
        Ok(format!("{}/metadata/{}", self.table_path, filename))
    }

    /// Build updated metadata with a new snapshot on a branch
    fn build_metadata_with_snapshot(
        &self,
        current_metadata: &TableMetadata,
        snapshot: Snapshot,
        current_metadata_path: &str,
        branch: &str,
    ) -> Result<TableMetadata> {
        // Extract just the filename for the metadata log
        let metadata_filename = current_metadata_path
            .split('/')
            .next_back()
            .unwrap_or(current_metadata_path);

        let build_result = TableMetadataBuilder::new_from_metadata(
            current_metadata.clone(),
            Some(metadata_filename.to_string()),
        )
        .set_branch_snapshot(snapshot, branch)
        .map_err(|e| Error::General(format!("Failed to set snapshot: {}", e)))?
        .build()
        .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        Ok(build_result.metadata)
    }
}

#[async_trait]
impl SnapshotCommitter for DirectCommitter {
    async fn commit_snapshot(
        &self,
        current_metadata: Arc<TableMetadata>,
        snapshot: Snapshot,
        branch: &str,
    ) -> Result<CommitResult> {
        // 1. Get current metadata path and version
        let current_metadata_path = self.current_metadata_path().await?;
        let expected_version = extract_version_from_path(&current_metadata_path).unwrap_or(0);

        // 2. Check for conflicts (optimistic concurrency)
        self.check_conflicts(expected_version).await?;

        // 3. Build new metadata with snapshot
        let new_metadata = self.build_metadata_with_snapshot(
            &current_metadata,
            snapshot,
            &current_metadata_path,
            branch,
        )?;

        // 4. Validate metadata before writing
        crate::core::metadata::validate_or_error(&new_metadata)?;

        // 5. Generate new metadata filename
        let new_metadata_filename = self.next_metadata_filename(&current_metadata_path)?;

        // 6. Write to storage and get absolute path
        let new_metadata_path = self.write_metadata(&new_metadata, &new_metadata_filename).await?;

        Ok(CommitResult {
            metadata: Arc::new(new_metadata),
            metadata_location: new_metadata_path,
        })
    }

    async fn commit_metadata(
        &self,
        _current_metadata: Arc<TableMetadata>,
        new_metadata: TableMetadata,
    ) -> Result<CommitResult> {
        // 1. Get current metadata path and version
        let current_metadata_path = self.current_metadata_path().await?;
        let expected_version = extract_version_from_path(&current_metadata_path).unwrap_or(0);

        // 2. Check for conflicts
        self.check_conflicts(expected_version).await?;

        // 3. Validate metadata
        crate::core::metadata::validate_or_error(&new_metadata)?;

        // 4. Generate new metadata filename
        let new_metadata_filename = self.next_metadata_filename(&current_metadata_path)?;

        // 5. Write to storage
        let new_metadata_path = self.write_metadata(&new_metadata, &new_metadata_filename).await?;

        Ok(CommitResult {
            metadata: Arc::new(new_metadata),
            metadata_location: new_metadata_path,
        })
    }

    fn supports_concurrency(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::local::LocalFileSystem;

    #[test]
    fn test_metadata_dir_relative() {
        let storage: Storage = Arc::new(LocalFileSystem::new());
        let committer = DirectCommitter::new("s3://bucket/table".to_string(), storage);
        assert_eq!(committer.metadata_dir_relative(), "metadata");
    }

    #[test]
    fn test_supports_concurrency() {
        let storage: Storage = Arc::new(LocalFileSystem::new());
        let committer = DirectCommitter::new("s3://bucket/table".to_string(), storage);
        assert!(!committer.supports_concurrency());
    }
}

//! Reference (branch/tag) management service
//!
//! Provides operations for managing Iceberg table references:
//! - Create/delete/rename branches
//! - Create/delete/rename tags
//!
//! When a `TableCommitter` is provided, commits go through the REST catalog
//! API for multi-writer safety. Otherwise, commits write directly to storage.

use iceberg::spec::{SnapshotReference, SnapshotRetention};

use crate::core::catalog::TableCommitter;
use crate::core::metadata::IcebergMetadataService;
use crate::core::storage::create_object_store;
use crate::error::{Error, Result};

/// Result of a reference operation
#[derive(Debug)]
pub struct RefResult {
    /// Name of the reference
    pub name: String,
    /// Snapshot ID the reference points to
    pub snapshot_id: i64,
    /// New metadata version (None if dry run)
    pub new_version: Option<i64>,
    /// Whether this was a dry run
    pub dry_run: bool,
}

/// Configuration for reference operations
#[derive(Debug, Clone, Default)]
pub struct RefConfig {
    /// Dry run mode - don't make changes
    pub dry_run: bool,
}

/// Branch retention configuration
#[derive(Debug, Clone, Default)]
pub struct BranchRetention {
    /// Minimum number of snapshots to keep
    pub min_snapshots_to_keep: Option<i32>,
    /// Maximum snapshot age in milliseconds
    pub max_snapshot_age_ms: Option<i64>,
    /// Maximum reference age in milliseconds
    pub max_ref_age_ms: Option<i64>,
}

/// Service for managing branches and tags
pub struct RefService {
    config: RefConfig,
    /// Optional committer for catalog-aware commits
    committer: Option<TableCommitter>,
}

impl RefService {
    /// Create a new reference service
    pub fn new() -> Self {
        Self {
            config: RefConfig::default(),
            committer: None,
        }
    }

    /// Create with configuration
    pub fn with_config(config: RefConfig) -> Self {
        Self {
            config,
            committer: None,
        }
    }

    /// Create with committer for catalog-aware operations
    pub fn with_committer(committer: TableCommitter) -> Self {
        Self {
            config: RefConfig::default(),
            committer: Some(committer),
        }
    }

    /// Create with both config and committer
    pub fn with_config_and_committer(config: RefConfig, committer: Option<TableCommitter>) -> Self {
        Self { config, committer }
    }

    /// Create a new branch
    pub async fn create_branch(
        &self,
        service: &IcebergMetadataService,
        table_path: &str,
        name: &str,
        snapshot_id: Option<i64>,
        retention: BranchRetention,
    ) -> Result<RefResult> {
        let (metadata, current_version) = service.load_metadata().await?;

        // Use specified snapshot or current
        let target_id = snapshot_id.unwrap_or_else(|| {
            metadata
                .current_snapshot_id()
                .expect("Table has no current snapshot")
        });

        // Verify snapshot exists
        metadata
            .snapshot_by_id(target_id)
            .ok_or_else(|| Error::General(format!("Snapshot {} not found", target_id)))?;

        // Check if ref already exists
        if metadata.snapshot_for_ref(name).is_some() {
            return Err(Error::General(format!(
                "Reference '{}' already exists",
                name
            )));
        }

        if self.config.dry_run {
            return Ok(RefResult {
                name: name.to_string(),
                snapshot_id: target_id,
                new_version: None,
                dry_run: true,
            });
        }

        let branch_ref = SnapshotReference {
            snapshot_id: target_id,
            retention: SnapshotRetention::Branch {
                min_snapshots_to_keep: retention.min_snapshots_to_keep,
                max_snapshot_age_ms: retention.max_snapshot_age_ms,
                max_ref_age_ms: retention.max_ref_age_ms,
            },
        };

        // Use committer if available (catalog mode)
        let new_version = if let Some(ref committer) = self.committer {
            committer
                .commit_add_ref(table_path, &metadata, name, branch_ref, current_version)
                .await?
        } else {
            // Direct mode: build and write new metadata
            let metadata_file_path = service.current_metadata_path().await?;
            let metadata_clone = (*metadata).clone();

            let build_result = metadata_clone
                .into_builder(Some(metadata_file_path))
                .set_ref(name, branch_ref)
                .map_err(|e| Error::General(format!("Failed to set branch: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

            self.write_metadata(table_path, &build_result.metadata, current_version)
                .await?
        };

        Ok(RefResult {
            name: name.to_string(),
            snapshot_id: target_id,
            new_version: Some(new_version),
            dry_run: false,
        })
    }

    /// Create a new tag
    pub async fn create_tag(
        &self,
        service: &IcebergMetadataService,
        table_path: &str,
        name: &str,
        snapshot_id: Option<i64>,
        max_ref_age_ms: Option<i64>,
    ) -> Result<RefResult> {
        let (metadata, current_version) = service.load_metadata().await?;

        // Use specified snapshot or current
        let target_id = snapshot_id.unwrap_or_else(|| {
            metadata
                .current_snapshot_id()
                .expect("Table has no current snapshot")
        });

        // Verify snapshot exists
        metadata
            .snapshot_by_id(target_id)
            .ok_or_else(|| Error::General(format!("Snapshot {} not found", target_id)))?;

        // Check if ref already exists
        if metadata.snapshot_for_ref(name).is_some() {
            return Err(Error::General(format!(
                "Reference '{}' already exists",
                name
            )));
        }

        if self.config.dry_run {
            return Ok(RefResult {
                name: name.to_string(),
                snapshot_id: target_id,
                new_version: None,
                dry_run: true,
            });
        }

        let tag_ref = SnapshotReference {
            snapshot_id: target_id,
            retention: SnapshotRetention::Tag { max_ref_age_ms },
        };

        // Use committer if available (catalog mode)
        let new_version = if let Some(ref committer) = self.committer {
            committer
                .commit_add_ref(table_path, &metadata, name, tag_ref, current_version)
                .await?
        } else {
            // Direct mode: build and write new metadata
            let metadata_file_path = service.current_metadata_path().await?;
            let metadata_clone = (*metadata).clone();

            let build_result = metadata_clone
                .into_builder(Some(metadata_file_path))
                .set_ref(name, tag_ref)
                .map_err(|e| Error::General(format!("Failed to set tag: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

            self.write_metadata(table_path, &build_result.metadata, current_version)
                .await?
        };

        Ok(RefResult {
            name: name.to_string(),
            snapshot_id: target_id,
            new_version: Some(new_version),
            dry_run: false,
        })
    }

    /// Delete a reference (branch or tag)
    pub async fn delete_ref(
        &self,
        service: &IcebergMetadataService,
        table_path: &str,
        name: &str,
    ) -> Result<RefResult> {
        if name == "main" {
            return Err(Error::General("Cannot delete 'main' branch".to_string()));
        }

        let (metadata, current_version) = service.load_metadata().await?;

        // Verify ref exists and get its snapshot
        let snapshot = metadata
            .snapshot_for_ref(name)
            .ok_or_else(|| Error::General(format!("Reference '{}' not found", name)))?;
        let snapshot_id = snapshot.snapshot_id();

        if self.config.dry_run {
            return Ok(RefResult {
                name: name.to_string(),
                snapshot_id,
                new_version: None,
                dry_run: true,
            });
        }

        // Use committer if available (catalog mode)
        let new_version = if let Some(ref committer) = self.committer {
            committer
                .commit_remove_ref(table_path, &metadata, name, current_version)
                .await?
        } else {
            // Direct mode: build and write new metadata
            let metadata_file_path = service.current_metadata_path().await?;
            let metadata_clone = (*metadata).clone();

            let build_result = metadata_clone
                .into_builder(Some(metadata_file_path))
                .remove_ref(name)
                .build()
                .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

            self.write_metadata(table_path, &build_result.metadata, current_version)
                .await?
        };

        Ok(RefResult {
            name: name.to_string(),
            snapshot_id,
            new_version: Some(new_version),
            dry_run: false,
        })
    }

    /// Rename a branch
    pub async fn rename_branch(
        &self,
        service: &IcebergMetadataService,
        table_path: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<RefResult> {
        if old_name == "main" {
            return Err(Error::General("Cannot rename 'main' branch".to_string()));
        }
        if new_name == "main" {
            return Err(Error::General("Cannot rename to 'main'".to_string()));
        }

        let (metadata, current_version) = service.load_metadata().await?;

        // Verify old ref exists
        let snapshot = metadata
            .snapshot_for_ref(old_name)
            .ok_or_else(|| Error::General(format!("Branch '{}' not found", old_name)))?;
        let snapshot_id = snapshot.snapshot_id();

        // Verify new name doesn't exist
        if metadata.snapshot_for_ref(new_name).is_some() {
            return Err(Error::General(format!(
                "Reference '{}' already exists",
                new_name
            )));
        }

        if self.config.dry_run {
            return Ok(RefResult {
                name: new_name.to_string(),
                snapshot_id,
                new_version: None,
                dry_run: true,
            });
        }

        // Create new branch ref with default retention
        let new_ref = SnapshotReference {
            snapshot_id,
            retention: SnapshotRetention::Branch {
                min_snapshots_to_keep: None,
                max_snapshot_age_ms: None,
                max_ref_age_ms: None,
            },
        };

        // Use committer if available (catalog mode)
        let new_version = if let Some(ref committer) = self.committer {
            committer
                .commit_rename_ref(
                    table_path,
                    &metadata,
                    old_name,
                    new_name,
                    new_ref,
                    current_version,
                )
                .await?
        } else {
            // Direct mode: build and write new metadata
            let metadata_file_path = service.current_metadata_path().await?;
            let metadata_clone = (*metadata).clone();

            let build_result = metadata_clone
                .into_builder(Some(metadata_file_path))
                .remove_ref(old_name)
                .set_ref(new_name, new_ref)
                .map_err(|e| Error::General(format!("Failed to set reference: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

            self.write_metadata(table_path, &build_result.metadata, current_version)
                .await?
        };

        Ok(RefResult {
            name: new_name.to_string(),
            snapshot_id,
            new_version: Some(new_version),
            dry_run: false,
        })
    }

    /// Rename a tag
    pub async fn rename_tag(
        &self,
        service: &IcebergMetadataService,
        table_path: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<RefResult> {
        let (metadata, current_version) = service.load_metadata().await?;

        // Verify old ref exists
        let snapshot = metadata
            .snapshot_for_ref(old_name)
            .ok_or_else(|| Error::General(format!("Tag '{}' not found", old_name)))?;
        let snapshot_id = snapshot.snapshot_id();

        // Verify new name doesn't exist
        if metadata.snapshot_for_ref(new_name).is_some() {
            return Err(Error::General(format!(
                "Reference '{}' already exists",
                new_name
            )));
        }

        if self.config.dry_run {
            return Ok(RefResult {
                name: new_name.to_string(),
                snapshot_id,
                new_version: None,
                dry_run: true,
            });
        }

        // Create new tag ref with default retention
        let new_ref = SnapshotReference {
            snapshot_id,
            retention: SnapshotRetention::Tag {
                max_ref_age_ms: None,
            },
        };

        // Use committer if available (catalog mode)
        let new_version = if let Some(ref committer) = self.committer {
            committer
                .commit_rename_ref(
                    table_path,
                    &metadata,
                    old_name,
                    new_name,
                    new_ref,
                    current_version,
                )
                .await?
        } else {
            // Direct mode: build and write new metadata
            let metadata_file_path = service.current_metadata_path().await?;
            let metadata_clone = (*metadata).clone();

            let build_result = metadata_clone
                .into_builder(Some(metadata_file_path))
                .remove_ref(old_name)
                .set_ref(new_name, new_ref)
                .map_err(|e| Error::General(format!("Failed to set reference: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

            self.write_metadata(table_path, &build_result.metadata, current_version)
                .await?
        };

        Ok(RefResult {
            name: new_name.to_string(),
            snapshot_id,
            new_version: Some(new_version),
            dry_run: false,
        })
    }

    /// Fast-forward a branch to another snapshot
    ///
    /// A fast-forward is only valid if the target snapshot is a descendant of the
    /// branch's current snapshot (i.e., the current snapshot is an ancestor of the target).
    pub async fn fast_forward_branch(
        &self,
        service: &IcebergMetadataService,
        table_path: &str,
        name: &str,
        target: &str, // snapshot ID or ref name
    ) -> Result<RefResult> {
        use crate::utils::core::snapshot::is_ancestor;

        let (metadata, current_version) = service.load_metadata().await?;

        // Verify branch exists and get its current snapshot
        let branch_snapshot = metadata
            .snapshot_for_ref(name)
            .ok_or_else(|| Error::General(format!("Branch '{}' not found", name)))?;
        let branch_snapshot_id = branch_snapshot.snapshot_id();

        // Resolve target to snapshot ID
        let target_id: i64 = if let Ok(id) = target.parse() {
            metadata
                .snapshot_by_id(id)
                .ok_or_else(|| Error::General(format!("Snapshot {} not found", id)))?;
            id
        } else if let Some(snap) = metadata.snapshot_for_ref(target) {
            snap.snapshot_id()
        } else {
            return Err(Error::General(format!("Reference '{}' not found", target)));
        };

        // Verify this is a valid fast-forward: target must be a descendant of current
        // (i.e., current must be an ancestor of target)
        if !is_ancestor(&metadata, branch_snapshot_id, target_id) {
            return Err(Error::General(format!(
                "Cannot fast-forward: snapshot {} is not a descendant of branch '{}' (snapshot {}). \
                The branches have diverged.",
                target_id, name, branch_snapshot_id
            )));
        }

        if self.config.dry_run {
            return Ok(RefResult {
                name: name.to_string(),
                snapshot_id: target_id,
                new_version: None,
                dry_run: true,
            });
        }

        let new_ref = SnapshotReference {
            snapshot_id: target_id,
            retention: SnapshotRetention::Branch {
                min_snapshots_to_keep: None,
                max_snapshot_age_ms: None,
                max_ref_age_ms: None,
            },
        };

        // Use committer if available (catalog mode)
        let new_version = if let Some(ref committer) = self.committer {
            committer
                .commit_add_ref(table_path, &metadata, name, new_ref, current_version)
                .await?
        } else {
            // Direct mode: build and write new metadata
            let metadata_file_path = service.current_metadata_path().await?;
            let metadata_clone = (*metadata).clone();

            let build_result = metadata_clone
                .into_builder(Some(metadata_file_path))
                .set_ref(name, new_ref)
                .map_err(|e| Error::General(format!("Failed to update branch: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

            self.write_metadata(table_path, &build_result.metadata, current_version)
                .await?
        };

        Ok(RefResult {
            name: name.to_string(),
            snapshot_id: target_id,
            new_version: Some(new_version),
            dry_run: false,
        })
    }

    /// Write new metadata using standard Iceberg naming
    async fn write_metadata(
        &self,
        table_path: &str,
        metadata: &iceberg::spec::TableMetadata,
        _current_version: i32, // Kept for API compatibility, version derived from metadata path
    ) -> Result<i64> {
        let storage = create_object_store(table_path).await?;
        let result = crate::utils::core::write_metadata_file(table_path, metadata, &storage).await?;
        Ok(result.version)
    }
}

impl Default for RefService {
    fn default() -> Self {
        Self::new()
    }
}

//! Reference (branch/tag) management service
//!
//! Provides operations for managing Iceberg table references:
//! - Create/delete/rename branches
//! - Create/delete/rename tags
//!
//! Uses `MetadataServiceWriter` trait for catalog-aware commits.

use std::sync::Arc;

use iceberg::spec::{SnapshotReference, SnapshotRetention, TableMetadata};

use crate::core::metadata::MetadataServiceWriter;
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
///
/// This service uses `MetadataServiceWriter` trait for catalog-aware operations.
/// The committer is obtained from the service, not stored in the struct.
pub struct RefService {
    config: RefConfig,
}

/// Helper context for ref operations
struct RefContext<'a> {
    metadata: Arc<TableMetadata>,
    table_path: &'a str,
    current_version: i32,
}

impl<'a> RefContext<'a> {
    /// Load context from service
    async fn load<S: MetadataServiceWriter>(service: &'a S) -> Result<RefContext<'a>> {
        let (metadata, current_version) = service.load_metadata().await?;
        Ok(RefContext {
            metadata,
            table_path: service.table_path(),
            current_version,
        })
    }

    /// Resolve target snapshot ID (from explicit ID or current snapshot)
    fn resolve_target_snapshot(&self, snapshot_id: Option<i64>) -> Result<i64> {
        let target_id = match snapshot_id {
            Some(id) => id,
            None => self
                .metadata
                .current_snapshot_id()
                .ok_or_else(|| Error::Metadata {
                    message: "Table has no current snapshot. Specify a snapshot ID explicitly."
                        .to_string(),
                })?,
        };

        // Verify snapshot exists
        self.metadata
            .snapshot_by_id(target_id)
            .ok_or_else(|| Error::SnapshotNotFound {
                snapshot_id: target_id,
            })?;

        Ok(target_id)
    }

    /// Check that a reference doesn't already exist
    fn ensure_ref_not_exists(&self, name: &str) -> Result<()> {
        if self.metadata.snapshot_for_ref(name).is_some() {
            return Err(Error::Conflict(format!(
                "Reference '{}' already exists",
                name
            )));
        }
        Ok(())
    }

    /// Get snapshot ID for an existing reference
    fn get_ref_snapshot(&self, name: &str, ref_type: &str) -> Result<i64> {
        let snapshot = self
            .metadata
            .snapshot_for_ref(name)
            .ok_or_else(|| Error::Metadata {
                message: format!("{} '{}' not found", ref_type, name),
            })?;
        Ok(snapshot.snapshot_id())
    }
}

/// Write metadata directly (without catalog) for a ref operation
async fn write_ref_direct<S: MetadataServiceWriter, F>(
    service: &S,
    metadata: &TableMetadata,
    table_path: &str,
    apply_changes: F,
) -> Result<i64>
where
    F: FnOnce(
        iceberg::spec::TableMetadataBuilder,
    ) -> std::result::Result<iceberg::spec::TableMetadataBuilder, iceberg::Error>,
{
    let metadata_file_path = service.current_metadata_path().await?;
    let metadata_clone = metadata.clone();

    let builder = metadata_clone.into_builder(Some(metadata_file_path));
    let builder = apply_changes(builder).map_err(|e| Error::Metadata {
        message: format!("Failed to apply ref changes: {}", e),
    })?;

    let build_result = builder.build().map_err(|e| Error::Metadata {
        message: format!("Failed to build metadata: {}", e),
    })?;

    super::write_metadata_direct(table_path, &build_result.metadata).await
}

impl RefService {
    /// Create a new reference service
    pub fn new() -> Self {
        Self {
            config: RefConfig::default(),
        }
    }

    /// Create with configuration
    pub fn with_config(config: RefConfig) -> Self {
        Self { config }
    }

    /// Create a new branch
    ///
    /// Uses `MetadataServiceWriter` trait to access metadata and commit changes.
    pub async fn create_branch<S: MetadataServiceWriter>(
        &self,
        service: &S,
        name: &str,
        snapshot_id: Option<i64>,
        retention: BranchRetention,
    ) -> Result<RefResult> {
        let ctx = RefContext::load(service).await?;
        let target_id = ctx.resolve_target_snapshot(snapshot_id)?;
        ctx.ensure_ref_not_exists(name)?;

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

        let new_version = if let Some(committer) = service.committer() {
            committer
                .commit_add_ref(
                    ctx.table_path,
                    &ctx.metadata,
                    name,
                    branch_ref,
                    ctx.current_version,
                )
                .await?
        } else {
            let name = name.to_string();
            write_ref_direct(service, &ctx.metadata, ctx.table_path, |b| {
                b.set_ref(&name, branch_ref)
            })
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
    ///
    /// Uses `MetadataServiceWriter` trait to access metadata and commit changes.
    pub async fn create_tag<S: MetadataServiceWriter>(
        &self,
        service: &S,
        name: &str,
        snapshot_id: Option<i64>,
        max_ref_age_ms: Option<i64>,
    ) -> Result<RefResult> {
        let ctx = RefContext::load(service).await?;
        let target_id = ctx.resolve_target_snapshot(snapshot_id)?;
        ctx.ensure_ref_not_exists(name)?;

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

        let new_version = if let Some(committer) = service.committer() {
            committer
                .commit_add_ref(
                    ctx.table_path,
                    &ctx.metadata,
                    name,
                    tag_ref,
                    ctx.current_version,
                )
                .await?
        } else {
            let name = name.to_string();
            write_ref_direct(service, &ctx.metadata, ctx.table_path, |b| {
                b.set_ref(&name, tag_ref)
            })
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
    ///
    /// Uses `MetadataServiceWriter` trait to access metadata and commit changes.
    pub async fn delete_ref<S: MetadataServiceWriter>(
        &self,
        service: &S,
        name: &str,
    ) -> Result<RefResult> {
        if name == "main" {
            return Err(Error::InvalidFormat {
                message: "Cannot delete 'main' branch".to_string(),
            });
        }

        let ctx = RefContext::load(service).await?;
        let snapshot_id = ctx.get_ref_snapshot(name, "Reference")?;

        if self.config.dry_run {
            return Ok(RefResult {
                name: name.to_string(),
                snapshot_id,
                new_version: None,
                dry_run: true,
            });
        }

        let new_version = if let Some(committer) = service.committer() {
            committer
                .commit_remove_ref(ctx.table_path, &ctx.metadata, name, ctx.current_version)
                .await?
        } else {
            let name = name.to_string();
            write_ref_direct(service, &ctx.metadata, ctx.table_path, |b| {
                Ok(b.remove_ref(&name))
            })
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
    ///
    /// Uses `MetadataServiceWriter` trait to access metadata and commit changes.
    pub async fn rename_branch<S: MetadataServiceWriter>(
        &self,
        service: &S,
        old_name: &str,
        new_name: &str,
    ) -> Result<RefResult> {
        if old_name == "main" {
            return Err(Error::InvalidFormat {
                message: "Cannot rename 'main' branch".to_string(),
            });
        }
        if new_name == "main" {
            return Err(Error::InvalidFormat {
                message: "Cannot rename to 'main'".to_string(),
            });
        }

        let ctx = RefContext::load(service).await?;
        let snapshot_id = ctx.get_ref_snapshot(old_name, "Branch")?;
        ctx.ensure_ref_not_exists(new_name)?;

        if self.config.dry_run {
            return Ok(RefResult {
                name: new_name.to_string(),
                snapshot_id,
                new_version: None,
                dry_run: true,
            });
        }

        let new_ref = SnapshotReference {
            snapshot_id,
            retention: SnapshotRetention::Branch {
                min_snapshots_to_keep: None,
                max_snapshot_age_ms: None,
                max_ref_age_ms: None,
            },
        };

        let new_version = if let Some(committer) = service.committer() {
            committer
                .commit_rename_ref(
                    ctx.table_path,
                    &ctx.metadata,
                    old_name,
                    new_name,
                    new_ref,
                    ctx.current_version,
                )
                .await?
        } else {
            let old_name = old_name.to_string();
            let new_name_owned = new_name.to_string();
            write_ref_direct(service, &ctx.metadata, ctx.table_path, |b| {
                b.remove_ref(&old_name).set_ref(&new_name_owned, new_ref)
            })
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
    ///
    /// Uses `MetadataServiceWriter` trait to access metadata and commit changes.
    pub async fn rename_tag<S: MetadataServiceWriter>(
        &self,
        service: &S,
        old_name: &str,
        new_name: &str,
    ) -> Result<RefResult> {
        let ctx = RefContext::load(service).await?;
        let snapshot_id = ctx.get_ref_snapshot(old_name, "Tag")?;
        ctx.ensure_ref_not_exists(new_name)?;

        if self.config.dry_run {
            return Ok(RefResult {
                name: new_name.to_string(),
                snapshot_id,
                new_version: None,
                dry_run: true,
            });
        }

        let new_ref = SnapshotReference {
            snapshot_id,
            retention: SnapshotRetention::Tag {
                max_ref_age_ms: None,
            },
        };

        let new_version = if let Some(committer) = service.committer() {
            committer
                .commit_rename_ref(
                    ctx.table_path,
                    &ctx.metadata,
                    old_name,
                    new_name,
                    new_ref,
                    ctx.current_version,
                )
                .await?
        } else {
            let old_name = old_name.to_string();
            let new_name_owned = new_name.to_string();
            write_ref_direct(service, &ctx.metadata, ctx.table_path, |b| {
                b.remove_ref(&old_name).set_ref(&new_name_owned, new_ref)
            })
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
    ///
    /// Uses `MetadataServiceWriter` trait to access metadata and commit changes.
    pub async fn fast_forward_branch<S: MetadataServiceWriter>(
        &self,
        service: &S,
        name: &str,
        target: &str, // snapshot ID or ref name
    ) -> Result<RefResult> {
        use crate::utils::core::snapshot::is_ancestor;

        let ctx = RefContext::load(service).await?;
        let branch_snapshot_id = ctx.get_ref_snapshot(name, "Branch")?;

        // Resolve target to snapshot ID
        let target_id: i64 = if let Ok(id) = target.parse() {
            ctx.metadata
                .snapshot_by_id(id)
                .ok_or_else(|| Error::SnapshotNotFound { snapshot_id: id })?;
            id
        } else if let Some(snap) = ctx.metadata.snapshot_for_ref(target) {
            snap.snapshot_id()
        } else {
            return Err(Error::Metadata {
                message: format!("Reference '{}' not found", target),
            });
        };

        // Verify this is a valid fast-forward: target must be a descendant of current
        if !is_ancestor(&ctx.metadata, branch_snapshot_id, target_id) {
            return Err(Error::Conflict(format!(
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

        let new_version = if let Some(committer) = service.committer() {
            committer
                .commit_add_ref(
                    ctx.table_path,
                    &ctx.metadata,
                    name,
                    new_ref,
                    ctx.current_version,
                )
                .await?
        } else {
            let name = name.to_string();
            write_ref_direct(service, &ctx.metadata, ctx.table_path, |b| {
                b.set_ref(&name, new_ref)
            })
            .await?
        };

        Ok(RefResult {
            name: name.to_string(),
            snapshot_id: target_id,
            new_version: Some(new_version),
            dry_run: false,
        })
    }
}

impl Default for RefService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ref_service_new() {
        let service = RefService::new();
        assert!(!service.config.dry_run);
    }

    #[test]
    fn test_ref_service_with_config() {
        let config = RefConfig { dry_run: true };
        let service = RefService::with_config(config);
        assert!(service.config.dry_run);
    }

    #[test]
    fn test_ref_config_default() {
        let config = RefConfig::default();
        assert!(!config.dry_run);
    }

    #[test]
    fn test_branch_retention_default() {
        let retention = BranchRetention::default();
        assert!(retention.min_snapshots_to_keep.is_none());
        assert!(retention.max_snapshot_age_ms.is_none());
        assert!(retention.max_ref_age_ms.is_none());
    }

    #[test]
    fn test_branch_retention_custom() {
        let retention = BranchRetention {
            min_snapshots_to_keep: Some(5),
            max_snapshot_age_ms: Some(86400000), // 1 day
            max_ref_age_ms: Some(604800000),     // 7 days
        };
        assert_eq!(retention.min_snapshots_to_keep, Some(5));
        assert_eq!(retention.max_snapshot_age_ms, Some(86400000));
        assert_eq!(retention.max_ref_age_ms, Some(604800000));
    }

    #[test]
    fn test_ref_result_struct() {
        let result = RefResult {
            name: "feature-branch".to_string(),
            snapshot_id: 12345,
            new_version: Some(2),
            dry_run: false,
        };
        assert_eq!(result.name, "feature-branch");
        assert_eq!(result.snapshot_id, 12345);
        assert_eq!(result.new_version, Some(2));
        assert!(!result.dry_run);
    }

    #[test]
    fn test_ref_result_dry_run() {
        let result = RefResult {
            name: "test-branch".to_string(),
            snapshot_id: 67890,
            new_version: None,
            dry_run: true,
        };
        assert!(result.dry_run);
        assert!(result.new_version.is_none());
    }
}

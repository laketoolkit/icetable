//! Manifest rewrite service
//!
//! Provides functionality to rewrite and compact Iceberg manifest files.

use std::collections::HashMap;
use std::sync::Arc;

use iceberg::spec::{
    ManifestContentType, ManifestEntry, ManifestFile, ManifestListWriter, ManifestStatus,
    ManifestWriterBuilder, Operation, Snapshot, SnapshotReference, SnapshotRetention, Summary,
    TableMetadata,
};

use crate::core::catalog::TableCommitter;
use crate::core::metadata::IcebergMetadataService;
use crate::core::storage::create_object_store;
use crate::error::{Error, Result};

/// Configuration for manifest rewrite operations
#[derive(Debug, Clone)]
pub struct ManifestConfig {
    /// Target size for manifest files in bytes (default: 8MB)
    pub target_size: u64,
    /// Minimum number of manifests before triggering rewrite
    pub min_manifests: usize,
    /// Whether to run in dry-run mode
    pub dry_run: bool,
    /// Target branch for the operation
    pub branch: Option<String>,
}

impl Default for ManifestConfig {
    fn default() -> Self {
        Self {
            target_size: 8 * 1024 * 1024, // 8MB
            min_manifests: 3,
            dry_run: false,
            branch: None,
        }
    }
}

/// Result of manifest analysis (for dry-run)
#[derive(Debug, Clone)]
pub struct ManifestAnalysis {
    /// Current number of manifests
    pub current_manifests: usize,
    /// Number of data manifests
    pub data_manifests: usize,
    /// Number of delete manifests
    pub delete_manifests: usize,
    /// Total entries across all data manifests
    pub total_entries: usize,
    /// Estimated number of manifests after rewrite
    pub estimated_after: usize,
    /// Whether rewrite would be beneficial
    pub should_rewrite: bool,
    /// Reason if not rewriting
    pub skip_reason: Option<String>,
}

/// Result of manifest rewrite operation
#[derive(Debug, Clone)]
pub struct ManifestRewriteResult {
    /// Previous number of manifests
    pub previous_manifests: usize,
    /// New number of manifests
    pub new_manifests: usize,
    /// Number of data manifests rewritten
    pub data_manifests_rewritten: usize,
    /// Number of delete manifests kept
    pub delete_manifests_kept: usize,
    /// Total entries processed
    pub total_entries: usize,
    /// New snapshot ID
    pub snapshot_id: i64,
    /// New metadata version
    pub metadata_version: u32,
}

/// Context for committing a snapshot
struct CommitContext<'a> {
    table_path: &'a str,
    metadata_dir: &'a str,
    metadata_file_path: &'a str,
    metadata: &'a Arc<TableMetadata>,
    new_snapshot: Snapshot,
    target_branch: &'a str,
    new_snapshot_id: i64,
    committer: Option<TableCommitter>,
}

/// Builder for creating a new snapshot for manifest rewrite
struct SnapshotBuilder<'a> {
    new_snapshot_id: i64,
    parent_snapshot_id: i64,
    sequence_number: i64,
    manifest_list_path: Option<&'a str>,
    new_manifest_files: Option<&'a [ManifestFile]>,
    data_manifests: Option<&'a [ManifestFile]>,
    final_manifest_count: usize,
    original_summary: Option<&'a Summary>,
    schema_id: i32,
}

impl<'a> SnapshotBuilder<'a> {
    fn new(new_snapshot_id: i64, parent_snapshot_id: i64, sequence_number: i64) -> Self {
        Self {
            new_snapshot_id,
            parent_snapshot_id,
            sequence_number,
            manifest_list_path: None,
            new_manifest_files: None,
            data_manifests: None,
            final_manifest_count: 0,
            original_summary: None,
            schema_id: 0,
        }
    }

    fn manifest_list_path(mut self, path: &'a str) -> Self {
        self.manifest_list_path = Some(path);
        self
    }

    fn new_manifest_files(mut self, files: &'a [ManifestFile]) -> Self {
        self.new_manifest_files = Some(files);
        self.final_manifest_count = files.len();
        self
    }

    fn data_manifests(mut self, manifests: &'a [ManifestFile]) -> Self {
        self.data_manifests = Some(manifests);
        self
    }

    fn original_summary(mut self, summary: &'a Summary) -> Self {
        self.original_summary = Some(summary);
        self
    }

    fn schema_id(mut self, id: i32) -> Self {
        self.schema_id = id;
        self
    }

    fn build(self) -> Snapshot {
        let new_manifest_files = self.new_manifest_files.unwrap_or(&[]);
        let data_manifests = self.data_manifests.unwrap_or(&[]);
        let manifest_list_path = self.manifest_list_path.unwrap_or("");

        // Calculate statistics
        let total_data_files: u64 = new_manifest_files
            .iter()
            .filter(|m| m.content == ManifestContentType::Data)
            .map(|m| {
                m.added_files_count.unwrap_or(0) as u64 + m.existing_files_count.unwrap_or(0) as u64
            })
            .sum();

        let total_rows: u64 = new_manifest_files
            .iter()
            .filter(|m| m.content == ManifestContentType::Data)
            .map(|m| m.added_rows_count.unwrap_or(0) + m.existing_rows_count.unwrap_or(0))
            .sum();

        let total_files_size: u64 = self
            .original_summary
            .and_then(|s| s.additional_properties.get("total-files-size"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Build summary
        let mut summary_map: HashMap<String, String> = HashMap::new();
        summary_map.insert("spark.app.id".to_string(), "icetable".to_string());
        summary_map.insert(
            "manifests-rewritten".to_string(),
            data_manifests.len().to_string(),
        );
        summary_map.insert(
            "manifests-created".to_string(),
            self.final_manifest_count.to_string(),
        );
        summary_map.insert("total-data-files".to_string(), total_data_files.to_string());
        summary_map.insert("total-records".to_string(), total_rows.to_string());
        summary_map.insert("total-files-size".to_string(), total_files_size.to_string());
        summary_map.insert("added-data-files".to_string(), "0".to_string());
        summary_map.insert("deleted-data-files".to_string(), "0".to_string());
        summary_map.insert("added-records".to_string(), "0".to_string());
        summary_map.insert("deleted-records".to_string(), "0".to_string());

        let summary = Summary {
            operation: Operation::Replace,
            additional_properties: summary_map,
        };

        Snapshot::builder()
            .with_snapshot_id(self.new_snapshot_id)
            .with_parent_snapshot_id(Some(self.parent_snapshot_id))
            .with_sequence_number(self.sequence_number + 1)
            .with_timestamp_ms(self.new_snapshot_id)
            .with_manifest_list(manifest_list_path.to_string())
            .with_summary(summary)
            .with_schema_id(self.schema_id)
            .build()
    }
}

/// Service for rewriting Iceberg manifest files
pub struct ManifestService {
    config: ManifestConfig,
}

impl ManifestService {
    /// Create a new ManifestService with default configuration
    pub fn new() -> Self {
        Self {
            config: ManifestConfig::default(),
        }
    }

    /// Create a new ManifestService with custom configuration
    pub fn with_config(config: ManifestConfig) -> Self {
        Self { config }
    }

    /// Analyze manifests for potential rewrite (dry-run mode)
    ///
    /// The metadata_service should be pre-configured with the appropriate branch
    /// for catalog-aware operations.
    pub async fn analyze(
        &self,
        metadata_service: &IcebergMetadataService,
    ) -> Result<ManifestAnalysis> {
        let (metadata, _) = metadata_service.load_metadata().await?;
        let file_io = metadata_service.file_io().clone();

        let target_branch = self.config.branch.as_deref().unwrap_or("main");

        // Get snapshot for the target branch
        let current_snapshot = if target_branch == "main" {
            metadata.current_snapshot().ok_or_else(|| Error::Manifest {
                message: "No current snapshot found".to_string(),
            })?
        } else {
            metadata
                .snapshot_for_ref(target_branch)
                .ok_or_else(|| Error::Manifest {
                    message: format!("Branch '{}' not found", target_branch),
                })?
        };

        // Load manifest list
        let manifest_list = current_snapshot
            .load_manifest_list(&file_io, &metadata)
            .await
            .map_err(|e| Error::Manifest {
                message: format!("Failed to load manifest list: {}", e),
            })?;

        let manifest_entries = manifest_list.entries();
        let total_manifests = manifest_entries.len();

        // Check minimum threshold
        if total_manifests < self.config.min_manifests {
            return Ok(ManifestAnalysis {
                current_manifests: total_manifests,
                data_manifests: 0,
                delete_manifests: 0,
                total_entries: 0,
                estimated_after: total_manifests,
                should_rewrite: false,
                skip_reason: Some(format!(
                    "Only {} manifests found (minimum: {})",
                    total_manifests, self.config.min_manifests
                )),
            });
        }

        // Group manifests by content type
        let mut data_manifests = Vec::new();
        let mut delete_manifests = Vec::new();

        for entry in manifest_entries {
            match entry.content {
                ManifestContentType::Data => data_manifests.push(entry.clone()),
                ManifestContentType::Deletes => delete_manifests.push(entry.clone()),
            }
        }

        // Count entries
        let mut total_entries = 0usize;
        for manifest_entry in &data_manifests {
            if let Ok(manifest) = manifest_entry.load_manifest(&file_io).await {
                total_entries += manifest
                    .entries()
                    .iter()
                    .filter(|e| e.status() != ManifestStatus::Deleted)
                    .count();
            }
        }

        let entries_per_manifest = (self.config.target_size as usize / 500).max(100);
        let new_manifest_count = (total_entries / entries_per_manifest).max(1);

        Ok(ManifestAnalysis {
            current_manifests: total_manifests,
            data_manifests: data_manifests.len(),
            delete_manifests: delete_manifests.len(),
            total_entries,
            estimated_after: new_manifest_count + delete_manifests.len(),
            should_rewrite: true,
            skip_reason: None,
        })
    }

    /// Rewrite manifests for the given table
    ///
    /// The metadata_service should be pre-configured with the appropriate committer
    /// for catalog-aware operations.
    pub async fn rewrite(
        &self,
        metadata_service: &IcebergMetadataService,
    ) -> Result<ManifestRewriteResult> {
        let (metadata, _) = metadata_service.load_metadata().await?;
        let file_io = metadata_service.file_io().clone();
        let table_path = metadata_service.path();

        let target_branch = self.config.branch.as_deref().unwrap_or("main");

        // Get snapshot for the target branch
        let current_snapshot = if target_branch == "main" {
            metadata.current_snapshot().ok_or_else(|| Error::Manifest {
                message: "No current snapshot found".to_string(),
            })?
        } else {
            metadata
                .snapshot_for_ref(target_branch)
                .ok_or_else(|| Error::Manifest {
                    message: format!("Branch '{}' not found", target_branch),
                })?
        };

        let snapshot_id = current_snapshot.snapshot_id();
        let parent_snapshot_id = current_snapshot.parent_snapshot_id();
        let sequence_number = current_snapshot.sequence_number();

        // Load manifest list
        let manifest_list = current_snapshot
            .load_manifest_list(&file_io, &metadata)
            .await
            .map_err(|e| Error::Manifest {
                message: format!("Failed to load manifest list: {}", e),
            })?;

        let manifest_entries = manifest_list.entries();
        let total_manifests = manifest_entries.len();

        // Group manifests by content type
        let mut data_manifests = Vec::new();
        let mut delete_manifests = Vec::new();

        for entry in manifest_entries {
            match entry.content {
                ManifestContentType::Data => data_manifests.push(entry.clone()),
                ManifestContentType::Deletes => delete_manifests.push(entry.clone()),
            }
        }

        // Read all data file entries
        let mut all_data_entries: Vec<Arc<ManifestEntry>> = Vec::new();

        for manifest_entry in &data_manifests {
            match manifest_entry.load_manifest(&file_io).await {
                Ok(manifest) => {
                    for entry in manifest.entries() {
                        if entry.status() != ManifestStatus::Deleted {
                            all_data_entries.push(entry.clone());
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to load manifest: {}", e);
                }
            }
        }

        let total_entries = all_data_entries.len();
        let entries_per_manifest = (self.config.target_size as usize / 500).max(100);

        // Get schema and partition spec
        let schema = metadata.current_schema().clone();
        let partition_spec = metadata.default_partition_spec().clone();

        // Create output paths
        let base_path = table_path.trim_end_matches('/');
        let metadata_dir = format!("{}/metadata", base_path);
        let new_snapshot_id = chrono::Utc::now().timestamp_millis();

        // Write new manifests
        let mut new_manifest_files = Vec::new();
        let chunks: Vec<_> = all_data_entries.chunks(entries_per_manifest).collect();

        for (idx, chunk) in chunks.iter().enumerate() {
            let manifest_filename = format!(
                "{:x}-m{}.avro",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system time is after UNIX epoch")
                    .as_nanos() as u64
                    ^ (idx as u64),
                idx
            );
            let manifest_path = format!("{}/{}", metadata_dir, manifest_filename);

            let output = file_io
                .new_output(&manifest_path)
                .map_err(|e| Error::Manifest {
                    message: format!("Failed to create manifest output: {}", e),
                })?;

            let mut writer = ManifestWriterBuilder::new(
                output,
                Some(new_snapshot_id),
                None,
                schema.clone(),
                (*partition_spec).clone(),
            )
            .build_v2_data();

            for entry in chunk.iter() {
                writer
                    .add_existing_file(
                        entry.data_file().clone(),
                        entry.snapshot_id().unwrap_or(snapshot_id),
                        entry.sequence_number().unwrap_or(sequence_number),
                        Some(entry.sequence_number().unwrap_or(sequence_number)),
                    )
                    .map_err(|e| Error::Manifest {
                        message: format!("Failed to add entry: {}", e),
                    })?;
            }

            let manifest_file =
                writer
                    .write_manifest_file()
                    .await
                    .map_err(|e| Error::Manifest {
                        message: format!("Failed to write manifest: {}", e),
                    })?;

            new_manifest_files.push(manifest_file);
        }

        // Keep delete manifests as-is
        for delete_manifest in &delete_manifests {
            new_manifest_files.push(delete_manifest.clone());
        }

        let final_manifest_count = new_manifest_files.len();

        // Write new manifest list
        let manifest_list_filename = format!(
            "snap-{}-0-{:x}.avro",
            new_snapshot_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time is after UNIX epoch")
                .as_nanos() as u64
        );
        let manifest_list_path = format!("{}/{}", metadata_dir, manifest_list_filename);

        let manifest_list_output =
            file_io
                .new_output(&manifest_list_path)
                .map_err(|e| Error::Manifest {
                    message: format!("Failed to create manifest list output: {}", e),
                })?;

        let mut manifest_list_writer = ManifestListWriter::v2(
            manifest_list_output,
            new_snapshot_id,
            parent_snapshot_id,
            sequence_number + 1,
        );

        manifest_list_writer
            .add_manifests(new_manifest_files.clone().into_iter())
            .map_err(|e| Error::Manifest {
                message: format!("Failed to add manifests: {}", e),
            })?;
        manifest_list_writer
            .close()
            .await
            .map_err(|e| Error::Manifest {
                message: format!("Failed to close manifest list writer: {}", e),
            })?;

        // Create new snapshot using builder
        let new_snapshot = SnapshotBuilder::new(new_snapshot_id, snapshot_id, sequence_number)
            .manifest_list_path(&manifest_list_path)
            .new_manifest_files(&new_manifest_files)
            .data_manifests(&data_manifests)
            .original_summary(current_snapshot.summary())
            .schema_id(metadata.current_schema_id())
            .build();

        // Commit the snapshot
        let metadata_file_path = metadata_service.current_metadata_path().await?;

        let ctx = CommitContext {
            table_path,
            metadata_dir: &metadata_dir,
            metadata_file_path: &metadata_file_path,
            metadata: &metadata,
            new_snapshot,
            target_branch,
            new_snapshot_id,
            committer: metadata_service.committer(),
        };
        let new_version = self.commit_snapshot(ctx).await?;

        Ok(ManifestRewriteResult {
            previous_manifests: total_manifests,
            new_manifests: final_manifest_count,
            data_manifests_rewritten: data_manifests.len(),
            delete_manifests_kept: delete_manifests.len(),
            total_entries,
            snapshot_id: new_snapshot_id,
            metadata_version: new_version,
        })
    }

    /// Commit the snapshot to storage or catalog
    async fn commit_snapshot(&self, ctx: CommitContext<'_>) -> Result<u32> {
        if let Some(ref c) = ctx.committer
            && c.uses_catalog()
        {
            // Catalog mode: commit via REST API
            return Ok(c
                .commit_add_snapshot(ctx.metadata, ctx.new_snapshot, ctx.target_branch)
                .await?
                .unwrap_or(1) as u32);
        }

        // Direct mode: build metadata and write to storage
        let metadata_clone = (**ctx.metadata).clone();
        let build_result = metadata_clone
            .into_builder(Some(ctx.metadata_file_path.to_string()))
            .add_snapshot(ctx.new_snapshot)
            .map_err(|e| Error::Metadata {
                message: format!("Failed to add snapshot: {}", e),
            })?
            .set_ref(
                ctx.target_branch,
                SnapshotReference {
                    snapshot_id: ctx.new_snapshot_id,
                    retention: SnapshotRetention::Branch {
                        min_snapshots_to_keep: None,
                        max_snapshot_age_ms: None,
                        max_ref_age_ms: None,
                    },
                },
            )
            .map_err(|e| Error::Metadata {
                message: format!("Failed to set ref: {}", e),
            })?
            .build()
            .map_err(|e| Error::Metadata {
                message: format!("Failed to build metadata: {}", e),
            })?;

        let new_metadata = build_result.metadata;
        self.write_metadata_direct(
            ctx.table_path,
            ctx.metadata_dir,
            ctx.metadata_file_path,
            &new_metadata,
        )
        .await
    }

    /// Write metadata directly to storage
    async fn write_metadata_direct(
        &self,
        table_path: &str,
        _metadata_dir: &str,       // Kept for API compatibility
        _metadata_file_path: &str, // Kept for API compatibility
        new_metadata: &TableMetadata,
    ) -> Result<u32> {
        let storage = create_object_store(table_path).await?;
        let result =
            crate::utils::core::write_metadata_file(table_path, new_metadata, &storage).await?;
        Ok(result.version as u32)
    }
}

impl Default for ManifestService {
    fn default() -> Self {
        Self::new()
    }
}

//! Iceberg metadata service implementation
//!
//! Encapsulates all the repetitive logic for writing Iceberg snapshots:
//! - Writing manifests
//! - Writing manifest lists
//! - Creating snapshots
//! - Updating table metadata
//! - Managing version-hint.text

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use iceberg::TableIdent;
use iceberg::io::{FileIO, FileIOBuilder};
use iceberg::spec::{DataFile, ManifestList, ManifestStatus, Summary, TableMetadata};
use iceberg::table::StaticTable;
use object_store::ObjectStore;

use super::traits::{DataFileChanges, DataFileInfo, MetadataService, OperationType, SnapshotInfo};
use crate::core::storage::{ObjectStoreAdapter, StorageBackend, StorageBackendFactory, s3};
use crate::core::utils::{
    extract_version_from_filename, find_latest_metadata, iceberg_to_arrow_type,
};
use crate::error::{Error, Result};

use super::iceberg_operations;
use super::iceberg_partition;
use super::iceberg_writer::IcebergSnapshotWriter;

/// Iceberg metadata service for transactional operations
pub struct IcebergMetadataService {
    table_path: String,
    file_io: FileIO,
    storage: Arc<dyn StorageBackend>,
    /// Target branch for operations (defaults to "main")
    target_branch: Option<String>,
}

impl IcebergMetadataService {
    /// Create a new Iceberg metadata service
    pub async fn new_async(table_path: String) -> Result<Self> {
        let file_io = Self::create_file_io(&table_path)?;
        let storage = StorageBackendFactory::create_backend(&table_path).await?;

        Ok(Self {
            table_path,
            file_io,
            storage,
            target_branch: None,
        })
    }

    /// Create a new Iceberg metadata service targeting a specific branch
    pub async fn new_with_branch(table_path: String, branch: Option<String>) -> Result<Self> {
        let file_io = Self::create_file_io(&table_path)?;
        let storage = StorageBackendFactory::create_backend(&table_path).await?;

        Ok(Self {
            table_path,
            file_io,
            storage,
            target_branch: branch,
        })
    }

    /// Get the target branch name (defaults to "main" if not set)
    pub fn target_branch(&self) -> &str {
        self.target_branch.as_deref().unwrap_or("main")
    }

    /// Create FileIO based on path scheme
    fn create_file_io(path: &str) -> Result<FileIO> {
        if path.starts_with("s3://") || path.starts_with("s3a://") {
            let mut builder = FileIOBuilder::new("s3");

            if let Ok(key) = std::env::var("AWS_ACCESS_KEY_ID") {
                builder = builder.with_prop("s3.access-key-id", key);
            }
            if let Ok(secret) = std::env::var("AWS_SECRET_ACCESS_KEY") {
                builder = builder.with_prop("s3.secret-access-key", secret);
            }
            if let Ok(token) = std::env::var("AWS_SESSION_TOKEN") {
                builder = builder.with_prop("s3.session-token", token);
            }
            if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
                builder = builder.with_prop("s3.endpoint", endpoint);
            }
            if let Ok(region) = std::env::var("AWS_REGION") {
                builder = builder.with_prop("s3.region", region);
            } else if let Ok(region) = std::env::var("AWS_DEFAULT_REGION") {
                builder = builder.with_prop("s3.region", region);
            } else {
                builder = builder.with_prop("s3.region", "us-east-1");
            }
            builder = builder.with_prop("s3.path-style-access", "true");

            builder
                .build()
                .map_err(|e| Error::General(format!("Failed to create S3 FileIO: {}", e)))
        } else if path.starts_with("gs://") || path.starts_with("gcs://") {
            FileIOBuilder::new("gcs")
                .build()
                .map_err(|e| Error::General(format!("Failed to create GCS FileIO: {}", e)))
        } else if path.starts_with("az://")
            || path.starts_with("abfs://")
            || path.starts_with("abfss://")
        {
            FileIOBuilder::new("azblob")
                .build()
                .map_err(|e| Error::General(format!("Failed to create Azure FileIO: {}", e)))
        } else {
            FileIOBuilder::new_fs_io()
                .build()
                .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))
        }
    }

    /// Load current table metadata
    pub async fn load_metadata(&self) -> Result<(Arc<TableMetadata>, i32)> {
        let metadata_file = find_latest_metadata(&self.table_path, &self.storage).await?;
        let version = extract_version_from_filename(&metadata_file);

        let table_ident = TableIdent::from_strs(["iceberg", "table"])
            .map_err(|e| Error::General(format!("Failed to create table ident: {}", e)))?;

        let static_table =
            StaticTable::from_metadata_file(&metadata_file, table_ident, self.file_io.clone())
                .await
                .map_err(|e| Error::General(format!("Failed to load table metadata: {}", e)))?;

        Ok((static_table.metadata(), version))
    }

    /// Get the FileIO for loading manifests
    pub fn file_io(&self) -> &FileIO {
        &self.file_io
    }

    /// Get the table path
    pub fn table_path(&self) -> &str {
        &self.table_path
    }

    /// Get the storage backend
    pub fn storage(&self) -> &Arc<dyn StorageBackend> {
        &self.storage
    }

    /// Get current metadata file path
    pub async fn current_metadata_path(&self) -> Result<String> {
        find_latest_metadata(&self.table_path, &self.storage).await
    }

    /// List all references (branches and tags) from raw metadata JSON
    /// Returns a list of (name, snapshot_id, ref_type) tuples
    pub async fn list_refs(&self) -> Result<Vec<RefInfo>> {
        use crate::core::storage::GetOptions;
        let metadata_path = self.current_metadata_path().await?;
        let content = self.storage.get(&metadata_path, &GetOptions::default()).await?;
        let json: serde_json::Value = serde_json::from_slice(&content)
            .map_err(|e| Error::General(format!("Failed to parse metadata JSON: {}", e)))?;

        let mut refs = Vec::new();

        // Parse refs from JSON
        if let Some(refs_obj) = json.get("refs").and_then(|v| v.as_object()) {
            for (name, ref_value) in refs_obj {
                let snapshot_id = ref_value
                    .get("snapshot-id")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                let ref_type = ref_value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                refs.push(RefInfo {
                    name: name.clone(),
                    snapshot_id,
                    ref_type,
                });
            }
        }

        // Always include "main" pointing to current snapshot if not already present
        if !refs.iter().any(|r| r.name == "main") {
            if let Some(current_id) = json.get("current-snapshot-id").and_then(|v| v.as_i64()) {
                refs.push(RefInfo {
                    name: "main".to_string(),
                    snapshot_id: current_id,
                    ref_type: "branch".to_string(),
                });
            }
        }

        Ok(refs)
    }

    /// Get snapshot ID for a branch name
    /// Returns the snapshot ID that the branch points to, or error if branch doesn't exist
    pub async fn get_branch_snapshot_id(&self, branch_name: &str) -> Result<i64> {
        let refs = self.list_refs().await?;

        refs.iter()
            .find(|r| r.name == branch_name && r.ref_type == "branch")
            .map(|r| r.snapshot_id)
            .ok_or_else(|| Error::General(format!("Branch '{}' not found", branch_name)))
    }

    /// Get the snapshot ID for a branch name, or current snapshot ID if branch is None
    pub async fn resolve_branch_snapshot_id(&self, branch: Option<&str>) -> Result<i64> {
        if let Some(branch_name) = branch {
            self.get_branch_snapshot_id(branch_name).await
        } else {
            let (metadata, _) = self.load_metadata().await?;
            metadata.current_snapshot_id()
                .ok_or_else(|| Error::General("No current snapshot".to_string()))
        }
    }

    /// List data files for a specific snapshot (by ID)
    ///
    /// This is the branch-aware version of list_data_files
    pub async fn list_data_files_for_snapshot(&self, snapshot_id: i64) -> Result<Vec<DataFileInfo>> {
        use std::collections::HashSet;

        let (metadata, _) = self.load_metadata().await?;

        // Find the specific snapshot
        let snapshot = metadata.snapshots()
            .find(|s| s.snapshot_id() == snapshot_id)
            .ok_or_else(|| Error::General(format!("Snapshot {} not found", snapshot_id)))?;

        // Read manifest list
        let manifest_list_path = snapshot.manifest_list();
        let manifest_list_content = self
            .file_io
            .new_input(manifest_list_path)
            .map_err(|e| Error::General(format!("Failed to open manifest list: {}", e)))?
            .read()
            .await
            .map_err(|e| Error::General(format!("Failed to read manifest list: {}", e)))?;

        let manifest_list =
            ManifestList::parse_with_version(&manifest_list_content, metadata.format_version())
                .map_err(|e| Error::General(format!("Failed to parse manifest list: {}", e)))?;

        // Read all manifests and collect data files
        let mut seen_paths: HashSet<String> = HashSet::new();
        let mut deleted_paths: HashSet<String> = HashSet::new();
        let mut data_files = Vec::new();

        // First pass: collect all deleted paths
        for manifest_file_entry in manifest_list.entries() {
            if manifest_file_entry.content != iceberg::spec::ManifestContentType::Data {
                continue;
            }

            let manifest = manifest_file_entry
                .load_manifest(&self.file_io)
                .await
                .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

            for entry in manifest.entries() {
                if entry.status() == ManifestStatus::Deleted {
                    deleted_paths.insert(entry.data_file().file_path().to_string());
                }
            }
        }

        // Second pass: collect alive files
        for manifest_file_entry in manifest_list.entries() {
            if manifest_file_entry.content != iceberg::spec::ManifestContentType::Data {
                continue;
            }

            let manifest = manifest_file_entry
                .load_manifest(&self.file_io)
                .await
                .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

            for entry in manifest.entries() {
                if entry.status() == ManifestStatus::Deleted {
                    continue;
                }

                let data_file = entry.data_file();
                let path = data_file.file_path().to_string();

                if deleted_paths.contains(&path) || seen_paths.contains(&path) {
                    continue;
                }
                seen_paths.insert(path.clone());

                data_files.push(DataFileInfo {
                    path,
                    size: data_file.file_size_in_bytes(),
                    record_count: data_file.record_count(),
                    partition: iceberg_partition::extract_partition_from_path_static(
                        data_file.file_path(),
                    ),
                });
            }
        }

        Ok(data_files)
    }

    /// List data files for a branch (or current if None)
    pub async fn list_data_files_for_branch(&self, branch: Option<&str>) -> Result<Vec<DataFileInfo>> {
        let snapshot_id = self.resolve_branch_snapshot_id(branch).await?;
        self.list_data_files_for_snapshot(snapshot_id).await
    }

    /// Get the branch name to use (defaults to "main" if None)
    pub fn branch_name_or_default(branch: Option<&str>) -> &str {
        branch.unwrap_or("main")
    }
}

/// Information about a reference (branch or tag)
#[derive(Debug, Clone)]
pub struct RefInfo {
    /// Name of the reference
    pub name: String,
    /// Snapshot ID the reference points to
    pub snapshot_id: i64,
    /// Type of reference: "branch" or "tag"
    pub ref_type: String,
}

#[async_trait]
impl MetadataService for IcebergMetadataService {
    async fn current_snapshot(&self) -> Result<Option<SnapshotInfo>> {
        let (metadata, _) = self.load_metadata().await?;

        Ok(metadata.current_snapshot().map(|s| SnapshotInfo {
            id: s.snapshot_id(),
            timestamp_ms: s.timestamp_ms(),
            operation: format!("{:?}", s.summary().operation),
            summary: s.summary().additional_properties.clone(),
            parent_id: s.parent_snapshot_id(),
        }))
    }

    async fn list_data_files(&self) -> Result<Vec<DataFileInfo>> {
        use std::collections::HashSet;

        let (metadata, _) = self.load_metadata().await?;

        // Use the target branch snapshot, or current snapshot if not set/not found
        let current_snapshot = if let Some(ref branch) = self.target_branch {
            match metadata.snapshot_for_ref(branch) {
                Some(s) => s,
                None => match metadata.current_snapshot() {
                    Some(s) => s,
                    None => return Ok(Vec::new()),
                },
            }
        } else {
            match metadata.current_snapshot() {
                Some(s) => s,
                None => return Ok(Vec::new()),
            }
        };

        // Read manifest list
        let manifest_list_path = current_snapshot.manifest_list();
        let manifest_list_content = self
            .file_io
            .new_input(manifest_list_path)
            .map_err(|e| Error::General(format!("Failed to open manifest list: {}", e)))?
            .read()
            .await
            .map_err(|e| Error::General(format!("Failed to read manifest list: {}", e)))?;

        let manifest_list =
            ManifestList::parse_with_version(&manifest_list_content, metadata.format_version())
                .map_err(|e| Error::General(format!("Failed to parse manifest list: {}", e)))?;

        // Read all manifests and collect data files
        // Track deleted paths separately to handle cross-manifest deletions
        let mut seen_paths: HashSet<String> = HashSet::new();
        let mut deleted_paths: HashSet<String> = HashSet::new();
        let mut data_files = Vec::new();

        // Debug counters (prefixed with _ as they're for debugging)
        let mut _total_entries = 0usize;
        let mut _added_count = 0usize;
        let mut _existing_count = 0usize;
        let mut _deleted_count = 0usize;

        // First pass: collect all deleted paths from all manifests
        for manifest_file_entry in manifest_list.entries() {
            if manifest_file_entry.content != iceberg::spec::ManifestContentType::Data {
                continue;
            }

            let manifest = manifest_file_entry
                .load_manifest(&self.file_io)
                .await
                .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

            for entry in manifest.entries() {
                if entry.status() == ManifestStatus::Deleted {
                    deleted_paths.insert(entry.data_file().file_path().to_string());
                }
            }
        }

        // Second pass: collect alive files that are not in deleted set
        for manifest_file_entry in manifest_list.entries() {
            if manifest_file_entry.content != iceberg::spec::ManifestContentType::Data {
                continue;
            }

            let manifest = manifest_file_entry
                .load_manifest(&self.file_io)
                .await
                .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

            for entry in manifest.entries() {
                _total_entries += 1;
                match entry.status() {
                    ManifestStatus::Added => _added_count += 1,
                    ManifestStatus::Existing => _existing_count += 1,
                    ManifestStatus::Deleted => {
                        _deleted_count += 1;
                        continue;
                    }
                }

                let data_file = entry.data_file();
                let path = data_file.file_path().to_string();

                // Skip if this file was deleted in any manifest
                if deleted_paths.contains(&path) {
                    continue;
                }

                // Deduplicate by full path
                if seen_paths.contains(&path) {
                    continue;
                }
                seen_paths.insert(path.clone());

                data_files.push(DataFileInfo {
                    path,
                    size: data_file.file_size_in_bytes(),
                    record_count: data_file.record_count(),
                    partition: iceberg_partition::extract_partition_from_path_static(
                        data_file.file_path(),
                    ),
                });
            }
        }

        Ok(data_files)
    }

    async fn list_snapshots(&self, limit: Option<usize>) -> Result<Vec<SnapshotInfo>> {
        let (metadata, _) = self.load_metadata().await?;

        let mut snapshots: Vec<SnapshotInfo> = metadata
            .snapshots()
            .map(|s| SnapshotInfo {
                id: s.snapshot_id(),
                timestamp_ms: s.timestamp_ms(),
                operation: format!("{:?}", s.summary().operation),
                summary: s.summary().additional_properties.clone(),
                parent_id: s.parent_snapshot_id(),
            })
            .collect();

        // Sort by timestamp descending (newest first)
        snapshots.sort_by_key(|s| -s.timestamp_ms);

        if let Some(n) = limit {
            snapshots.truncate(n);
        }

        Ok(snapshots)
    }

    async fn write_snapshot(
        &self,
        changes: DataFileChanges,
        operation: OperationType,
        summary: HashMap<String, String>,
    ) -> Result<SnapshotInfo> {
        // Load current metadata
        let (metadata, current_version) = self.load_metadata().await?;
        let partition_spec = metadata.default_partition_spec();
        let schema_id = metadata.current_schema().schema_id();
        // Create snapshot writer
        let writer = IcebergSnapshotWriter::new(
            self.table_path.clone(),
            self.file_io.clone(),
            self.storage.clone(),
        );

        // Get current state for the target branch
        let target_branch = self.target_branch();
        let current_snapshot = if target_branch == "main" {
            metadata.current_snapshot()
        } else {
            metadata.snapshot_for_ref(target_branch)
        };
        let parent_snapshot_id = current_snapshot.map(|s| s.snapshot_id());
        // Use last_sequence_number from metadata (tracks max ever assigned, not just current snapshot)
        // This handles cases where snapshots were expired
        let sequence_number = metadata.last_sequence_number() + 1;

        // Generate IDs
        let snapshot_id = chrono::Utc::now().timestamp_millis();
        let timestamp_nanos = crate::core::utils::generate_unique_id();

        // Get existing files (if replacing/repairing, we need to include unchanged files)
        let mut all_files: Vec<DataFile> = Vec::new();

        // For Replace/Repair operations, start with existing files minus removed ones
        if matches!(operation, OperationType::Replace | OperationType::Repair) {
            let existing_files = self.list_data_files().await?;
            let removed_paths: std::collections::HashSet<_> =
                changes.removed.iter().map(|f| &f.path).collect();

            for file in existing_files {
                if !removed_paths.contains(&file.path) {
                    all_files.push(writer.to_iceberg_data_file(&file, partition_spec)?);
                }
            }
        }

        // Add new files
        for file_info in &changes.added {
            all_files.push(writer.to_iceberg_data_file(file_info, partition_spec)?);
        }

        // Write manifest
        let manifest_file = writer
            .write_manifest(
                &all_files,
                snapshot_id,
                sequence_number,
                &metadata,
                timestamp_nanos,
            )
            .await?;

        // Write manifest list
        let manifest_list_path = writer
            .write_manifest_list(
                manifest_file,
                snapshot_id,
                parent_snapshot_id,
                sequence_number,
                timestamp_nanos,
            )
            .await?;

        // Build summary with all standard Iceberg fields
        let total_records: u64 = all_files.iter().map(|f| f.record_count()).sum();
        let total_files_size: u64 = all_files.iter().map(|f| f.file_size_in_bytes()).sum();
        let mut full_summary = summary.clone();
        full_summary.insert("total-records".to_string(), total_records.to_string());
        full_summary.insert("total-data-files".to_string(), all_files.len().to_string());
        full_summary.insert("total-files-size".to_string(), total_files_size.to_string());

        // Add change metrics
        let added_files = changes.added.len();
        let removed_files = changes.removed.len();
        let added_size: u64 = changes.added.iter().map(|f| f.size).sum();
        let removed_size: u64 = changes.removed.iter().map(|f| f.size).sum();
        let added_records: u64 = changes.added.iter().map(|f| f.record_count).sum();
        let removed_records: u64 = changes.removed.iter().map(|f| f.record_count).sum();

        if added_files > 0 {
            full_summary.insert("added-data-files".to_string(), added_files.to_string());
            full_summary.insert("added-files-size".to_string(), added_size.to_string());
            full_summary.insert("added-records".to_string(), added_records.to_string());
        }
        if removed_files > 0 {
            full_summary.insert("deleted-data-files".to_string(), removed_files.to_string());
            full_summary.insert("removed-files-size".to_string(), removed_size.to_string());
            full_summary.insert("deleted-records".to_string(), removed_records.to_string());
        }

        let iceberg_summary = Summary {
            operation: iceberg_operations::to_iceberg_operation(operation),
            additional_properties: full_summary.clone(),
        };

        // Build snapshot
        let snapshot = writer.build_snapshot(
            snapshot_id,
            parent_snapshot_id,
            sequence_number,
            manifest_list_path,
            iceberg_summary,
            schema_id,
        );

        // Update metadata for the target branch
        let new_metadata =
            writer.update_metadata_for_branch((*metadata).clone(), snapshot, current_version, target_branch)?;

        // Write new metadata file
        let new_version = current_version + 1;
        writer
            .write_metadata_file(&new_metadata, new_version)
            .await?;

        // Update version hint
        writer.update_version_hint(new_version).await?;

        Ok(SnapshotInfo {
            id: snapshot_id,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            operation: operation.to_string(),
            summary: full_summary,
            parent_id: parent_snapshot_id,
        })
    }

    fn data_directory(&self) -> PathBuf {
        PathBuf::from(format!("{}/data", self.table_path.trim_end_matches('/')))
    }

    async fn scan_data_files_on_storage(&self) -> Result<Vec<DataFileInfo>> {
        use crate::core::storage::traits::ListOptions;
        use indicatif::{ProgressBar, ProgressStyle};

        let data_prefix = format!("{}/data/", self.table_path.trim_end_matches('/'));

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("  {spinner:.cyan} Listing files on storage...")
                .unwrap(),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        // Don't set max_results - let object_store handle pagination internally
        let list_opts = ListOptions {
            prefix: Some(data_prefix),
            delimiter: None,
            max_results: None,
            continuation_token: None,
        };

        let result = self.storage.list(&list_opts).await?;

        pb.finish_and_clear();

        let all_files: Vec<DataFileInfo> = result
            .objects
            .iter()
            .filter(|obj| obj.path.ends_with(".parquet"))
            .map(|obj| {
                let partition = iceberg_partition::extract_partition_from_path_static(&obj.path);
                DataFileInfo {
                    path: obj.path.clone(),
                    size: obj.size,
                    record_count: 0,
                    partition,
                }
            })
            .collect();

        Ok(all_files)
    }

    async fn get_all_referenced_files(&self) -> Result<std::collections::HashSet<String>> {
        use indicatif::{ProgressBar, ProgressStyle};
        use std::collections::HashSet;

        let (metadata, _) = self.load_metadata().await?;

        // Collect unique manifest entries from ALL snapshots (deduplicated by path)
        let mut seen_manifest_paths: HashSet<String> = HashSet::new();
        let mut manifest_entries: Vec<iceberg::spec::ManifestFile> = Vec::new();

        for snapshot in metadata.snapshots() {
            let manifest_list_path = snapshot.manifest_list();

            let manifest_list_content = match self
                .file_io
                .new_input(manifest_list_path)
                .map_err(|e| Error::General(format!("Failed to open manifest list: {}", e)))?
                .read()
                .await
            {
                Ok(content) => content,
                Err(_) => continue,
            };

            let manifest_list = match ManifestList::parse_with_version(
                &manifest_list_content,
                metadata.format_version(),
            ) {
                Ok(ml) => ml,
                Err(_) => continue,
            };

            for entry in manifest_list.entries() {
                if entry.content == iceberg::spec::ManifestContentType::Data
                    && !seen_manifest_paths.contains(&entry.manifest_path)
                {
                    seen_manifest_paths.insert(entry.manifest_path.clone());
                    manifest_entries.push(entry.clone());
                }
            }
        }

        let total_manifests = manifest_entries.len();

        let pb = ProgressBar::new(total_manifests as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  {spinner:.cyan} Scanning manifests {bar:30.dim.white/dim} {pos}/{len}")
                .unwrap()
                .progress_chars("━━╺"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        // Process manifests sequentially (load_manifest requires &self.file_io)
        // but we only process each unique manifest once
        let mut all_alive: HashSet<String> = HashSet::new();

        for manifest_entry in manifest_entries {
            let manifest = match manifest_entry.load_manifest(&self.file_io).await {
                Ok(m) => m,
                Err(_) => {
                    pb.inc(1);
                    continue;
                }
            };

            for entry in manifest.entries() {
                // For orphan detection: if a file appears as Added/Existing in ANY manifest,
                // it's referenced and not an orphan
                if entry.status() != ManifestStatus::Deleted {
                    all_alive.insert(entry.data_file().file_path().to_string());
                }
            }
            pb.inc(1);
        }

        pb.finish_and_clear();

        Ok(all_alive)
    }

    async fn schema(&self) -> Result<Arc<arrow::datatypes::Schema>> {
        let (metadata, _) = self.load_metadata().await?;
        let iceberg_schema = metadata.current_schema();

        // Convert Iceberg schema to Arrow schema
        let fields: Vec<arrow::datatypes::Field> = iceberg_schema
            .as_struct()
            .fields()
            .iter()
            .map(|field| {
                let arrow_type = iceberg_to_arrow_type(&field.field_type);
                arrow::datatypes::Field::new(&field.name, arrow_type, !field.required)
            })
            .collect();

        Ok(Arc::new(arrow::datatypes::Schema::new(fields)))
    }

    fn object_store(&self) -> Arc<dyn ObjectStore> {
        // For S3/GCS/Azure, use native object_store to get multipart upload support
        if self.table_path.starts_with("s3://") {
            s3::create_s3_object_store(&self.table_path).unwrap_or_else(|_| {
                // Fallback to adapter if native creation fails
                Arc::new(ObjectStoreAdapter::new(
                    self.storage.clone(),
                    self.table_path.clone(),
                ))
            })
        } else {
            // For local filesystem, use our adapter
            Arc::new(ObjectStoreAdapter::new(
                self.storage.clone(),
                self.table_path.clone(),
            ))
        }
    }
}

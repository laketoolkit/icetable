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
use iceberg::spec::{
    DataContentType, DataFile, DataFileBuilder, DataFileFormat, FormatVersion, MAIN_BRANCH,
    ManifestList, ManifestListWriter, ManifestStatus, ManifestWriterBuilder, Snapshot, Struct,
    Summary, TableMetadata, TableMetadataBuilder,
};
use iceberg::table::StaticTable;

use super::traits::{DataFileChanges, DataFileInfo, MetadataService, OperationType, SnapshotInfo};
use crate::core::storage::{StorageBackend, StorageBackendFactory};
use crate::error::{Error, Result};

/// Iceberg metadata service for transactional operations
pub struct IcebergMetadataService {
    table_path: String,
    file_io: FileIO,
    storage: Arc<dyn StorageBackend>,
}

impl IcebergMetadataService {
    /// Create a new Iceberg metadata service
    pub fn new(table_path: String) -> Result<Self> {
        // This is a sync constructor, so we create a minimal version
        // The actual initialization happens in new_async
        let file_io = FileIOBuilder::new_fs_io()
            .build()
            .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))?;

        // Create a dummy storage for now - will be replaced in new_async
        let storage: Arc<dyn StorageBackend> = Arc::new(DummyStorage);

        Ok(Self {
            table_path,
            file_io,
            storage,
        })
    }

    /// Create a new Iceberg metadata service with async initialization
    pub async fn new_async(table_path: String) -> Result<Self> {
        let file_io = Self::create_file_io(&table_path)?;
        let storage = StorageBackendFactory::create_backend(&table_path).await?;

        Ok(Self {
            table_path,
            file_io,
            storage,
        })
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

    /// Get the metadata directory path
    fn metadata_dir(&self) -> String {
        format!("{}/metadata", self.table_path.trim_end_matches('/'))
    }

    /// Find the latest metadata file
    async fn find_latest_metadata(&self) -> Result<String> {
        use crate::core::storage::traits::{GetOptions, ListOptions};

        let metadata_dir = self.metadata_dir();

        // First try to read version-hint.text for authoritative version
        // This avoids S3 eventual consistency issues with list operations
        let version_hint_path = format!("{}/version-hint.text", metadata_dir);
        let get_opts = GetOptions::default();

        if let Ok(version_bytes) = self.storage.get(&version_hint_path, &get_opts).await {
            if let Ok(version_str) = String::from_utf8(version_bytes.to_vec()) {
                if let Ok(version) = version_str.trim().parse::<i32>() {
                    let metadata_path = format!("{}/v{}.metadata.json", metadata_dir, version);
                    // Verify file exists by trying to read it
                    if self.storage.get(&metadata_path, &get_opts).await.is_ok() {
                        return Ok(metadata_path);
                    }
                }
            }
        }

        // Fallback to listing if version-hint doesn't exist or is invalid
        let list_opts = ListOptions {
            prefix: Some(format!("{}/", metadata_dir)),
            delimiter: None,
            max_results: Some(500),
            continuation_token: None,
        };

        let files = self.storage.list(&list_opts).await?;

        // Find the latest metadata.json file by version number
        let metadata_file = files
            .objects
            .iter()
            .filter(|obj| obj.path.contains(".metadata.json"))
            .max_by_key(|obj| {
                let name = obj.path.rsplit('/').next().unwrap_or("");
                if name.starts_with('v') {
                    name.trim_start_matches('v')
                        .split('.')
                        .next()
                        .and_then(|n| n.parse::<i64>().ok())
                        .unwrap_or(0)
                } else {
                    name.split('-')
                        .next()
                        .and_then(|n| n.parse::<i64>().ok())
                        .unwrap_or(0)
                }
            })
            .ok_or_else(|| Error::General("No metadata.json file found".to_string()))?;

        Ok(metadata_file.path.clone())
    }

    /// Load current table metadata
    pub async fn load_metadata(&self) -> Result<(Arc<TableMetadata>, i32)> {
        let metadata_file = self.find_latest_metadata().await?;

        // Extract version from filename
        let filename = metadata_file.rsplit('/').next().unwrap_or("");
        let version = if filename.starts_with('v') {
            filename
                .trim_start_matches('v')
                .split('.')
                .next()
                .and_then(|n| n.parse::<i32>().ok())
                .unwrap_or(1)
        } else {
            filename
                .split('-')
                .next()
                .and_then(|n| n.parse::<i32>().ok())
                .unwrap_or(1)
        };

        let table_ident = TableIdent::from_strs(&["iceberg", "table"])
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

    /// Get current metadata file path
    pub async fn current_metadata_path(&self) -> Result<String> {
        self.find_latest_metadata().await
    }
}

/// Dummy storage for sync constructor (will be replaced in async init)
struct DummyStorage;

#[async_trait]
impl StorageBackend for DummyStorage {
    fn storage_type(&self) -> &str {
        "dummy"
    }

    async fn exists(&self, _path: &str) -> Result<bool> {
        Ok(false)
    }

    async fn head(&self, path: &str) -> Result<crate::core::storage::traits::ObjectMetadata> {
        Err(Error::General(format!(
            "DummyStorage: cannot head {}",
            path
        )))
    }

    async fn get(
        &self,
        path: &str,
        _options: &crate::core::storage::traits::GetOptions,
    ) -> Result<bytes::Bytes> {
        Err(Error::General(format!("DummyStorage: cannot get {}", path)))
    }

    async fn put(
        &self,
        _path: &str,
        _data: bytes::Bytes,
        _options: &crate::core::storage::traits::PutOptions,
    ) -> Result<()> {
        Ok(())
    }

    async fn list(
        &self,
        _options: &crate::core::storage::traits::ListOptions,
    ) -> Result<crate::core::storage::traits::ListResult> {
        Ok(crate::core::storage::traits::ListResult {
            objects: vec![],
            prefixes: vec![],
            continuation_token: None,
        })
    }

    async fn delete(&self, _path: &str) -> Result<()> {
        Ok(())
    }

    async fn copy(&self, _from: &str, _to: &str) -> Result<()> {
        Ok(())
    }
}

impl IcebergMetadataService {
    /// Write a manifest file containing the given data files
    async fn write_manifest(
        &self,
        data_files: &[DataFile],
        snapshot_id: i64,
        sequence_number: i64,
        metadata: &TableMetadata,
        timestamp_nanos: u128,
    ) -> Result<iceberg::spec::ManifestFile> {
        let iceberg_schema = metadata.current_schema();
        let partition_spec = metadata.default_partition_spec();

        let manifest_filename = format!("{:016x}-m0.avro", timestamp_nanos);
        let manifest_path = format!("{}/{}", self.metadata_dir(), manifest_filename);

        let output_file = self
            .file_io
            .new_output(&manifest_path)
            .map_err(|e| Error::General(format!("Failed to create manifest output: {}", e)))?;

        let mut manifest_writer = ManifestWriterBuilder::new(
            output_file,
            Some(snapshot_id),
            None,
            iceberg_schema.clone(),
            (**partition_spec).clone(),
        )
        .build_v2_data();

        for data_file in data_files {
            manifest_writer
                .add_file(data_file.clone(), sequence_number)
                .map_err(|e| Error::General(format!("Failed to add file to manifest: {}", e)))?;
        }

        manifest_writer
            .write_manifest_file()
            .await
            .map_err(|e| Error::General(format!("Failed to write manifest: {}", e)))
    }

    /// Write a manifest list containing the given manifest files
    async fn write_manifest_list(
        &self,
        manifest_file: iceberg::spec::ManifestFile,
        snapshot_id: i64,
        parent_snapshot_id: Option<i64>,
        sequence_number: i64,
        timestamp_nanos: u128,
    ) -> Result<String> {
        let manifest_list_filename =
            format!("snap-{}-0-{:016x}.avro", snapshot_id, timestamp_nanos);
        let manifest_list_path = format!("{}/{}", self.metadata_dir(), manifest_list_filename);

        let manifest_list_output = self
            .file_io
            .new_output(&manifest_list_path)
            .map_err(|e| Error::General(format!("Failed to create manifest list output: {}", e)))?;

        let mut manifest_list_writer = ManifestListWriter::v2(
            manifest_list_output,
            snapshot_id,
            parent_snapshot_id,
            sequence_number,
        );

        manifest_list_writer
            .add_manifests(vec![manifest_file].into_iter())
            .map_err(|e| Error::General(format!("Failed to add manifest to list: {}", e)))?;

        manifest_list_writer
            .close()
            .await
            .map_err(|e| Error::General(format!("Failed to close manifest list: {}", e)))?;

        Ok(manifest_list_path)
    }

    /// Build a snapshot object
    fn build_snapshot(
        &self,
        snapshot_id: i64,
        parent_snapshot_id: Option<i64>,
        sequence_number: i64,
        manifest_list_path: String,
        summary: Summary,
        schema_id: i32,
    ) -> Snapshot {
        let timestamp_ms = chrono::Utc::now().timestamp_millis();

        Snapshot::builder()
            .with_snapshot_id(snapshot_id)
            .with_parent_snapshot_id(parent_snapshot_id)
            .with_sequence_number(sequence_number)
            .with_timestamp_ms(timestamp_ms)
            .with_manifest_list(manifest_list_path)
            .with_summary(summary)
            .with_schema_id(schema_id)
            .build()
    }

    /// Update table metadata with new snapshot
    fn update_metadata(
        &self,
        old_metadata: TableMetadata,
        snapshot: Snapshot,
        current_version: i32,
    ) -> Result<TableMetadata> {
        let metadata_log_path = format!("v{}.metadata.json", current_version);

        let build_result =
            TableMetadataBuilder::new_from_metadata(old_metadata, Some(metadata_log_path))
                .set_branch_snapshot(snapshot, MAIN_BRANCH)
                .map_err(|e| Error::General(format!("Failed to set snapshot: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        Ok(build_result.metadata)
    }

    /// Write metadata to file
    async fn write_metadata_file(&self, metadata: &TableMetadata, version: i32) -> Result<()> {
        use crate::core::storage::traits::PutOptions;

        let metadata_path = format!("{}/v{}.metadata.json", self.metadata_dir(), version);

        let metadata_json = serde_json::to_string_pretty(metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        let put_opts = PutOptions {
            content_type: Some("application/json".to_string()),
            metadata: std::collections::HashMap::new(),
            if_none_match: None,
        };

        self.storage
            .put(&metadata_path, bytes::Bytes::from(metadata_json), &put_opts)
            .await
            .map_err(|e| Error::General(format!("Failed to write metadata file: {}", e)))?;

        Ok(())
    }

    /// Update version-hint.text
    async fn update_version_hint(&self, version: i32) -> Result<()> {
        use crate::core::storage::traits::PutOptions;

        let version_hint_path = format!("{}/version-hint.text", self.metadata_dir());
        let put_opts = PutOptions {
            content_type: Some("text/plain".to_string()),
            metadata: std::collections::HashMap::new(),
            if_none_match: None,
        };

        self.storage
            .put(
                &version_hint_path,
                bytes::Bytes::from(version.to_string()),
                &put_opts,
            )
            .await
            .map_err(|e| Error::General(format!("Failed to update version hint: {}", e)))?;

        Ok(())
    }

    /// Convert DataFileInfo to Iceberg DataFile
    fn to_iceberg_data_file(
        &self,
        info: &DataFileInfo,
        partition_spec_id: i32,
    ) -> Result<DataFile> {
        DataFileBuilder::default()
            .content(DataContentType::Data)
            .file_path(info.path.clone())
            .file_format(DataFileFormat::Parquet)
            .partition(Struct::empty())
            .partition_spec_id(partition_spec_id)
            .record_count(info.record_count)
            .file_size_in_bytes(info.size)
            .build()
            .map_err(|e| Error::General(format!("Failed to build DataFile: {}", e)))
    }

    /// Convert Iceberg DataFile to DataFileInfo
    fn from_iceberg_data_file(data_file: &DataFile) -> DataFileInfo {
        // Extract partition values from the data file
        let partition = Self::extract_partition_values(data_file);

        DataFileInfo {
            path: data_file.file_path().to_string(),
            size: data_file.file_size_in_bytes() as u64,
            record_count: data_file.record_count(),
            partition,
        }
    }

    /// Extract partition values from an Iceberg DataFile
    fn extract_partition_values(data_file: &DataFile) -> HashMap<String, String> {
        // The partition struct contains the partition field values
        // For now, we extract what we can from the file path as a fallback
        Self::extract_partition_from_path_static(data_file.file_path())
    }

    /// Extract partition information from a file path string
    fn extract_partition_from_path_static(path: &str) -> HashMap<String, String> {
        let mut partition = HashMap::new();

        // Parse partition values from the file path (e.g., "year=2024/month=01/file.parquet")
        for component in path.split('/') {
            if let Some(eq_pos) = component.find('=') {
                let key = component[..eq_pos].to_string();
                let value = component[eq_pos + 1..].to_string();
                partition.insert(key, value);
            }
        }

        partition
    }

    /// Convert OperationType to Iceberg Operation
    fn to_iceberg_operation(op: OperationType) -> iceberg::spec::Operation {
        match op {
            OperationType::Append => iceberg::spec::Operation::Append,
            OperationType::Replace => iceberg::spec::Operation::Replace,
            OperationType::Delete => iceberg::spec::Operation::Delete,
            OperationType::Overwrite => iceberg::spec::Operation::Overwrite,
            OperationType::Restore => iceberg::spec::Operation::Replace,
            OperationType::Repair => iceberg::spec::Operation::Replace,
        }
    }
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

        let current_snapshot = match metadata.current_snapshot() {
            Some(s) => s,
            None => return Ok(Vec::new()),
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

        // Debug counters
        let mut total_entries = 0usize;
        let mut added_count = 0usize;
        let mut existing_count = 0usize;
        let mut deleted_count = 0usize;

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
                total_entries += 1;
                match entry.status() {
                    ManifestStatus::Added => added_count += 1,
                    ManifestStatus::Existing => existing_count += 1,
                    ManifestStatus::Deleted => {
                        deleted_count += 1;
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
                    size: data_file.file_size_in_bytes() as u64,
                    record_count: data_file.record_count(),
                    partition: Self::extract_partition_from_path_static(data_file.file_path()),
                });
            }
        }

        eprintln!(
            "[DEBUG list_data_files] total_entries={}, added={}, existing={}, deleted={}, deleted_paths={}, final_count={}",
            total_entries,
            added_count,
            existing_count,
            deleted_count,
            deleted_paths.len(),
            data_files.len()
        );

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

        // Get current state
        let current_snapshot = metadata.current_snapshot();
        let parent_snapshot_id = current_snapshot.map(|s| s.snapshot_id());
        let sequence_number = current_snapshot
            .map(|s| s.sequence_number() + 1)
            .unwrap_or(1);

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
                    all_files.push(self.to_iceberg_data_file(&file, partition_spec.spec_id())?);
                }
            }
        }

        // Add new files
        for file_info in &changes.added {
            all_files.push(self.to_iceberg_data_file(file_info, partition_spec.spec_id())?);
        }

        // Write manifest
        let manifest_file = self
            .write_manifest(
                &all_files,
                snapshot_id,
                sequence_number,
                &metadata,
                timestamp_nanos,
            )
            .await?;

        // Write manifest list
        let manifest_list_path = self
            .write_manifest_list(
                manifest_file,
                snapshot_id,
                parent_snapshot_id,
                sequence_number,
                timestamp_nanos,
            )
            .await?;

        // Build summary
        let total_records: u64 = all_files.iter().map(|f| f.record_count()).sum();
        let mut full_summary = summary.clone();
        full_summary.insert("total-records".to_string(), total_records.to_string());
        full_summary.insert("total-data-files".to_string(), all_files.len().to_string());

        let iceberg_summary = Summary {
            operation: Self::to_iceberg_operation(operation),
            additional_properties: full_summary.clone(),
        };

        // Build snapshot
        let snapshot = self.build_snapshot(
            snapshot_id,
            parent_snapshot_id,
            sequence_number,
            manifest_list_path,
            iceberg_summary,
            schema_id,
        );

        // Update metadata
        let new_metadata = self.update_metadata((*metadata).clone(), snapshot, current_version)?;

        // Write new metadata file
        let new_version = current_version + 1;
        self.write_metadata_file(&new_metadata, new_version).await?;

        // Update version hint
        self.update_version_hint(new_version).await?;

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
                let partition = Self::extract_partition_from_path_static(&obj.path);
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
                if entry.content == iceberg::spec::ManifestContentType::Data {
                    if !seen_manifest_paths.contains(&entry.manifest_path) {
                        seen_manifest_paths.insert(entry.manifest_path.clone());
                        manifest_entries.push(entry.clone());
                    }
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
}

/// Convert Iceberg type to Arrow type (simplified)
fn iceberg_to_arrow_type(iceberg_type: &iceberg::spec::Type) -> arrow::datatypes::DataType {
    use arrow::datatypes::DataType;
    use iceberg::spec::{PrimitiveType, Type};

    match iceberg_type {
        Type::Primitive(p) => match p {
            PrimitiveType::Boolean => DataType::Boolean,
            PrimitiveType::Int => DataType::Int32,
            PrimitiveType::Long => DataType::Int64,
            PrimitiveType::Float => DataType::Float32,
            PrimitiveType::Double => DataType::Float64,
            PrimitiveType::String => DataType::Utf8,
            PrimitiveType::Binary => DataType::Binary,
            PrimitiveType::Date => DataType::Date32,
            PrimitiveType::Timestamp => {
                DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, None)
            }
            PrimitiveType::Timestamptz => {
                DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, Some("UTC".into()))
            }
            _ => DataType::Utf8, // Fallback for other types
        },
        _ => arrow::datatypes::DataType::Utf8, // Fallback for complex types
    }
}

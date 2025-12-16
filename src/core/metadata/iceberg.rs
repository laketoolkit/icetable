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
use iceberg::io::FileIO;
use iceberg::spec::{DataFile, Summary, TableMetadata};
use iceberg::table::StaticTable;
use object_store::ObjectStore;

use super::traits::{DataFileChanges, DataFileInfo, TableServiceReader, TableServiceWriter, OperationType, SnapshotInfo};
use crate::core::catalog::TableCommitter;
use crate::core::storage::{ObjectStoreExt, Storage, create_file_io, create_object_store};
use crate::error::{Error, Result};
use crate::utils::core::{extract_version_from_path, find_latest_metadata};
use crate::utils::create_spinner;

use super::iceberg_operations;
use super::iceberg_partition;
use super::refs::{self, RefInfo};
use super::refs_scanner;
use super::writer::SnapshotWriter;

/// Iceberg metadata service for transactional operations
pub struct IcebergMetadataService {
    table_path: String,
    file_io: FileIO,
    storage: Storage,
    /// The loaded Iceberg table (StaticTable for direct path access)
    table: StaticTable,
    /// Target branch for operations (defaults to "main")
    target_branch: Option<String>,
    /// Optional committer for catalog-aware commits
    committer: Option<TableCommitter>,
}

impl IcebergMetadataService {
    /// Load StaticTable from path
    async fn load_static_table(
        table_path: &str,
        file_io: &FileIO,
        storage: &Storage,
    ) -> Result<StaticTable> {
        use iceberg::NamespaceIdent;

        // find_latest_metadata returns full path (e.g., /path/to/table/metadata/00001-xxx.json)
        let metadata_file = find_latest_metadata(table_path, storage).await?;

        let table_ident = TableIdent::new(
            NamespaceIdent::new("iceberg".to_string()),
            "table".to_string(),
        );

        StaticTable::from_metadata_file(&metadata_file, table_ident, file_io.clone())
            .await
            .map_err(|e| Error::Metadata {
                message: format!("Failed to load table: {}", e),
            })
    }

    /// Create a new Iceberg metadata service
    pub async fn new_async(table_path: String) -> Result<Self> {
        let file_io = create_file_io(&table_path)?;
        let storage = create_object_store(&table_path).await?;
        let table = Self::load_static_table(&table_path, &file_io, &storage).await?;

        Ok(Self {
            table_path,
            file_io,
            storage,
            table,
            target_branch: None,
            committer: None,
        })
    }

    /// Create a new Iceberg metadata service targeting a specific branch
    pub async fn new_with_branch(table_path: String, branch: Option<String>) -> Result<Self> {
        let file_io = create_file_io(&table_path)?;
        let storage = create_object_store(&table_path).await?;
        let table = Self::load_static_table(&table_path, &file_io, &storage).await?;

        Ok(Self {
            table_path,
            file_io,
            storage,
            table,
            target_branch: branch,
            committer: None,
        })
    }

    /// Create a new Iceberg metadata service from a catalog-loaded table
    ///
    /// This ensures we use the metadata from the catalog, not from storage,
    /// which is required for proper catalog commit consistency.
    pub async fn from_catalog_table(
        catalog_table: &crate::core::IcebergTable,
        branch: Option<String>,
        committer: TableCommitter,
    ) -> Result<Self> {
        let table_path = catalog_table.metadata().location().to_string();
        let file_io = create_file_io(&table_path)?;
        let storage = create_object_store(&table_path).await?;

        // Create StaticTable from the catalog table's metadata
        // This ensures UUID and snapshot IDs match the catalog
        let table_ident = catalog_table.identifier().clone();
        let static_table = StaticTable::from_metadata(
            catalog_table.metadata().clone(),
            table_ident,
            file_io.clone(),
        )
        .await
        .map_err(|e| Error::Metadata {
            message: format!("Failed to create static table from catalog metadata: {}", e),
        })?;

        Ok(Self {
            table_path,
            file_io,
            storage,
            table: static_table,
            target_branch: branch,
            committer: Some(committer),
        })
    }

    /// Create a read-only Iceberg metadata service from a catalog-loaded table
    ///
    /// Use this for read-only operations (analyze, inspect) that don't need
    /// to commit changes. Uses catalog metadata for consistency but without a committer.
    pub async fn from_catalog_table_readonly(
        catalog_table: &crate::core::IcebergTable,
    ) -> Result<Self> {
        let table_path = catalog_table.metadata().location().to_string();
        let file_io = create_file_io(&table_path)?;
        let storage = create_object_store(&table_path).await?;

        // Create StaticTable from the catalog table's metadata
        let table_ident = catalog_table.identifier().clone();
        let static_table = StaticTable::from_metadata(
            catalog_table.metadata().clone(),
            table_ident,
            file_io.clone(),
        )
        .await
        .map_err(|e| Error::Metadata {
            message: format!("Failed to create static table from catalog metadata: {}", e),
        })?;

        Ok(Self {
            table_path,
            file_io,
            storage,
            table: static_table,
            target_branch: None,
            committer: None,
        })
    }

    /// Refresh the table after modifications
    pub async fn refresh(&mut self) -> Result<()> {
        self.table =
            Self::load_static_table(&self.table_path, &self.file_io, &self.storage).await?;
        Ok(())
    }

    /// Get access to the underlying table
    pub fn table(&self) -> &StaticTable {
        &self.table
    }

    /// Get table metadata
    pub fn metadata(&self) -> Arc<TableMetadata> {
        self.table.metadata()
    }

    /// Get the target branch name (defaults to "main" if not set)
    pub fn target_branch(&self) -> &str {
        self.target_branch.as_deref().unwrap_or("main")
    }

    /// Load current table metadata
    ///
    /// Returns the cached metadata from the StaticTable. Call `refresh()`
    /// after modifications to get updated metadata.
    pub async fn load_metadata(&self) -> Result<(Arc<TableMetadata>, i32)> {
        let metadata_file = find_latest_metadata(&self.table_path, &self.storage).await?;
        let version = extract_version_from_path(&metadata_file).unwrap_or(0);
        Ok((self.table.metadata(), version))
    }

    /// Get the FileIO for loading manifests
    pub fn file_io(&self) -> &FileIO {
        &self.file_io
    }

    /// Get the table path
    pub fn table_path(&self) -> &str {
        &self.table_path
    }

    /// Alias for table_path() - for convenience
    pub fn path(&self) -> &str {
        &self.table_path
    }

    /// Get the storage backend
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Get the committer if configured (for catalog-aware operations)
    pub fn committer(&self) -> Option<TableCommitter> {
        self.committer.clone()
    }

    /// Get current metadata file path
    pub async fn current_metadata_path(&self) -> Result<String> {
        find_latest_metadata(&self.table_path, &self.storage).await
    }

    /// List all references (branches and tags) from raw metadata JSON
    ///
    /// Note: This parses raw JSON because iceberg 0.7 doesn't have a method
    /// to list all refs (only `snapshot_for_ref()` for single lookups).
    pub async fn list_refs(&self) -> Result<Vec<RefInfo>> {
        let metadata_path = self.current_metadata_path().await?;
        refs::list_refs(&self.storage, &metadata_path).await
    }

    /// Resolve branch name to snapshot ID using iceberg's native API
    ///
    /// If branch is None, returns current snapshot ID.
    /// If branch is Some, uses iceberg's `snapshot_for_ref()` directly.
    pub async fn resolve_branch_snapshot_id(&self, branch: Option<&str>) -> Result<i64> {
        let (metadata, _) = self.load_metadata().await?;
        refs::resolve_branch_snapshot_id(&metadata, branch)
    }

    /// List data files for a specific snapshot (by ID)
    pub async fn list_data_files_for_snapshot(
        &self,
        snapshot_id: i64,
    ) -> Result<Vec<DataFileInfo>> {
        use futures::TryStreamExt;

        // Build scan targeting the specific snapshot
        let scan = self
            .table
            .scan()
            .snapshot_id(snapshot_id)
            .build()
            .map_err(|e| Error::Metadata {
                message: format!("Failed to build scan: {}", e),
            })?;

        // Use plan_files() to get all data files
        let tasks: Vec<_> = scan
            .plan_files()
            .await
            .map_err(|e| Error::Metadata {
                message: format!("Failed to plan files: {}", e),
            })?
            .try_collect()
            .await
            .map_err(|e| Error::Metadata {
                message: format!("Failed to collect file tasks: {}", e),
            })?;

        // Convert FileScanTasks to DataFileInfo
        let data_files = tasks
            .iter()
            .map(|task| DataFileInfo {
                path: task.data_file_path().to_string(),
                size: task.length,
                record_count: task.record_count.unwrap_or(0),
                partition: iceberg_partition::extract_partition_from_path_static(
                    task.data_file_path(),
                ),
            })
            .collect();

        Ok(data_files)
    }

    /// List data files for a branch (or current if None)
    pub async fn list_data_files_for_branch(
        &self,
        branch: Option<&str>,
    ) -> Result<Vec<DataFileInfo>> {
        let snapshot_id = self.resolve_branch_snapshot_id(branch).await?;
        self.list_data_files_for_snapshot(snapshot_id).await
    }

    /// Get the branch name to use (defaults to "main" if None)
    pub fn branch_name_or_default(branch: Option<&str>) -> &str {
        refs::branch_name_or_default(branch)
    }
}

#[async_trait]
impl TableServiceReader for IcebergMetadataService {
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
        use futures::TryStreamExt;

        // Build scan, optionally targeting a specific branch/snapshot
        let mut scan_builder = self.table.scan();

        // If targeting a specific branch, get its snapshot ID
        if let Some(ref branch) = self.target_branch {
            let metadata = self.table.metadata();
            if let Some(snapshot) = metadata.snapshot_for_ref(branch) {
                scan_builder = scan_builder.snapshot_id(snapshot.snapshot_id());
            }
        }

        let scan = scan_builder.build().map_err(|e| Error::Metadata {
            message: format!("Failed to build scan: {}", e),
        })?;

        // Use plan_files() to get all data files
        let tasks: Vec<_> = scan
            .plan_files()
            .await
            .map_err(|e| Error::Metadata {
                message: format!("Failed to plan files: {}", e),
            })?
            .try_collect()
            .await
            .map_err(|e| Error::Metadata {
                message: format!("Failed to collect file tasks: {}", e),
            })?;

        // Convert FileScanTasks to DataFileInfo
        let data_files = tasks
            .iter()
            .map(|task| DataFileInfo {
                path: task.data_file_path().to_string(),
                size: task.length,
                record_count: task.record_count.unwrap_or(0),
                partition: iceberg_partition::extract_partition_from_path_static(
                    task.data_file_path(),
                ),
            })
            .collect();

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

    fn data_directory(&self) -> PathBuf {
        PathBuf::from(format!("{}/data", self.table_path.trim_end_matches('/')))
    }

    async fn scan_data_files_on_storage(&self) -> Result<Vec<DataFileInfo>> {
        let data_prefix = format!("{}/data/", self.table_path.trim_end_matches('/'));

        let pb = create_spinner("Listing files on storage");

        let all_objects = self.storage.list_prefix(&data_prefix).await?;

        pb.finish_and_clear();

        let all_files: Vec<DataFileInfo> = all_objects
            .iter()
            .filter(|obj| obj.location.to_string().ends_with(".parquet"))
            .map(|obj| {
                let path_str = obj.location.to_string();
                let partition = iceberg_partition::extract_partition_from_path_static(&path_str);
                DataFileInfo {
                    path: path_str,
                    size: obj.size,
                    record_count: 0,
                    partition,
                }
            })
            .collect();

        Ok(all_files)
    }

    async fn get_all_referenced_files(&self) -> Result<std::collections::HashSet<String>> {
        refs_scanner::scan_all_referenced_files(&self.table).await
    }

    async fn schema(&self) -> Result<Arc<arrow::datatypes::Schema>> {
        let (metadata, _) = self.load_metadata().await?;
        let iceberg_schema = metadata.current_schema();

        // Use iceberg's native schema conversion
        let arrow_schema = iceberg::arrow::schema_to_arrow_schema(iceberg_schema).map_err(|e| {
            Error::Metadata {
                message: format!("Failed to convert schema: {}", e),
            }
        })?;

        Ok(Arc::new(arrow_schema))
    }

    fn object_store(&self) -> Arc<dyn ObjectStore> {
        // storage is already an Arc<dyn ObjectStore>, just clone it
        self.storage.clone()
    }
}

#[async_trait]
impl TableServiceWriter for IcebergMetadataService {
    async fn write_snapshot(
        &self,
        changes: DataFileChanges,
        operation: OperationType,
        summary: HashMap<String, String>,
    ) -> Result<SnapshotInfo> {
        // Write operations require a catalog for proper atomicity and concurrency control
        let use_catalog = self.committer.as_ref().is_some_and(|c| c.uses_catalog());
        if !use_catalog {
            return Err(Error::CatalogRequiredForWrite {
                operation: format!("{:?}", operation).to_lowercase(),
            });
        }

        // Load current metadata
        let (metadata, _current_version) = self.load_metadata().await?;
        let partition_spec = metadata.default_partition_spec();
        let schema_id = metadata.current_schema().schema_id();
        // Create snapshot writer with storage for metadata operations
        let writer = SnapshotWriter::with_storage(
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
        let timestamp_nanos = crate::utils::core::generate_unique_id();

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

        // Commit via catalog (required - validated at function entry)
        self.committer
            .as_ref()
            .expect("catalog required - validated at entry")
            .commit_add_snapshot(&metadata, snapshot, target_branch)
            .await?;

        Ok(SnapshotInfo {
            id: snapshot_id,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            operation: operation.to_string(),
            summary: full_summary,
            parent_id: parent_snapshot_id,
        })
    }
}


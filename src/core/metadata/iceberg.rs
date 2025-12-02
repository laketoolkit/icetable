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
use iceberg::io::{FileIO, FileIOBuilder};
use iceberg::spec::{
    DataContentType, DataFile, DataFileBuilder, DataFileFormat, FormatVersion, ManifestList,
    ManifestListWriter, ManifestStatus, ManifestWriterBuilder, Snapshot, Struct, Summary,
    TableMetadata, TableMetadataBuilder, MAIN_BRANCH,
};
use iceberg::table::StaticTable;
use iceberg::TableIdent;

use super::traits::{
    DataFileChanges, DataFileInfo, MetadataService, OperationType, SnapshotInfo,
};
use crate::error::{Error, Result};

/// Iceberg metadata service for transactional operations
pub struct IcebergMetadataService {
    table_path: PathBuf,
    file_io: FileIO,
}

impl IcebergMetadataService {
    /// Create a new Iceberg metadata service
    pub fn new(table_path: PathBuf) -> Result<Self> {
        let file_io = FileIOBuilder::new_fs_io()
            .build()
            .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))?;

        Ok(Self {
            table_path,
            file_io,
        })
    }

    /// Get the metadata directory path
    fn metadata_dir(&self) -> PathBuf {
        self.table_path.join("metadata")
    }

    /// Read current version from version-hint.text
    fn read_version_hint(&self) -> Result<i32> {
        let version_hint_path = self.metadata_dir().join("version-hint.text");
        Ok(std::fs::read_to_string(&version_hint_path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1))
    }

    /// Load current table metadata
    async fn load_metadata(&self) -> Result<(Arc<TableMetadata>, i32)> {
        let current_version = self.read_version_hint()?;
        let metadata_file = self
            .metadata_dir()
            .join(format!("v{}.metadata.json", current_version));

        let table_ident = TableIdent::from_strs(&["iceberg", "table"])
            .map_err(|e| Error::General(format!("Failed to create table ident: {}", e)))?;

        let static_table = StaticTable::from_metadata_file(
            &metadata_file.to_string_lossy(),
            table_ident,
            self.file_io.clone(),
        )
        .await
        .map_err(|e| Error::General(format!("Failed to load table metadata: {}", e)))?;

        Ok((static_table.metadata(), current_version))
    }

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
        let manifest_path = self.metadata_dir().join(&manifest_filename);

        let output_file = self
            .file_io
            .new_output(&manifest_path.to_string_lossy())
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
        let manifest_list_path = self.metadata_dir().join(&manifest_list_filename);

        let manifest_list_output = self
            .file_io
            .new_output(&manifest_list_path.to_string_lossy())
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

        Ok(manifest_list_path.to_string_lossy().to_string())
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

        let build_result = TableMetadataBuilder::new_from_metadata(
            old_metadata,
            Some(metadata_log_path),
        )
        .set_branch_snapshot(snapshot, MAIN_BRANCH)
        .map_err(|e| Error::General(format!("Failed to set snapshot: {}", e)))?
        .build()
        .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        Ok(build_result.metadata)
    }

    /// Write metadata to file
    async fn write_metadata_file(&self, metadata: &TableMetadata, version: i32) -> Result<()> {
        let metadata_path = self
            .metadata_dir()
            .join(format!("v{}.metadata.json", version));

        let metadata_json = serde_json::to_string_pretty(metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        std::fs::write(&metadata_path, metadata_json)
            .map_err(|e| Error::General(format!("Failed to write metadata file: {}", e)))?;

        Ok(())
    }

    /// Update version-hint.text
    fn update_version_hint(&self, version: i32) -> Result<()> {
        let version_hint_path = self.metadata_dir().join("version-hint.text");
        std::fs::write(&version_hint_path, version.to_string())
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
        let mut partition = HashMap::new();

        // Parse partition values from the file path (e.g., "year=2024/month=01/file.parquet")
        let path = data_file.file_path();
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

        let manifest_list = ManifestList::parse_with_version(&manifest_list_content, FormatVersion::V2)
            .map_err(|e| Error::General(format!("Failed to parse manifest list: {}", e)))?;

        // Read all manifests and collect data files
        let mut data_files = Vec::new();

        for manifest_file_entry in manifest_list.entries() {
            let manifest = manifest_file_entry
                .load_manifest(&self.file_io)
                .await
                .map_err(|e| Error::General(format!("Failed to load manifest: {}", e)))?;

            for entry in manifest.entries() {
                if entry.status != ManifestStatus::Deleted {
                    data_files.push(Self::from_iceberg_data_file(&entry.data_file));
                }
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
        let new_metadata =
            self.update_metadata((*metadata).clone(), snapshot, current_version)?;

        // Write new metadata file
        let new_version = current_version + 1;
        self.write_metadata_file(&new_metadata, new_version).await?;

        // Update version hint
        self.update_version_hint(new_version)?;

        Ok(SnapshotInfo {
            id: snapshot_id,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            operation: operation.to_string(),
            summary: full_summary,
            parent_id: parent_snapshot_id,
        })
    }

    fn data_directory(&self) -> PathBuf {
        self.table_path.join("data")
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
            PrimitiveType::Timestamp => DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, None),
            PrimitiveType::Timestamptz => DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, Some("UTC".into())),
            _ => DataType::Utf8, // Fallback for other types
        },
        _ => arrow::datatypes::DataType::Utf8, // Fallback for complex types
    }
}

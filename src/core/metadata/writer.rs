//! Snapshot and manifest writing
//!
//! This module provides functionality to write Iceberg manifests and construct
//! snapshots. It does NOT handle committing - that's the responsibility of
//! TableCommitter implementations.
//!
//! # Architecture
//!
//! ```text
//! SnapshotWriter
//!   ├── write_manifest()     -> ManifestFile
//!   ├── write_manifest_list() -> manifest list path
//!   └── build_snapshot()     -> Snapshot
//!
//! The actual commit (writing metadata.json) is handled by:
//!   - DirectCommitter: writes directly to storage
//!   - CatalogCommitter: uses catalog API
//! ```

use std::collections::HashMap;

use bytes::Bytes;
use iceberg::io::FileIO;
use iceberg::spec::{
    DataContentType, DataFile, DataFileBuilder, DataFileFormat, MAIN_BRANCH, ManifestFile,
    ManifestListWriter, ManifestWriterBuilder, Snapshot, Struct, Summary, TableMetadata,
    TableMetadataBuilder,
};

use crate::core::storage::{ObjectStoreExt, Storage};
use crate::error::{Error, Result};

// Re-use DataFileInfo from traits module (single source of truth)
pub use super::traits::DataFileInfo;

/// Result of preparing a snapshot (before commit)
#[derive(Debug)]
pub struct PreparedSnapshot {
    /// The snapshot ready to be committed
    pub snapshot: Snapshot,
    /// Path to the manifest list file
    pub manifest_list_path: String,
}

/// Writer for Iceberg snapshots and manifests
///
/// This handles writing manifest files and manifest lists to storage,
/// and constructing Snapshot objects. Can also write metadata files
/// if storage is provided.
pub struct SnapshotWriter {
    /// Table location (e.g., "s3://bucket/table")
    table_path: String,
    /// FileIO for iceberg operations (writes manifests)
    file_io: FileIO,
    /// Optional storage for writing metadata files
    storage: Option<Storage>,
}

impl SnapshotWriter {
    /// Create a new snapshot writer
    pub fn new(table_path: String, file_io: FileIO) -> Self {
        Self {
            table_path: table_path.trim_end_matches('/').to_string(),
            file_io,
            storage: None,
        }
    }

    /// Create a new snapshot writer with storage for metadata operations
    pub fn with_storage(table_path: String, file_io: FileIO, storage: Storage) -> Self {
        Self {
            table_path: table_path.trim_end_matches('/').to_string(),
            file_io,
            storage: Some(storage),
        }
    }

    /// Get the metadata directory path
    fn metadata_dir(&self) -> String {
        format!("{}/metadata", self.table_path)
    }

    /// Prepare a snapshot with data files
    ///
    /// This writes the manifest and manifest list files, and returns a
    /// PreparedSnapshot that can be committed using TableCommitter.
    pub async fn prepare_append(
        &self,
        data_files: &[DataFileInfo],
        metadata: &TableMetadata,
        parent_snapshot_id: Option<i64>,
    ) -> Result<PreparedSnapshot> {
        let snapshot_id = chrono::Utc::now().timestamp_millis();
        let sequence_number = metadata
            .current_snapshot()
            .map(|s| s.sequence_number() + 1)
            .unwrap_or(1);

        let timestamp_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time is after UNIX epoch")
            .as_nanos();

        // Convert DataFileInfo to Iceberg DataFile
        let iceberg_files = self.to_iceberg_data_files(data_files, metadata)?;

        // Write manifest
        let manifest_file = self
            .write_manifest(
                &iceberg_files,
                snapshot_id,
                sequence_number,
                metadata,
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

        // Calculate summary
        let total_records: u64 = data_files.iter().map(|f| f.record_count).sum();
        let total_size: u64 = data_files.iter().map(|f| f.size).sum();

        let summary = Summary {
            operation: iceberg::spec::Operation::Append,
            additional_properties: HashMap::from([
                ("added-data-files".to_string(), data_files.len().to_string()),
                ("added-records".to_string(), total_records.to_string()),
                ("added-files-size".to_string(), total_size.to_string()),
                ("total-records".to_string(), total_records.to_string()),
                ("total-data-files".to_string(), data_files.len().to_string()),
            ]),
        };

        // Build snapshot
        let snapshot = self.build_snapshot(
            snapshot_id,
            parent_snapshot_id,
            sequence_number,
            manifest_list_path.clone(),
            summary,
            metadata.current_schema().schema_id(),
        );

        Ok(PreparedSnapshot {
            snapshot,
            manifest_list_path,
        })
    }

    /// Write a manifest file containing the given data files
    pub async fn write_manifest(
        &self,
        data_files: &[DataFile],
        snapshot_id: i64,
        sequence_number: i64,
        metadata: &TableMetadata,
        timestamp_nanos: u128,
    ) -> Result<ManifestFile> {
        let iceberg_schema = metadata.current_schema();
        let partition_spec = metadata.default_partition_spec();

        // Generate unique manifest filename
        let random_suffix = rand::random::<u32>();
        let manifest_filename = format!("{:016x}-{:08x}-m0.avro", timestamp_nanos, random_suffix);
        let manifest_path = format!("{}/{}", self.metadata_dir(), manifest_filename);

        let output_file = self
            .file_io
            .new_output(&manifest_path)
            .map_err(|e| Error::Metadata {
                message: format!("Failed to create manifest output: {}", e),
            })?;

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
                .map_err(|e| Error::Metadata {
                    message: format!("Failed to add file to manifest: {}", e),
                })?;
        }

        manifest_writer
            .write_manifest_file()
            .await
            .map_err(|e| Error::Metadata {
                message: format!("Failed to write manifest: {}", e),
            })
    }

    /// Write a manifest list containing the given manifest files
    pub async fn write_manifest_list(
        &self,
        manifest_file: ManifestFile,
        snapshot_id: i64,
        parent_snapshot_id: Option<i64>,
        sequence_number: i64,
        timestamp_nanos: u128,
    ) -> Result<String> {
        let manifest_list_filename =
            format!("snap-{}-0-{:016x}.avro", snapshot_id, timestamp_nanos);
        let manifest_list_path = format!("{}/{}", self.metadata_dir(), manifest_list_filename);

        let manifest_list_output =
            self.file_io
                .new_output(&manifest_list_path)
                .map_err(|e| Error::Metadata {
                    message: format!("Failed to create manifest list output: {}", e),
                })?;

        let mut manifest_list_writer = ManifestListWriter::v2(
            manifest_list_output,
            snapshot_id,
            parent_snapshot_id,
            sequence_number,
        );

        manifest_list_writer
            .add_manifests(vec![manifest_file].into_iter())
            .map_err(|e| Error::Metadata {
                message: format!("Failed to add manifest to list: {}", e),
            })?;

        manifest_list_writer
            .close()
            .await
            .map_err(|e| Error::Metadata {
                message: format!("Failed to close manifest list: {}", e),
            })?;

        Ok(manifest_list_path)
    }

    /// Build a snapshot object
    pub fn build_snapshot(
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

    /// Convert DataFileInfo to Iceberg DataFile
    pub fn to_iceberg_data_files(
        &self,
        files: &[DataFileInfo],
        metadata: &TableMetadata,
    ) -> Result<Vec<DataFile>> {
        let partition_spec = metadata.default_partition_spec();

        files
            .iter()
            .map(|info| {
                let partition_struct = self.build_partition_struct(info, partition_spec);

                DataFileBuilder::default()
                    .content(DataContentType::Data)
                    .file_path(info.path.clone())
                    .file_format(DataFileFormat::Parquet)
                    .partition(partition_struct)
                    .partition_spec_id(partition_spec.spec_id())
                    .record_count(info.record_count)
                    .file_size_in_bytes(info.size)
                    .build()
                    .map_err(|e| Error::Metadata {
                        message: format!("Failed to build DataFile: {}", e),
                    })
            })
            .collect()
    }

    /// Convert a single DataFileInfo to Iceberg DataFile
    pub fn to_iceberg_data_file(
        &self,
        info: &DataFileInfo,
        partition_spec: &iceberg::spec::PartitionSpec,
    ) -> Result<DataFile> {
        let partition_struct = self.build_partition_struct(info, partition_spec);

        DataFileBuilder::default()
            .content(DataContentType::Data)
            .file_path(info.path.clone())
            .file_format(DataFileFormat::Parquet)
            .partition(partition_struct)
            .partition_spec_id(partition_spec.spec_id())
            .record_count(info.record_count)
            .file_size_in_bytes(info.size)
            .build()
            .map_err(|e| Error::Metadata {
                message: format!("Failed to build DataFile: {}", e),
            })
    }

    /// Build a partition Struct from DataFileInfo
    pub fn build_partition_struct(
        &self,
        info: &DataFileInfo,
        partition_spec: &iceberg::spec::PartitionSpec,
    ) -> Struct {
        use iceberg::spec::Literal;

        if partition_spec.fields().is_empty() || info.partition.is_empty() {
            return Struct::empty();
        }

        let fields: Vec<Option<Literal>> = partition_spec
            .fields()
            .iter()
            .map(|field| {
                info.partition.get(&field.name).and_then(|value| {
                    super::iceberg_partition::convert_partition_value(value, &field.transform)
                })
            })
            .collect();

        fields.into_iter().collect()
    }

    /// Update table metadata with new snapshot (defaults to main branch)
    pub fn update_metadata(
        &self,
        old_metadata: TableMetadata,
        snapshot: Snapshot,
        current_metadata_path: &str,
    ) -> Result<TableMetadata> {
        self.update_metadata_for_branch(old_metadata, snapshot, current_metadata_path, MAIN_BRANCH)
    }

    /// Update table metadata with new snapshot on a specific branch
    pub fn update_metadata_for_branch(
        &self,
        old_metadata: TableMetadata,
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
            old_metadata,
            Some(metadata_filename.to_string()),
        )
        .set_branch_snapshot(snapshot, branch)
        .map_err(|e| Error::Metadata {
            message: format!("Failed to set snapshot: {}", e),
        })?
        .build()
        .map_err(|e| Error::Metadata {
            message: format!("Failed to build metadata: {}", e),
        })?;

        Ok(build_result.metadata)
    }

    /// Write metadata to file using standard Iceberg naming
    ///
    /// Requires storage to be set via `with_storage`.
    /// Validates metadata before writing and performs conflict detection.
    pub async fn write_metadata_file(
        &self,
        metadata: &TableMetadata,
        current_metadata_path: &str,
    ) -> Result<String> {
        use crate::utils::core::{
            extract_version_from_path, metadata_location_filename, next_metadata_location,
        };

        let storage = self.storage.as_ref().ok_or_else(|| Error::Metadata {
            message: "Storage not configured. Use with_storage() to enable metadata writes."
                .to_string(),
        })?;

        // Validate metadata before writing
        super::iceberg_validator::validate_or_error(metadata)?;

        // Parse current metadata location and get next version
        let expected_version = extract_version_from_path(current_metadata_path).unwrap_or(0);

        // Check for conflicts before writing
        super::iceberg_conflict::check_and_fail_on_conflict(
            &self.table_path,
            storage,
            expected_version,
        )
        .await?;

        // Generate next metadata location
        let next_location = next_metadata_location(current_metadata_path)?;
        let metadata_path = format!(
            "{}/{}",
            self.metadata_dir(),
            metadata_location_filename(&next_location)
        );

        let metadata_json =
            serde_json::to_string_pretty(metadata).map_err(|e| Error::Serialization {
                message: format!("Failed to serialize metadata: {}", e),
            })?;

        storage
            .put_bytes_str(&metadata_path, Bytes::from(metadata_json))
            .await?;

        Ok(metadata_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_dir() {
        let file_io = iceberg::io::FileIOBuilder::new_fs_io()
            .build()
            .expect("Failed to create FileIO");

        let writer = SnapshotWriter::new("s3://bucket/table".to_string(), file_io);
        assert_eq!(writer.metadata_dir(), "s3://bucket/table/metadata");
    }

    #[test]
    fn test_metadata_dir_strips_trailing_slash() {
        let file_io = iceberg::io::FileIOBuilder::new_fs_io()
            .build()
            .expect("Failed to create FileIO");

        let writer = SnapshotWriter::new("s3://bucket/table/".to_string(), file_io);
        assert_eq!(writer.metadata_dir(), "s3://bucket/table/metadata");
    }
}

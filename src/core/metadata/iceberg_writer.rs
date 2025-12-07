//! Iceberg snapshot writer for transactional operations
//!
//! Encapsulates the logic for writing Iceberg snapshots, manifests, and metadata files.
//! This module is used by `IcebergMetadataService` to perform transactional writes.

use std::sync::Arc;

use super::traits::DataFileInfo;
use crate::core::metadata::iceberg_partition;
use crate::core::storage::StorageBackend;
use crate::error::{Error, Result};
use bytes;

use iceberg::io::FileIO;
use iceberg::spec::{
    DataContentType, DataFile, DataFileBuilder, DataFileFormat, MAIN_BRANCH, ManifestListWriter,
    ManifestWriterBuilder, Snapshot, Struct, Summary, TableMetadata, TableMetadataBuilder,
};

/// Writer for Iceberg snapshots and metadata
pub struct IcebergSnapshotWriter {
    table_path: String,
    file_io: FileIO,
    storage: Arc<dyn StorageBackend>,
}

impl IcebergSnapshotWriter {
    /// Create a new snapshot writer
    pub fn new(table_path: String, file_io: FileIO, storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            table_path,
            file_io,
            storage,
        }
    }

    /// Get the metadata directory path
    fn metadata_dir(&self) -> String {
        format!("{}/metadata", self.table_path.trim_end_matches('/'))
    }

    /// Write a manifest file containing the given data files
    pub async fn write_manifest(
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
    pub async fn write_manifest_list(
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

        let build_result =
            TableMetadataBuilder::new_from_metadata(old_metadata, Some(metadata_filename.to_string()))
                .set_branch_snapshot(snapshot, branch)
                .map_err(|e| Error::General(format!("Failed to set snapshot: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        Ok(build_result.metadata)
    }

    /// Write metadata to file using standard Iceberg naming: `<version>-<uuid>.metadata.json`
    ///
    /// Validates metadata before writing to ensure consistency.
    /// Also performs conflict detection to prevent concurrent modifications.
    ///
    /// Returns the path of the new metadata file.
    pub async fn write_metadata_file(
        &self,
        metadata: &TableMetadata,
        current_metadata_path: &str,
    ) -> Result<String> {
        use crate::core::storage::traits::PutOptions;
        use crate::core::utils::{extract_version_from_path, metadata_location_filename, next_metadata_location};

        // Validate metadata before writing
        super::iceberg_validator::validate_or_error(metadata)?;

        // Parse current metadata location and get next version
        let expected_version = extract_version_from_path(current_metadata_path);

        // Check for conflicts before writing (optimistic concurrency control)
        super::iceberg_conflict::check_and_fail_on_conflict(
            &self.table_path,
            &self.storage,
            expected_version,
        )
        .await?;

        // Generate next metadata location with new UUID
        let next_location = next_metadata_location(current_metadata_path)?;
        let metadata_path = format!("{}/{}", self.metadata_dir(), metadata_location_filename(&next_location));

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

        Ok(metadata_path)
    }

    /// Convert DataFileInfo to Iceberg DataFile
    pub fn to_iceberg_data_file(
        &self,
        info: &DataFileInfo,
        partition_spec: &iceberg::spec::PartitionSpec,
    ) -> Result<DataFile> {
        // Build the partition struct based on the partition spec fields
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
            .map_err(|e| Error::General(format!("Failed to build DataFile: {}", e)))
    }

    /// Build a partition Struct from DataFileInfo and PartitionSpec
    fn build_partition_struct(
        &self,
        info: &DataFileInfo,
        partition_spec: &iceberg::spec::PartitionSpec,
    ) -> Struct {
        use iceberg::spec::Literal;

        // If no partition fields or no partition values, return empty struct
        if partition_spec.fields().is_empty() || info.partition.is_empty() {
            return Struct::empty();
        }

        // Build the struct with values in the order of partition spec fields
        let fields: Vec<Option<Literal>> = partition_spec
            .fields()
            .iter()
            .map(|field| {
                // Look up the value in info.partition by field name
                info.partition.get(&field.name).and_then(|value| {
                    // Convert value based on transform type
                    iceberg_partition::convert_partition_value(value, &field.transform)
                })
            })
            .collect();

        fields.into_iter().collect()
    }
}

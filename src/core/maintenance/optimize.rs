//! Optimize service for compacting small files
//!
//! This service handles file compaction for both Delta Lake and Iceberg tables
//! by using the MetadataService trait for transactional operations.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use arrow::array::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use super::{FileGroup, MaintenanceConfig, group_files_by_partition};
use crate::core::metadata::{
    DataFileChanges, DataFileInfo, MaintenanceResult, MetadataService, OperationType,
};
use crate::core::utils::{format_bytes, generate_unique_id};
use crate::error::{Error, Result};

/// Service for optimizing tables by compacting small files
pub struct OptimizeService {
    config: MaintenanceConfig,
}

impl OptimizeService {
    /// Create a new optimize service with default config
    pub fn new() -> Self {
        Self {
            config: MaintenanceConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: MaintenanceConfig) -> Self {
        Self { config }
    }

    /// Analyze files and identify groups that need compaction
    pub fn analyze(&self, files: &[DataFileInfo]) -> Vec<FileGroup> {
        let groups = group_files_by_partition(files.to_vec());

        groups
            .into_values()
            .filter(|g| g.needs_compaction(self.config.min_size))
            .collect()
    }

    /// Run the optimize operation
    pub async fn execute<M: MetadataService>(
        &self,
        metadata_service: &M,
    ) -> Result<MaintenanceResult> {
        // Get current data files
        let files = metadata_service.list_data_files().await?;

        if files.is_empty() {
            return Ok(MaintenanceResult::no_changes("No data files found"));
        }

        // Find groups that need compaction
        let groups_to_compact = self.analyze(&files);

        if groups_to_compact.is_empty() {
            return Ok(MaintenanceResult::no_changes(
                "No small files need compaction",
            ));
        }

        // Get schema for writing
        let schema = metadata_service.schema().await?;
        let data_dir = metadata_service.data_directory();

        let mut changes = DataFileChanges::new();
        let mut total_input_files = 0;
        let mut total_output_files = 0;

        // Process each group
        for group in groups_to_compact {
            if self.config.dry_run {
                total_input_files += group.files.len();
                total_output_files += 1;
                continue;
            }

            // Compact files in this group
            let (new_files, old_files) = self.compact_group(&group, &schema, &data_dir).await?;

            total_input_files += old_files.len();
            total_output_files += new_files.len();

            changes.added.extend(new_files);
            changes.removed.extend(old_files);
        }

        if self.config.dry_run {
            let mut details = HashMap::new();
            details.insert("mode".to_string(), "dry-run".to_string());
            details.insert(
                "would_compact".to_string(),
                format!("{} -> {} files", total_input_files, total_output_files),
            );

            return Ok(MaintenanceResult {
                files_added: total_output_files,
                files_removed: total_input_files,
                bytes_added: 0,
                bytes_removed: 0,
                records_affected: 0,
                operation: "optimize (dry-run)".to_string(),
                details,
            });
        }

        if changes.is_empty() {
            return Ok(MaintenanceResult::no_changes("No files compacted"));
        }

        // Build summary
        let mut summary = HashMap::new();
        summary.insert("input_files".to_string(), changes.removed.len().to_string());
        summary.insert("output_files".to_string(), changes.added.len().to_string());
        summary.insert(
            "target_size".to_string(),
            format_bytes(self.config.target_size),
        );

        // Commit the changes
        let snapshot_info = metadata_service
            .write_snapshot(changes.clone(), OperationType::Replace, summary)
            .await?;

        let mut details = HashMap::new();
        details.insert("snapshot_id".to_string(), snapshot_info.id.to_string());

        Ok(MaintenanceResult {
            files_added: changes.added.len(),
            files_removed: changes.removed.len(),
            bytes_added: changes.bytes_added(),
            bytes_removed: changes.bytes_removed(),
            records_affected: changes.records_removed(),
            operation: "optimize".to_string(),
            details,
        })
    }

    /// Compact a group of files into one or more larger files
    async fn compact_group(
        &self,
        group: &FileGroup,
        schema: &Arc<arrow::datatypes::Schema>,
        data_dir: &Path,
    ) -> Result<(Vec<DataFileInfo>, Vec<DataFileInfo>)> {
        // Generate output file path
        let unique_id = generate_unique_id();
        let output_path = if group.partition_key.is_empty() {
            data_dir.join(format!("compact-{}.parquet", unique_id))
        } else {
            data_dir
                .join(&group.partition_key)
                .join(format!("compact-{}.parquet", unique_id))
        };

        // Ensure parent directory exists
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::General(format!("Failed to create directory: {}", e)))?;
        }

        // Read and merge all input files
        let batches = self.read_files_as_batches(&group.files)?;

        if batches.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // Write merged output
        let output_info = self.write_parquet_file(&output_path, &batches, schema)?;

        // Parse partition from group key
        let partition = self.parse_partition_key(&group.partition_key);
        let mut output_with_partition = output_info;
        output_with_partition.partition = partition;

        Ok((vec![output_with_partition], group.files.clone()))
    }

    /// Read multiple parquet files as record batches
    fn read_files_as_batches(&self, files: &[DataFileInfo]) -> Result<Vec<RecordBatch>> {
        let mut all_batches = Vec::new();

        for file in files {
            let path = file.path.strip_prefix("file://").unwrap_or(&file.path);
            let file_handle = std::fs::File::open(path)
                .map_err(|e| Error::General(format!("Failed to open file {}: {}", path, e)))?;

            let reader = ParquetRecordBatchReaderBuilder::try_new(file_handle)
                .map_err(|e| Error::General(format!("Failed to create reader: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build reader: {}", e)))?;

            for batch_result in reader {
                let batch = batch_result
                    .map_err(|e| Error::General(format!("Failed to read batch: {}", e)))?;
                all_batches.push(batch);
            }
        }

        Ok(all_batches)
    }

    /// Write record batches to a parquet file
    fn write_parquet_file(
        &self,
        path: &Path,
        batches: &[RecordBatch],
        schema: &Arc<arrow::datatypes::Schema>,
    ) -> Result<DataFileInfo> {
        let file = std::fs::File::create(path)
            .map_err(|e| Error::General(format!("Failed to create file: {}", e)))?;

        let props = WriterProperties::builder()
            .set_compression(Compression::ZSTD(Default::default()))
            .build();

        let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props))
            .map_err(|e| Error::General(format!("Failed to create writer: {}", e)))?;

        let mut total_records = 0u64;
        for batch in batches {
            total_records += batch.num_rows() as u64;
            writer
                .write(batch)
                .map_err(|e| Error::General(format!("Failed to write batch: {}", e)))?;
        }

        writer
            .close()
            .map_err(|e| Error::General(format!("Failed to close writer: {}", e)))?;

        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        Ok(DataFileInfo {
            path: path.to_string_lossy().to_string(),
            size,
            record_count: total_records,
            partition: HashMap::new(),
        })
    }

    /// Parse a partition key string into a HashMap
    fn parse_partition_key(&self, key: &str) -> HashMap<String, String> {
        if key.is_empty() {
            return HashMap::new();
        }

        key.split('/')
            .filter_map(|part| {
                let mut split = part.splitn(2, '=');
                match (split.next(), split.next()) {
                    (Some(k), Some(v)) => Some((k.to_string(), v.to_string())),
                    _ => None,
                }
            })
            .collect()
    }
}

impl Default for OptimizeService {
    fn default() -> Self {
        Self::new()
    }
}

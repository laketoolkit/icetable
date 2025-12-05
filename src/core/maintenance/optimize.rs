//! Optimize service for compacting small files
//!
//! This service handles file compaction for both Delta Lake and Iceberg tables
//! by using the MetadataService trait for transactional operations.
//!
//! Features:
//! - **True streaming**: Batches are streamed directly from readers to writer
//!   without accumulating in memory
//! - **Parallel partitions**: Uses rayon for true CPU parallelization across
//!   partition groups
//! - **Cloud storage support**: Works with S3, GCS, Azure through object_store

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use arrow::array::RecordBatch;
use arrow::datatypes::SchemaRef;
use arrow_cast::cast;
use futures::TryStreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use object_store::ObjectStore;
use object_store::path::Path as ObjectPath;
use parquet::arrow::AsyncArrowWriter;
use parquet::arrow::async_reader::{ParquetObjectReader, ParquetRecordBatchStreamBuilder};
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use rayon::prelude::*;

use super::{FileGroup, MaintenanceConfig, group_files_by_partition};
use crate::core::metadata::{
    DataFileChanges, DataFileInfo, MaintenanceResult, MetadataService, OperationType,
};
use crate::core::utils::{format_bytes, generate_unique_id, normalize_relative_path};
use crate::error::{Error, Result};

/// Result of compacting a single partition group
struct CompactionResult {
    added: Vec<DataFileInfo>,
    removed: Vec<DataFileInfo>,
}

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
            .into_iter()
            .filter(|(key, g)| {
                // Filter by partition if specified
                if let Some(ref filter) = self.config.partition_filter
                    && key != filter
                {
                    return false;
                }
                g.needs_compaction(self.config.min_size)
            })
            .map(|(_, g)| g)
            .collect()
    }

    /// Run the optimize operation with parallel partition processing
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

        // Handle dry-run mode
        if self.config.dry_run {
            let total_input_files: usize = groups_to_compact.iter().map(|g| g.files.len()).sum();
            let total_output_files = groups_to_compact.len();

            let mut details = HashMap::new();
            details.insert("mode".to_string(), "dry-run".to_string());
            details.insert(
                "would_compact".to_string(),
                format!("{} -> {} files", total_input_files, total_output_files),
            );
            details.insert(
                "partitions".to_string(),
                groups_to_compact.len().to_string(),
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

        // Get schema and object store for I/O
        let schema = metadata_service.schema().await?;
        let data_dir = metadata_service.data_directory();
        let object_store = metadata_service.object_store();

        // Setup progress tracking
        let multi_progress = MultiProgress::new();
        let overall_pb = multi_progress.add(ProgressBar::new(groups_to_compact.len() as u64));
        overall_pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} Compacting {bar:30.cyan/blue} {pos}/{len} partitions ({percent}%) {msg}")
                .unwrap()
                .progress_chars("━━╺"),
        );
        overall_pb.enable_steady_tick(Duration::from_millis(100));

        // Counters for progress
        let files_processed = Arc::new(AtomicUsize::new(0));
        let bytes_written = Arc::new(AtomicU64::new(0));

        // Get tokio runtime handle for async operations inside rayon
        let rt = tokio::runtime::Handle::current();

        // Process partition groups in parallel using rayon
        let results: Vec<Result<CompactionResult>> = groups_to_compact
            .par_iter()
            .map(|group| {
                // Each rayon thread uses block_on to run async code
                let result = rt.block_on(self.compact_group_streaming(
                    group,
                    &schema,
                    &data_dir,
                    &object_store,
                ));

                // Update progress
                files_processed.fetch_add(group.files.len(), Ordering::Relaxed);
                overall_pb.inc(1);

                if let Ok(ref r) = result {
                    let added_bytes: u64 = r.added.iter().map(|f| f.size).sum();
                    bytes_written.fetch_add(added_bytes, Ordering::Relaxed);
                    overall_pb.set_message(format!(
                        "{} files -> {}",
                        files_processed.load(Ordering::Relaxed),
                        format_bytes(bytes_written.load(Ordering::Relaxed))
                    ));
                }

                result
            })
            .collect();

        overall_pb.finish_with_message("done");

        // Aggregate results
        let mut changes = DataFileChanges::new();
        for result in results {
            let compaction = result?;
            changes.added.extend(compaction.added);
            changes.removed.extend(compaction.removed);
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
        summary.insert(
            "parallelism".to_string(),
            self.config.parallelism.to_string(),
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

    /// Compact a group of files using true streaming
    ///
    /// Batches are streamed directly from input files to the output writer
    /// without accumulating in memory. This allows processing arbitrarily
    /// large partitions with constant memory usage.
    ///
    /// Uses the table's schema from metadata (not from parquet files) to ensure
    /// the output file has the current schema, handling schema evolution correctly.
    async fn compact_group_streaming(
        &self,
        group: &FileGroup,
        table_schema: &Arc<arrow::datatypes::Schema>,
        data_dir: &std::path::Path,
        object_store: &Arc<dyn ObjectStore>,
    ) -> Result<CompactionResult> {
        if group.files.is_empty() {
            return Ok(CompactionResult {
                added: Vec::new(),
                removed: Vec::new(),
            });
        }

        // Generate output file path
        let unique_id = generate_unique_id();
        let output_path = if group.partition_key.is_empty() {
            data_dir.join(format!("compact-{}.parquet", unique_id))
        } else {
            data_dir
                .join(&group.partition_key)
                .join(format!("compact-{}.parquet", unique_id))
        };

        // Get the table base path for computing relative paths
        let data_dir_str = data_dir.to_string_lossy();
        let table_base = data_dir_str.trim_end_matches("/data");

        // Convert output path to ObjectPath
        let output_path_str = output_path.to_string_lossy();
        let output_object_path = self.path_to_object_path(&output_path_str, table_base)?;

        // Create writer properties with ZSTD compression
        let props = WriterProperties::builder()
            .set_compression(Compression::ZSTD(Default::default()))
            .build();

        // Create async parquet writer
        let writer = parquet::arrow::async_writer::ParquetObjectWriter::new(
            object_store.clone(),
            output_object_path.clone(),
        );

        let mut async_writer = AsyncArrowWriter::try_new(writer, table_schema.clone(), Some(props))
            .map_err(|e| Error::General(format!("Failed to create async writer: {}", e)))?;

        let mut total_records = 0u64;

        // Stream batches from each input file directly to writer
        for file in &group.files {
            let path = self.path_to_object_path(&file.path, table_base)?;

            let reader =
                ParquetObjectReader::new(object_store.clone(), path).with_file_size(file.size);

            let builder = ParquetRecordBatchStreamBuilder::new(reader)
                .await
                .map_err(|e| {
                    Error::General(format!(
                        "Failed to create parquet reader for {}: {}",
                        file.path, e
                    ))
                })?;

            let mut stream = builder.build().map_err(|e| {
                Error::General(format!(
                    "Failed to build record batch stream for {}: {}",
                    file.path, e
                ))
            })?;

            // Stream each batch directly to writer - no intermediate buffer!
            while let Some(batch) = stream.try_next().await.map_err(|e| {
                Error::General(format!("Failed to read batch from {}: {}", file.path, e))
            })? {
                // Coerce batch to table schema to handle schema evolution
                let coerced = Self::coerce_batch_to_schema(&batch, table_schema)?;
                total_records += coerced.num_rows() as u64;
                async_writer
                    .write(&coerced)
                    .await
                    .map_err(|e| Error::General(format!("Failed to write batch: {}", e)))?;
            }
        }

        // Close writer and flush to storage
        async_writer
            .close()
            .await
            .map_err(|e| Error::General(format!("Failed to close writer: {}", e)))?;

        // Get the file size from object store
        let meta = object_store
            .head(&output_object_path)
            .await
            .map_err(|e| Error::General(format!("Failed to get file metadata: {}", e)))?;

        // Parse partition from group key
        let partition = self.parse_partition_key(&group.partition_key);

        let output_info = DataFileInfo {
            path: output_path.to_string_lossy().to_string(),
            size: meta.size,
            record_count: total_records,
            partition,
        };

        Ok(CompactionResult {
            added: vec![output_info],
            removed: group.files.clone(),
        })
    }

    /// Convert a file path string to an ObjectPath relative to the table base
    ///
    /// Uses centralized path normalization to handle `file://` prefixes consistently.
    fn path_to_object_path(&self, path: &str, table_base: &str) -> Result<ObjectPath> {
        // Use centralized normalization to compute relative path
        if let Some(relative) = normalize_relative_path(path, table_base) {
            return Ok(ObjectPath::from(relative));
        }

        // Fallback: use the path as-is (already relative or different base)
        // Strip file:// if present for ObjectPath compatibility
        let clean_path = path.strip_prefix("file://").unwrap_or(path);
        Ok(ObjectPath::from(clean_path))
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

    /// Coerce a batch to match the target schema
    ///
    /// This handles cases where batches from different files have slightly
    /// different schemas (e.g., different field metadata, different column order,
    /// or missing columns). Columns are matched by name, not position.
    fn coerce_batch_to_schema(
        batch: &RecordBatch,
        target_schema: &SchemaRef,
    ) -> Result<RecordBatch> {
        // If schemas match exactly, return as-is
        if batch.schema() == *target_schema {
            return Ok(batch.clone());
        }

        let batch_schema = batch.schema();
        let num_rows = batch.num_rows();

        // Match columns by name, not by position
        let columns: Vec<_> = target_schema
            .fields()
            .iter()
            .map(|target_field| {
                let target_name = target_field.name();
                let target_type = target_field.data_type();

                // Find column in source batch by name
                match batch_schema.column_with_name(target_name) {
                    Some((idx, _source_field)) => {
                        let source_column = batch.column(idx);
                        let source_type = source_column.data_type();

                        if source_type == target_type {
                            Ok(source_column.clone())
                        } else {
                            cast(source_column, target_type).map_err(|e| {
                                Error::General(format!(
                                    "Failed to cast column '{}' from {:?} to {:?}: {}",
                                    target_name, source_type, target_type, e
                                ))
                            })
                        }
                    }
                    None => {
                        // Column missing in source - create null array
                        use arrow::array::new_null_array;
                        Ok(new_null_array(target_type, num_rows))
                    }
                }
            })
            .collect::<Result<Vec<_>>>()?;

        RecordBatch::try_new(target_schema.clone(), columns)
            .map_err(|e| Error::General(format!("Failed to create coerced batch: {}", e)))
    }
}

impl Default for OptimizeService {
    fn default() -> Self {
        Self::new()
    }
}

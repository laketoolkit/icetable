//! Optimize service for compacting small files
//!
//! This service handles file compaction for both Delta Lake and Iceberg tables
//! by using the MetadataService trait for transactional operations.
//!
//! Features:
//! - **True streaming**: Batches are streamed directly from readers to writer
//!   without accumulating in memory
//! - **Concurrent partitions**: Uses async concurrency for I/O-bound partition
//!   processing with configurable parallelism
//! - **Target size splitting**: Splits output files when exceeding target_size
//!   to avoid OOM and produce optimally-sized files
//! - **Cloud storage support**: Works with S3, GCS, Azure through object_store

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use arrow::array::RecordBatch;
use arrow::datatypes::SchemaRef;
use arrow_cast::cast;
use futures::{StreamExt, TryStreamExt, stream};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use object_store::ObjectStore;
use object_store::path::Path as ObjectPath;
use parquet::arrow::AsyncArrowWriter;
use parquet::arrow::async_reader::{ParquetObjectReader, ParquetRecordBatchStreamBuilder};
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use super::{FileGroup, MaintenanceConfig, group_files_by_partition};
use crate::core::metadata::{
    DataFileChanges, DataFileInfo, MaintenanceResult, MetadataService, OperationType,
};
use crate::core::utils::{format_bytes, generate_unique_id, normalize_relative_path};
use crate::error::{Error, Result};
use crate::utils::register_cleanup_handler;

/// Result of compacting a single partition group
struct CompactionResult {
    added: Vec<DataFileInfo>,
    removed: Vec<DataFileInfo>,
}

/// Tracks temporary files created during compaction for cleanup on cancellation
struct TemporaryFileTracker {
    files: Vec<String>,
}

impl TemporaryFileTracker {
    fn new() -> Self {
        Self { files: Vec::new() }
    }

    fn add_file(&mut self, path: String) {
        self.files.push(path);
    }
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
                // Filter by partition if specified (supports wildcards)
                if let Some(ref filter) = self.config.partition_filter
                    && !super::matches_partition_filter(key, filter)
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
        let mut groups_to_compact = self.analyze(&files);

        if groups_to_compact.is_empty() {
            return Ok(MaintenanceResult::no_changes(
                "No small files need compaction",
            ));
        }

        // Sort groups by file count (most fragmented first) for incremental compaction
        groups_to_compact.sort_by(|a, b| b.files.len().cmp(&a.files.len()));

        // Apply max_files limit (incremental compaction)
        let mut limited_groups = Vec::new();
        let mut accumulated_files = 0usize;
        let mut accumulated_bytes = 0u64;
        let mut was_limited = false;

        for group in groups_to_compact {
            let group_files = group.files.len();
            let group_bytes = group.total_size;

            // Check max_files limit
            if let Some(max_files) = self.config.max_files
                && accumulated_files + group_files > max_files
                && !limited_groups.is_empty()
            {
                was_limited = true;
                break;
            }

            // Check max_bytes limit
            if let Some(max_bytes) = self.config.max_bytes
                && accumulated_bytes + group_bytes > max_bytes
                && !limited_groups.is_empty()
            {
                was_limited = true;
                break;
            }

            accumulated_files += group_files;
            accumulated_bytes += group_bytes;
            limited_groups.push(group);
        }

        let groups_to_compact = limited_groups;

        // Handle dry-run mode
        if self.config.dry_run {
            let total_input_files: usize = groups_to_compact.iter().map(|g| g.files.len()).sum();
            let total_output_files = groups_to_compact.len();
            let total_bytes: u64 = groups_to_compact.iter().map(|g| g.total_size).sum();

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
            details.insert("bytes_to_process".to_string(), format_bytes(total_bytes));

            if was_limited {
                details.insert("incremental".to_string(), "true".to_string());
                details.insert(
                    "note".to_string(),
                    "More files need compaction. Run again to continue.".to_string(),
                );
            }

            return Ok(MaintenanceResult {
                files_added: total_output_files,
                files_removed: total_input_files,
                bytes_added: 0,
                bytes_removed: total_bytes,
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
                .expect("hardcoded progress template is valid")
                .progress_chars("━━╺"),
        );
        overall_pb.enable_steady_tick(Duration::from_millis(100));

        // Counters for progress
        let files_processed = Arc::new(AtomicUsize::new(0));
        let bytes_written = Arc::new(AtomicU64::new(0));

        // Process partition groups concurrently using async streams
        let concurrency = self.config.parallelism;
        let results: Vec<Result<CompactionResult>> = stream::iter(groups_to_compact.iter())
            .map(|group| {
                let schema = schema.clone();
                let data_dir = data_dir.clone();
                let object_store = object_store.clone();
                let files_processed = files_processed.clone();
                let bytes_written = bytes_written.clone();
                let overall_pb = overall_pb.clone();
                let group = group.clone();

                async move {
                    let result = self
                        .compact_group_streaming(&group, &schema, &data_dir, &object_store)
                        .await;

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
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;

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

    /// Compact a group of files using true streaming with target size splitting
    ///
    /// Batches are streamed directly from input files to the output writer
    /// without accumulating in memory. When the current output file exceeds
    /// target_size, a new output file is started.
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

        // Get the table base path for computing relative paths
        let data_dir_str = data_dir.to_string_lossy();
        let table_base = data_dir_str.trim_end_matches("/data");

        // Parse partition from group key (used for all output files)
        let partition = self.parse_partition_key(&group.partition_key);

        // Writer properties with ZSTD compression
        let props = WriterProperties::builder()
            .set_compression(Compression::ZSTD(Default::default()))
            .build();

        // Track all output files
        let mut output_files: Vec<DataFileInfo> = Vec::new();

        // Track temporary files for cleanup
        let mut temp_tracker = TemporaryFileTracker::new();

        // Register cleanup handler for cancellation
        let tracker_for_cleanup = temp_tracker.files.clone();
        let object_store_for_cleanup = object_store.clone();
        register_cleanup_handler(move || {
            for path in &tracker_for_cleanup {
                let object_path = ObjectPath::from(path.as_str());
                let _ = tokio::runtime::Handle::current()
                    .block_on(async { object_store_for_cleanup.delete(&object_path).await });
            }
        });

        // Current writer state
        let mut current_writer: Option<
            AsyncArrowWriter<parquet::arrow::async_writer::ParquetObjectWriter>,
        > = None;
        let mut current_path: Option<std::path::PathBuf> = None;
        let mut current_records = 0u64;
        let mut current_bytes_estimate = 0u64;
        let mut file_counter = 0u32;

        // Stream batches from each input file
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

            // Stream each batch
            while let Some(batch) = stream.try_next().await.map_err(|e| {
                Error::General(format!("Failed to read batch from {}: {}", file.path, e))
            })? {
                let coerced = Self::coerce_batch_to_schema(&batch, table_schema)?;
                let batch_size_estimate = coerced.get_array_memory_size() as u64;

                // Check if we need to start a new file (before writing this batch)
                // Only split if we have written something and would exceed target
                if current_writer.is_some()
                    && current_bytes_estimate > 0
                    && current_bytes_estimate + batch_size_estimate > self.config.target_size
                {
                    // Finalize current writer
                    if let (Some(writer), Some(ref out_path)) =
                        (current_writer.take(), current_path.take())
                    {
                        writer.close().await.map_err(|e| {
                            Error::General(format!("Failed to close writer: {}", e))
                        })?;

                        let out_object_path =
                            self.path_to_object_path(&out_path.to_string_lossy(), table_base)?;
                        let meta = object_store.head(&out_object_path).await.map_err(|e| {
                            Error::General(format!("Failed to get file metadata: {}", e))
                        })?;

                        let file_path = out_path.to_string_lossy().to_string();
                        temp_tracker.add_file(file_path.clone());
                        output_files.push(DataFileInfo {
                            path: file_path,
                            size: meta.size,
                            record_count: current_records,
                            partition: partition.clone(),
                        });
                    }
                    current_records = 0;
                    current_bytes_estimate = 0;
                }

                // Create new writer if needed
                if current_writer.is_none() {
                    let unique_id = generate_unique_id();
                    let filename = format!("compact-{}-{}.parquet", unique_id, file_counter);
                    let output_path = if group.partition_key.is_empty() {
                        data_dir.join(filename)
                    } else {
                        data_dir.join(&group.partition_key).join(filename)
                    };
                    file_counter += 1;

                    let output_object_path =
                        self.path_to_object_path(&output_path.to_string_lossy(), table_base)?;

                    let writer_obj = parquet::arrow::async_writer::ParquetObjectWriter::new(
                        object_store.clone(),
                        output_object_path,
                    );

                    let async_writer = AsyncArrowWriter::try_new(
                        writer_obj,
                        table_schema.clone(),
                        Some(props.clone()),
                    )
                    .map_err(|e| Error::General(format!("Failed to create async writer: {}", e)))?;

                    current_writer = Some(async_writer);
                    current_path = Some(output_path);
                }

                // Write the batch
                if let Some(ref mut writer) = current_writer {
                    current_records += coerced.num_rows() as u64;
                    current_bytes_estimate += batch_size_estimate;
                    writer
                        .write(&coerced)
                        .await
                        .map_err(|e| Error::General(format!("Failed to write batch: {}", e)))?;
                }
            }
        }

        // Finalize the last writer
        if let (Some(writer), Some(ref out_path)) = (current_writer, current_path) {
            writer
                .close()
                .await
                .map_err(|e| Error::General(format!("Failed to close writer: {}", e)))?;

            let out_object_path =
                self.path_to_object_path(&out_path.to_string_lossy(), table_base)?;
            let meta = object_store
                .head(&out_object_path)
                .await
                .map_err(|e| Error::General(format!("Failed to get file metadata: {}", e)))?;

            let file_path = out_path.to_string_lossy().to_string();
            temp_tracker.add_file(file_path.clone());
            output_files.push(DataFileInfo {
                path: file_path,
                size: meta.size,
                record_count: current_records,
                partition: partition.clone(),
            });
        }

        Ok(CompactionResult {
            added: output_files,
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

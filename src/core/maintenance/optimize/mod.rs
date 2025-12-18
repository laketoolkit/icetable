//! Optimize service for compacting small files
//!
//! This service handles file compaction for Iceberg tables
//! by using the MetadataService trait for transactional operations.
//!
//! # Architecture
//!
//! The optimization pipeline is split into two modules:
//! - `mod.rs` - Service API and orchestration
//! - `pipeline.rs` - Internal types and helper functions
//!
//! # Concurrency Model
//!
//! The optimization pipeline uses a producer-consumer pattern with async channels:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Partition Processing                         │
//! │  (buffer_unordered with configurable parallelism)               │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                   Per-Partition Pipeline                        │
//! │                                                                 │
//! │  ┌──────────┐   ┌──────────┐   ┌──────────┐                    │
//! │  │ Reader 1 │   │ Reader 2 │   │ Reader N │  (8-32 concurrent)  │
//! │  └────┬─────┘   └────┬─────┘   └────┬─────┘                    │
//! │       │              │              │                          │
//! │       └──────────────┼──────────────┘                          │
//! │                      │                                         │
//! │                      ▼                                         │
//! │            ┌─────────────────┐                                 │
//! │            │  async_channel  │  (bounded, memory-limited)      │
//! │            └────────┬────────┘                                 │
//! │                     │                                          │
//! │       ┌─────────────┼─────────────┐                            │
//! │       │             │             │                            │
//! │       ▼             ▼             ▼                            │
//! │  ┌─────────┐   ┌─────────┐   ┌─────────┐  (1-8 writers)       │
//! │  │Writer 1 │   │Writer 2 │   │Writer N │                       │
//! │  └─────────┘   └─────────┘   └─────────┘                       │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Dynamic Concurrency Tuning
//!
//! - **Read concurrency**: 8-32 based on average file size
//!   - Small files (<10KB): 32 readers
//!   - Large files (>10MB): 4 readers
//!
//! - **Write concurrency**: 1-8 based on cores and expected output
//!   - `min(cores, expected_files).clamp(1, 8)`
//!
//! - **Channel size**: Memory-aware bounded channel
//!   - With `--max-memory`: `(limit / 2) / 5MB_per_batch`
//!   - Without: `writers * readers * 4`
//!
//! # Features
//!
//! - **True streaming**: Batches are streamed directly from readers to writer
//!   without accumulating in memory
//! - **Concurrent partitions**: Uses async concurrency for I/O-bound partition
//!   processing with configurable parallelism
//! - **Target size splitting**: Splits output files when exceeding target_size
//!   to avoid OOM and produce optimally-sized files
//! - **Cloud storage support**: Works with S3, GCS, Azure through object_store

mod pipeline;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::{StreamExt, TryStreamExt, stream};
use object_store::ObjectStore;
use object_store::path::Path as ObjectPath;
use parquet::arrow::AsyncArrowWriter;
use parquet::arrow::async_reader::{ParquetObjectReader, ParquetRecordBatchStreamBuilder};
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::{WriterProperties, WriterVersion};

use super::{FileGroup, MaintenanceConfig, group_files_by_partition};
use crate::core::metadata::{
    DataFileChanges, DataFileInfo, MaintenanceResult, OperationType, TableServiceWriter,
};
use crate::core::progress::{OptionalProgress, ProgressReporter};
use crate::error::{Error, Result};
use crate::utils::core::{format_bytes, generate_unique_id};
use crate::utils::resources::get_resource_limits;

use pipeline::{
    BatchMessage, CompactionResult, calculate_optimal_subgroup_size, coerce_batch_to_schema,
    parse_partition_key, path_to_object_path, subdivide_groups,
};

/// Service for optimizing tables by compacting small files
pub struct OptimizeService {
    config: MaintenanceConfig,
    progress: OptionalProgress,
}

impl OptimizeService {
    /// Create a new optimize service with default config
    pub fn new() -> Self {
        Self {
            config: MaintenanceConfig::default(),
            progress: None,
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: MaintenanceConfig) -> Self {
        Self {
            config,
            progress: None,
        }
    }

    /// Set the progress reporter for this service
    pub fn with_progress(mut self, progress: Arc<dyn ProgressReporter>) -> Self {
        self.progress = Some(progress);
        self
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
    pub async fn execute<M: TableServiceWriter>(
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

        // Calculate totals for heuristic subgroup sizing
        let total_files_for_sizing: usize = groups_to_compact.iter().map(|g| g.files.len()).sum();
        let total_bytes_for_sizing: u64 = groups_to_compact.iter().map(|g| g.total_size).sum();
        let optimal_subgroup_size = calculate_optimal_subgroup_size(
            total_files_for_sizing,
            total_bytes_for_sizing,
            self.config.parallelism,
        );

        // Subdivide large groups for parallel processing within partitions
        let groups_to_compact = subdivide_groups(groups_to_compact, optimal_subgroup_size);

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
            details.insert("groups".to_string(), groups_to_compact.len().to_string());
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

        // Calculate total files for progress bar (per-file progress, not per-group)
        let total_input_files: usize = groups_to_compact.iter().map(|g| g.files.len()).sum();

        // Setup progress tracking using the abstract reporter
        if let Some(ref progress) = self.progress {
            progress.set_total(total_input_files as u64);
            progress.set_message("Compacting");
        }

        // Counters for progress
        let bytes_written = Arc::new(AtomicU64::new(0));

        // Process partition groups concurrently using async streams
        let concurrency = self.config.parallelism;
        let progress_for_tasks = self.progress.clone();
        let results: Vec<Result<CompactionResult>> = stream::iter(groups_to_compact.iter())
            .map(|group| {
                let schema = schema.clone();
                let data_dir = data_dir.clone();
                let object_store = object_store.clone();
                let bytes_written = bytes_written.clone();
                let progress = progress_for_tasks.clone();
                let group = group.clone();

                async move {
                    let result = self
                        .compact_group_pipeline(
                            &group,
                            &schema,
                            &data_dir,
                            &object_store,
                            &progress,
                        )
                        .await;

                    if let Ok(ref r) = result {
                        let added_bytes: u64 = r.added.iter().map(|f| f.size).sum();
                        bytes_written.fetch_add(added_bytes, Ordering::Relaxed);
                        if let Some(ref p) = progress {
                            p.set_message(&format_bytes(bytes_written.load(Ordering::Relaxed)));
                        }
                    }

                    result
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;

        if let Some(ref progress) = self.progress {
            progress.finish_with_message("done");
        }

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

    /// Compact a group of files using optimized pipeline architecture
    ///
    /// Optimizations:
    /// - **Dynamic read concurrency**: 8-32 based on average file size
    /// - **Multiple parallel writers**: Dynamic based on cores and expected output
    /// - **Optimized WriterProperties**: ZSTD level 1, large row groups
    /// - **Schema coercion per-file**: Skip coercion if schema matches
    /// - **Progress per-file**: Smooth progress bar updates
    async fn compact_group_pipeline(
        &self,
        group: &FileGroup,
        table_schema: &Arc<arrow::datatypes::Schema>,
        data_dir: &std::path::Path,
        object_store: &Arc<dyn ObjectStore>,
        progress: &OptionalProgress,
    ) -> Result<CompactionResult> {
        if group.files.is_empty() {
            return Ok(CompactionResult {
                added: Vec::new(),
                removed: Vec::new(),
            });
        }

        // Get the table base path for computing relative paths
        let data_dir_str = data_dir.to_string_lossy();
        let table_base = data_dir_str.trim_end_matches("/data").to_string();

        // Clean partition key: remove __subgroup_X suffix
        let clean_partition_key = group
            .partition_key
            .split("/__subgroup_")
            .next()
            .unwrap_or("")
            .trim_start_matches("__subgroup_")
            .to_string();

        // Parse partition from clean key (used for all output files)
        // Wrap in Arc for cheap cloning in async tasks
        let partition = Arc::new(parse_partition_key(&clean_partition_key));

        // P2: Optimized WriterProperties - ZSTD level 1 is ~3x faster with ~5% less compression
        let props = Arc::new(
            WriterProperties::builder()
                .set_compression(Compression::ZSTD(ZstdLevel::try_new(1).unwrap_or_default()))
                .set_max_row_group_size(1_000_000) // 1M rows per row group
                .set_data_page_size_limit(1024 * 1024) // 1MB pages
                .set_write_batch_size(10_000)
                .set_writer_version(WriterVersion::PARQUET_2_0)
                .set_dictionary_enabled(true)
                .build(),
        );

        // P1: Dynamic read concurrency based on average file size
        let avg_file_size = group.total_size / group.files.len().max(1) as u64;
        let read_concurrency = match avg_file_size {
            0..=10_000 => 32,            // <10KB: maximize concurrency for small files
            10_001..=100_000 => 24,      // 10-100KB
            100_001..=1_000_000 => 16,   // 100KB-1MB
            1_000_001..=10_000_000 => 8, // 1-10MB
            _ => 4,                      // >10MB: large files, limit concurrency
        };

        // P0: Calculate number of parallel writers dynamically
        let available_cores = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4);
        let expected_output_files = (group.total_size / self.config.target_size).max(1) as usize;
        let num_writers = available_cores.min(expected_output_files).clamp(1, 8); // Cap at 1-8 writers per group to avoid too much contention

        // Channel size: respect --max-memory if set, otherwise use default
        // Each batch in channel is ~5MB average, so limit channel to use at most 50% of memory limit
        let resource_limits = get_resource_limits();
        let default_channel_size = num_writers * read_concurrency * 4;
        let channel_size = if resource_limits.has_memory_limit() {
            let avg_batch_bytes = 5 * 1024 * 1024u64; // ~5MB per batch estimate
            let max_channel_bytes = resource_limits.max_memory_bytes / 2;
            let memory_limited_size = (max_channel_bytes / avg_batch_bytes) as usize;
            memory_limited_size.min(default_channel_size).max(16) // At least 16 for progress
        } else {
            default_channel_size
        };
        let (tx, rx) = async_channel::bounded::<BatchMessage>(channel_size);

        // Spawn reader tasks - multiple files read concurrently
        let files = group.files.clone();
        let object_store_for_readers = object_store.clone();
        let table_schema_for_readers = table_schema.clone();
        let progress_for_readers = progress.clone();

        let reader_handle = tokio::spawn(async move {
            stream::iter(files.into_iter())
                .map(|file| {
                    let tx = tx.clone();
                    let object_store = object_store_for_readers.clone();
                    let table_schema = table_schema_for_readers.clone();
                    let progress = progress_for_readers.clone();

                    async move {
                        // Open parquet reader
                        let path = match file
                            .path
                            .strip_prefix("s3://")
                            .and_then(|p| p.find('/').map(|i| &p[i + 1..]))
                        {
                            Some(p) => ObjectPath::from(p),
                            None => ObjectPath::from(file.path.as_str()),
                        };
                        let reader =
                            ParquetObjectReader::new(object_store, path).with_file_size(file.size);

                        let builder = match ParquetRecordBatchStreamBuilder::new(reader).await {
                            Ok(b) => b,
                            Err(e) => {
                                let _ = tx
                                    .send(BatchMessage::Error(format!(
                                        "Failed to open {}: {}",
                                        file.path, e
                                    )))
                                    .await;
                                return;
                            }
                        };

                        // Check schema once per file, not per batch
                        let file_schema = builder.schema().clone();
                        let needs_coercion = file_schema != table_schema;

                        let mut stream = match builder.build() {
                            Ok(s) => s,
                            Err(e) => {
                                let _ = tx
                                    .send(BatchMessage::Error(format!(
                                        "Failed to build stream for {}: {}",
                                        file.path, e
                                    )))
                                    .await;
                                return;
                            }
                        };

                        // Stream batches to channel
                        while let Ok(Some(batch)) = stream.try_next().await {
                            // Skip coercion if schema matches (zero-copy)
                            let output_batch = if needs_coercion {
                                match coerce_batch_to_schema(&batch, &table_schema) {
                                    Ok(b) => b,
                                    Err(e) => {
                                        let _ = tx
                                            .send(BatchMessage::Error(format!(
                                                "Schema coercion failed for {}: {}",
                                                file.path, e
                                            )))
                                            .await;
                                        return;
                                    }
                                }
                            } else {
                                batch
                            };

                            if tx.send(BatchMessage::Batch(output_batch)).await.is_err() {
                                return; // Channel closed
                            }
                        }

                        // Signal file complete (for progress tracking)
                        if let Some(ref p) = progress {
                            p.inc(1);
                        }
                    }
                })
                .buffer_unordered(read_concurrency)
                .collect::<Vec<()>>()
                .await;

            // Signal all readers are done
            for _ in 0..num_writers {
                let _ = tx.send(BatchMessage::Done).await;
            }
        });

        // P0: Spawn multiple parallel writers
        let output_files = Arc::new(tokio::sync::Mutex::new(Vec::<DataFileInfo>::new()));
        let file_counter = Arc::new(AtomicU64::new(0));
        let error_flag = Arc::new(tokio::sync::Mutex::new(None::<String>));

        let writer_handles: Vec<_> = (0..num_writers)
            .map(|writer_id| {
                let rx = rx.clone();
                let object_store = object_store.clone();
                let table_schema = table_schema.clone();
                let props = props.clone();
                let partition = partition.clone();
                let clean_partition_key = clean_partition_key.clone();
                let data_dir = data_dir.to_path_buf();
                let table_base = table_base.clone();
                let output_files = output_files.clone();
                let file_counter = file_counter.clone();
                let error_flag = error_flag.clone();
                let target_size = self.config.target_size;

                tokio::spawn(async move {
                    let mut current_writer: Option<
                        AsyncArrowWriter<parquet::arrow::async_writer::ParquetObjectWriter>,
                    > = None;
                    let mut current_path: Option<std::path::PathBuf> = None;
                    let mut current_records = 0u64;
                    let mut current_bytes_estimate = 0u64;
                    let mut local_output_files: Vec<DataFileInfo> = Vec::new();

                    loop {
                        let msg = match rx.recv().await {
                            Ok(m) => m,
                            Err(_) => break, // Channel closed
                        };

                        match msg {
                            BatchMessage::Batch(batch) => {
                                let batch_size_estimate = batch.get_array_memory_size() as u64;

                                // Check if we need to start a new file
                                if current_writer.is_some()
                                    && current_bytes_estimate > 0
                                    && current_bytes_estimate + batch_size_estimate > target_size
                                {
                                    // Finalize current writer
                                    if let (Some(writer), Some(ref out_path)) =
                                        (current_writer.take(), current_path.take())
                                    {
                                        if let Err(e) = writer.close().await {
                                            *error_flag.lock().await = Some(format!(
                                                "Writer {}: Failed to close: {}",
                                                writer_id, e
                                            ));
                                            break;
                                        }

                                        let out_object_path = match path_to_object_path(
                                            &out_path.to_string_lossy(),
                                            &table_base,
                                        ) {
                                            Ok(p) => p,
                                            Err(e) => {
                                                *error_flag.lock().await = Some(e);
                                                break;
                                            }
                                        };

                                        let meta = match object_store.head(&out_object_path).await {
                                            Ok(m) => m,
                                            Err(e) => {
                                                *error_flag.lock().await = Some(format!(
                                                    "Writer {}: Failed to get metadata: {}",
                                                    writer_id, e
                                                ));
                                                break;
                                            }
                                        };

                                        local_output_files.push(DataFileInfo {
                                            path: out_path.to_string_lossy().to_string(),
                                            size: meta.size,
                                            record_count: current_records,
                                            partition: (*partition).clone(),
                                        });
                                    }
                                    current_records = 0;
                                    current_bytes_estimate = 0;
                                }

                                // Create new writer if needed
                                if current_writer.is_none() {
                                    let file_num = file_counter.fetch_add(1, Ordering::Relaxed);
                                    let unique_id = generate_unique_id();
                                    let filename =
                                        format!("compact-{}-{}.parquet", unique_id, file_num);
                                    let output_path = if clean_partition_key.is_empty() {
                                        data_dir.join(&filename)
                                    } else {
                                        data_dir.join(&clean_partition_key).join(&filename)
                                    };

                                    let output_object_path = match path_to_object_path(
                                        &output_path.to_string_lossy(),
                                        &table_base,
                                    ) {
                                        Ok(p) => p,
                                        Err(e) => {
                                            *error_flag.lock().await = Some(e);
                                            break;
                                        }
                                    };

                                    let writer_obj =
                                        parquet::arrow::async_writer::ParquetObjectWriter::new(
                                            object_store.clone(),
                                            output_object_path,
                                        );

                                    let async_writer = match AsyncArrowWriter::try_new(
                                        writer_obj,
                                        table_schema.clone(),
                                        Some((*props).clone()),
                                    ) {
                                        Ok(w) => w,
                                        Err(e) => {
                                            *error_flag.lock().await = Some(format!(
                                                "Writer {}: Failed to create: {}",
                                                writer_id, e
                                            ));
                                            break;
                                        }
                                    };

                                    current_writer = Some(async_writer);
                                    current_path = Some(output_path);
                                }

                                // Write the batch
                                if let Some(ref mut writer) = current_writer {
                                    current_records += batch.num_rows() as u64;
                                    current_bytes_estimate += batch_size_estimate;
                                    if let Err(e) = writer.write(&batch).await {
                                        *error_flag.lock().await = Some(format!(
                                            "Writer {}: Failed to write: {}",
                                            writer_id, e
                                        ));
                                        break;
                                    }
                                }
                            }
                            BatchMessage::Done => {
                                break; // Exit writer loop
                            }
                            BatchMessage::Error(e) => {
                                *error_flag.lock().await = Some(e);
                                break;
                            }
                        }
                    }

                    // Finalize the last writer for this worker
                    if let (Some(writer), Some(ref out_path)) = (current_writer, current_path) {
                        if let Err(e) = writer.close().await {
                            *error_flag.lock().await = Some(format!(
                                "Writer {}: Failed to close final: {}",
                                writer_id, e
                            ));
                            return;
                        }

                        let out_object_path =
                            match path_to_object_path(&out_path.to_string_lossy(), &table_base) {
                                Ok(p) => p,
                                Err(e) => {
                                    *error_flag.lock().await = Some(e);
                                    return;
                                }
                            };

                        let meta = match object_store.head(&out_object_path).await {
                            Ok(m) => m,
                            Err(e) => {
                                *error_flag.lock().await = Some(format!(
                                    "Writer {}: Failed to get final metadata: {}",
                                    writer_id, e
                                ));
                                return;
                            }
                        };

                        local_output_files.push(DataFileInfo {
                            path: out_path.to_string_lossy().to_string(),
                            size: meta.size,
                            record_count: current_records,
                            partition: (*partition).clone(),
                        });
                    }

                    // Merge local output files into shared list
                    output_files.lock().await.extend(local_output_files);
                })
            })
            .collect();

        // Wait for all tasks
        let _ = reader_handle.await;
        for handle in writer_handles {
            let _ = handle.await;
        }

        // Check for errors
        if let Some(e) = error_flag.lock().await.take() {
            return Err(Error::Metadata { message: e });
        }

        let output_files = Arc::try_unwrap(output_files)
            .map(|mutex| mutex.into_inner())
            .unwrap_or_else(|arc| arc.blocking_lock().clone());

        Ok(CompactionResult {
            added: output_files,
            removed: group.files.clone(),
        })
    }
}

impl Default for OptimizeService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

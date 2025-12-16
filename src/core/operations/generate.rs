//! Synthetic data generation for Iceberg tables
//!
//! This module provides functionality to generate synthetic test data
//! and create complete, valid Iceberg tables with proper manifests and snapshots
//! for benchmarking and integration testing.
//!
//! Supports both creating new tables and appending data to existing tables.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use arrow::array::{
    ArrayRef, BooleanBuilder, Float64Builder, Int32Builder, Int64Builder, StringBuilder,
    TimestampMicrosecondBuilder,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use iceberg::Catalog;
use rayon::prelude::*;
use iceberg::arrow::FieldMatchMode;
use iceberg::io::FileIO;
use iceberg::spec::{DataFileFormat, TableMetadata};
use iceberg::table::Table;
use iceberg::transaction::Transaction;
use iceberg::writer::base_writer::data_file_writer::DataFileWriterBuilder;
use iceberg::writer::file_writer::ParquetWriterBuilder;
use iceberg::writer::file_writer::location_generator::{
    DefaultFileNameGenerator, DefaultLocationGenerator,
};
use iceberg::writer::{IcebergWriter, IcebergWriterBuilder};
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use crate::core::metadata::{
    DataFileChanges, DataFileInfo, IcebergMetadataService, TableServiceReader, TableServiceWriter, OperationType,
};
use crate::core::storage::{ObjectStoreExt, create_object_store, to_path};
use crate::error::{Error, Result};
use crate::utils::core::find_latest_metadata;
use crate::utils::track_memory_usage;

/// Context for generating a single data file (used internally for parallel generation)
struct FileGenerationContext {
    schema: Arc<Schema>,
    file_io: FileIO,
    table_metadata: Arc<TableMetadata>,
    iceberg_schema: Arc<iceberg::spec::Schema>,
    partition_spec_id: i32,
    unique_prefix: String,
    writer_props: Arc<WriterProperties>,
}

/// Configuration for data generation
#[derive(Debug, Clone)]
pub struct GenerateConfig {
    /// Base path for the table
    pub path: String,
    /// Arrow schema for the table
    pub schema: Arc<Schema>,
    /// Total number of rows to generate
    pub rows: u64,
    /// Number of data files to create
    pub files: u32,
    /// Partition columns
    pub partition_columns: Vec<String>,
    /// Random seed for reproducible generation
    pub seed: u64,
    /// Target file size in bytes
    pub target_file_size: u64,
}

/// Result of a generate operation
#[derive(Debug)]
pub struct GenerateResult {
    /// Path to the created table
    pub table_path: String,
    /// Total rows generated
    pub total_rows: u64,
    /// Number of files created
    pub files_created: u32,
    /// Total bytes written
    pub total_bytes: u64,
    /// Paths to generated data files
    pub data_files: Vec<DataFileInfo>,
    /// Path to metadata file
    pub metadata_path: String,
    /// Snapshot ID of the created snapshot
    pub snapshot_id: i64,
    /// Whether this was an append to an existing table
    pub appended: bool,
}

/// Information about an existing Iceberg table
#[derive(Debug, Clone)]
pub struct ExistingTableInfo {
    /// Path to the table
    pub path: String,
    /// Number of existing snapshots
    pub snapshot_count: usize,
    /// Total existing data files
    pub data_file_count: usize,
    /// Total existing records
    pub total_records: u64,
}

/// Progress callback for file generation
/// Called with (completed_files, total_files)
pub type ProgressCallback = Arc<dyn Fn(u32, u32) + Send + Sync>;

/// Operation for generating synthetic Iceberg tables
pub struct GenerateOperation;

impl GenerateOperation {
    /// Check if an Iceberg table exists at the given path
    ///
    /// Returns Some(ExistingTableInfo) if a valid table exists, None otherwise.
    pub async fn table_exists(path: &str) -> Option<ExistingTableInfo> {
        let storage = create_object_store(path).await.ok()?;

        // Try to find metadata file
        if find_latest_metadata(path, &storage).await.is_err() {
            return None;
        }

        // Table exists, try to load it to get stats
        let metadata_service = IcebergMetadataService::new_async(path.to_string())
            .await
            .ok()?;
        let snapshots = metadata_service.list_snapshots(None).await.ok()?;
        let data_files = metadata_service.list_data_files().await.ok()?;

        let total_records: u64 = data_files.iter().map(|f| f.record_count).sum();

        Some(ExistingTableInfo {
            path: path.to_string(),
            snapshot_count: snapshots.len(),
            data_file_count: data_files.len(),
            total_records,
        })
    }

    /// Execute the generate operation - creates a complete valid Iceberg table
    ///
    /// NOTE: This operation requires a catalog. Direct path writes are not supported.
    pub async fn execute(_config: GenerateConfig) -> Result<GenerateResult> {
        Err(Error::CatalogRequiredForWrite {
            operation: "generate".to_string(),
        })
    }

    /// Execute generate/append operation using an Iceberg catalog for commits.
    ///
    /// This is the recommended way to generate data as it properly integrates
    /// with the catalog for atomic commits.
    ///
    /// File generation is parallelized for better performance.
    ///
    /// The optional `progress` callback is called after each file is written,
    /// with (completed_files, total_files) as arguments.
    pub async fn execute_with_catalog(
        table: Table,
        catalog: &dyn Catalog,
        schema: Arc<Schema>,
        rows: u64,
        files: u32,
        seed: u64,
        progress: Option<ProgressCallback>,
    ) -> Result<GenerateResult> {
        use iceberg::transaction::ApplyTransactionAction;

        let table_location = table.metadata().location().to_string();
        let base_path = table_location.trim_end_matches('/').to_string();

        let rows_per_file = (rows / files as u64).max(1);

        // Get resources from table for writers
        let file_io = table.file_io().clone();
        let table_metadata = table.metadata().clone();
        let partition_spec_id = table_metadata.default_partition_spec_id();
        let iceberg_schema = table_metadata.current_schema().clone();

        // Generate unique prefix for file names using timestamp
        let timestamp_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time is after UNIX epoch")
            .as_nanos();
        let unique_prefix = format!("{:016x}", timestamp_nanos as u64);

        // Pre-create shared writer properties
        let writer_props = Arc::new(WriterProperties::default());

        // Create shared context for file generation (all Arc to avoid cloning)
        let ctx = Arc::new(FileGenerationContext {
            schema,
            file_io,
            table_metadata: Arc::new(table_metadata),
            iceberg_schema: Arc::new((*iceberg_schema).clone()),
            partition_spec_id,
            unique_prefix,
            writer_props,
        });

        // Producer-consumer pattern with bounded channel for backpressure:
        // - Producer (rayon): generates batches using all CPU cores
        // - Consumer (tokio): writes to S3 with high concurrency
        // Channel capacity limits memory usage (max ~64 batches in flight)

        let channel_capacity = 64;
        let (tx, rx) = tokio::sync::mpsc::channel::<(u32, RecordBatch)>(channel_capacity);

        // Spawn producer thread (CPU-bound, uses rayon internally)
        let schema_for_producer = ctx.schema.clone();
        let producer = std::thread::spawn(move || {
            // Generate file configs
            let file_configs: Vec<_> = (0..files)
                .map(|file_idx| {
                    let file_rows = if file_idx == files - 1 {
                        rows - (rows_per_file * (files - 1) as u64)
                    } else {
                        rows_per_file
                    };
                    (file_idx, file_rows, seed + file_idx as u64)
                })
                .collect();

            // Generate batches in parallel and send through channel
            file_configs.into_par_iter().for_each(|(file_idx, file_rows, file_seed)| {
                if let Ok(batch) = Self::generate_batch(&schema_for_producer, file_rows, file_seed) {
                    // blocking_send waits if channel is full (backpressure)
                    let _ = tx.blocking_send((file_idx, batch));
                }
            });
            // tx is dropped here, closing the channel
        });

        // Consumer: read from channel and write to S3
        let write_concurrency = (files as usize / 4).clamp(8, 256);
        let rx_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

        // Progress counter
        let completed = Arc::new(AtomicU32::new(0));

        let results: Vec<Result<(DataFileInfo, iceberg::spec::DataFile)>> = rx_stream
            .map(|(file_idx, batch)| {
                let ctx = ctx.clone();
                let completed = completed.clone();
                let progress = progress.clone();
                let total_files = files;
                async move {
                    let result = Self::write_batch_to_file(&ctx, file_idx, batch).await;
                    // Report progress after each file is written
                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(ref cb) = progress {
                        cb(done, total_files);
                    }
                    result
                }
            })
            .buffer_unordered(write_concurrency)
            .collect()
            .await;

        // Wait for producer to finish
        producer.join().expect("Producer thread panicked");

        // Collect results
        let mut total_bytes = 0u64;
        let mut data_files_info = Vec::new();
        let mut all_data_files = Vec::new();

        for result in results {
            let (info, data_file) = result?;
            total_bytes += info.size;
            data_files_info.push(info);
            all_data_files.push(data_file);
        }

        // Calculate snapshot summary stats manually
        // This is a workaround for iceberg-rs 0.7.0 bug where mem::take() in
        // write_added_manifest() clears added_data_files before summary() is called.
        let added_records: u64 = all_data_files.iter().map(|f| f.record_count()).sum();
        let added_data_files_count = all_data_files.len() as u32;
        let added_file_size: u64 = all_data_files.iter().map(|f| f.file_size_in_bytes()).sum();

        // Build snapshot properties with added stats only
        let mut snapshot_properties = HashMap::new();
        snapshot_properties.insert("added-records".to_string(), added_records.to_string());
        snapshot_properties.insert(
            "added-data-files".to_string(),
            added_data_files_count.to_string(),
        );
        snapshot_properties.insert("added-files-size".to_string(), added_file_size.to_string());

        // Commit using transaction API
        let tx = Transaction::new(&table);
        let action = tx
            .fast_append()
            .set_snapshot_properties(snapshot_properties)
            .add_data_files(all_data_files);

        // Apply action to transaction and commit
        let tx = action.apply(tx).map_err(|e| Error::Metadata {
            message: format!("Failed to apply transaction: {}", e),
        })?;

        let updated_table = tx.commit(catalog).await.map_err(|e| Error::Metadata {
            message: format!("Failed to commit transaction: {}", e),
        })?;

        let snapshot_id = updated_table
            .metadata()
            .current_snapshot()
            .map(|s| s.snapshot_id())
            .unwrap_or(0);

        let metadata_path = updated_table
            .metadata_location()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{}/metadata/latest.json", base_path));

        Ok(GenerateResult {
            table_path: base_path,
            total_rows: rows,
            files_created: files,
            total_bytes,
            data_files: data_files_info,
            metadata_path,
            snapshot_id,
            appended: true,
        })
    }

    /// Write a pre-generated batch to a single data file (I/O-bound)
    async fn write_batch_to_file(
        ctx: &FileGenerationContext,
        file_idx: u32,
        batch: RecordBatch,
    ) -> Result<(DataFileInfo, iceberg::spec::DataFile)> {
        // Create location generator
        let location_gen =
            DefaultLocationGenerator::new((*ctx.table_metadata).clone()).map_err(|e| {
                Error::Metadata {
                    message: format!("Failed to create location generator: {}", e),
                }
            })?;

        // Create a writer for this file with unique prefix
        let file_name_gen = DefaultFileNameGenerator::new(
            format!("{}-{:05}", ctx.unique_prefix, file_idx),
            None,
            DataFileFormat::Parquet,
        );

        let parquet_writer = ParquetWriterBuilder::new_with_match_mode(
            (*ctx.writer_props).clone(),
            ctx.iceberg_schema.clone(),
            None,
            FieldMatchMode::Name,
            ctx.file_io.clone(),
            location_gen,
            file_name_gen,
        );

        let mut writer =
            DataFileWriterBuilder::new(parquet_writer, None, ctx.partition_spec_id)
                .build()
                .await
                .map_err(|e| Error::Serialization {
                    message: format!("Failed to build data file writer: {}", e),
                })?;

        writer.write(batch).await.map_err(|e| Error::Serialization {
            message: format!("Failed to write batch: {}", e),
        })?;

        let data_files = writer.close().await.map_err(|e| Error::Serialization {
            message: format!("Failed to close writer: {}", e),
        })?;

        let df = data_files
            .into_iter()
            .next()
            .ok_or_else(|| Error::Metadata {
                message: "Writer produced no data files".to_string(),
            })?;

        let info = DataFileInfo {
            path: df.file_path().to_string(),
            size: df.file_size_in_bytes(),
            record_count: df.record_count(),
            partition: HashMap::new(),
        };

        Ok((info, df))
    }

    /// Execute append operation - adds data to an existing Iceberg table
    ///
    /// This generates new parquet files and creates a new snapshot that references
    /// the new files while preserving existing table data.
    ///
    /// File generation is parallelized for better performance.
    pub async fn execute_append(config: GenerateConfig) -> Result<GenerateResult> {
        let storage = Arc::new(create_object_store(&config.path).await?);
        let base_path = config.path.trim_end_matches('/').to_string();

        let rows_per_file = (config.rows / config.files as u64).max(1);

        // Use timestamp for unique file names to avoid conflicts with existing files
        let timestamp_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time is after UNIX epoch")
            .as_nanos() as u64;

        // Determine concurrency based on file count
        let concurrency = (config.files as usize / 10).clamp(4, 64);

        // Generate file tasks
        let file_tasks: Vec<_> = (0..config.files)
            .map(|file_idx| {
                let file_rows = if file_idx == config.files - 1 {
                    config.rows - (rows_per_file * (config.files - 1) as u64)
                } else {
                    rows_per_file
                };
                (file_idx, file_rows)
            })
            .collect();

        // Process files in parallel
        let schema = config.schema.clone();
        let seed = config.seed;
        let base_path_clone = base_path.clone();

        let results: Vec<Result<DataFileInfo>> = stream::iter(file_tasks)
            .map(|(file_idx, file_rows)| {
                let schema = schema.clone();
                let storage = storage.clone();
                let base_path = base_path_clone.clone();

                async move {
                    // Generate batch
                    let batch = Self::generate_batch(&schema, file_rows, seed + file_idx as u64)?;

                    // Use timestamp + seed + index for unique file ID
                    let file_id = format!(
                        "{:016x}",
                        timestamp_nanos
                            .wrapping_mul(1000003)
                            .wrapping_add(seed)
                            .wrapping_add(file_idx as u64)
                    );
                    let file_name = format!("{:05}-{}.parquet", file_idx, file_id);

                    // Full path for Iceberg metadata
                    let iceberg_path = format!("{}/data/{}", base_path, file_name);

                    let parquet_bytes = Self::write_parquet_bytes(&batch)?;
                    let file_size = parquet_bytes.len() as u64;

                    // Write to storage
                    storage
                        .put_bytes(&to_path(&iceberg_path), Bytes::from(parquet_bytes))
                        .await?;

                    Ok(DataFileInfo {
                        path: iceberg_path,
                        size: file_size,
                        record_count: file_rows,
                        partition: HashMap::new(),
                    })
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;

        // Collect results
        let mut data_files = Vec::new();
        let mut total_bytes = 0u64;

        for result in results {
            let info = result?;
            total_bytes += info.size;
            data_files.push(info);
        }

        // Step 2: Use IcebergMetadataService to append the new data files
        let metadata_service = IcebergMetadataService::new_async(base_path.clone()).await?;

        // Build data file changes for append
        let mut changes = DataFileChanges::new();
        changes.added = data_files.clone();

        // Create summary for the append operation
        let mut summary = HashMap::new();
        summary.insert("source".to_string(), "generate-append".to_string());

        // Write the new snapshot
        let snapshot_info = metadata_service
            .write_snapshot(changes, OperationType::Append, summary)
            .await?;

        // Get the latest metadata path after write
        let metadata_path = find_latest_metadata(&base_path, &storage)
            .await
            .unwrap_or_else(|_| format!("{}/metadata/latest.json", base_path));

        Ok(GenerateResult {
            table_path: base_path,
            total_rows: config.rows,
            files_created: config.files,
            total_bytes,
            data_files,
            metadata_path,
            snapshot_id: snapshot_info.id,
            appended: true,
        })
    }

    /// Generate a record batch with synthetic data
    pub fn generate_batch(schema: &Arc<Schema>, num_rows: u64, seed: u64) -> Result<RecordBatch> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Only track memory for large batches (>10K rows) to avoid overhead on small files
        let estimated_memory = if num_rows > 10_000 {
            let mem = Self::estimate_batch_memory(schema, num_rows);
            track_memory_usage(mem)?;
            mem
        } else {
            0
        };

        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        let mut rng_state = hasher.finish();

        let next_rand = |state: &mut u64| -> u64 {
            *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            *state
        };

        let mut columns: Vec<ArrayRef> = Vec::new();

        for field in schema.fields() {
            let array: ArrayRef = match field.data_type() {
                DataType::Int64 => {
                    let mut builder = Int64Builder::with_capacity(num_rows as usize);
                    for i in 0..num_rows {
                        if field.is_nullable() && next_rand(&mut rng_state) % 100 < 5 {
                            builder.append_null();
                        } else {
                            builder
                                .append_value(i as i64 + next_rand(&mut rng_state) as i64 % 1000);
                        }
                    }
                    Arc::new(builder.finish())
                }
                DataType::Int32 => {
                    let mut builder = Int32Builder::with_capacity(num_rows as usize);
                    for _ in 0..num_rows {
                        if field.is_nullable() && next_rand(&mut rng_state) % 100 < 5 {
                            builder.append_null();
                        } else {
                            builder.append_value((next_rand(&mut rng_state) % 100) as i32);
                        }
                    }
                    Arc::new(builder.finish())
                }
                DataType::Float64 => {
                    let mut builder = Float64Builder::with_capacity(num_rows as usize);
                    for _ in 0..num_rows {
                        if field.is_nullable() && next_rand(&mut rng_state) % 100 < 5 {
                            builder.append_null();
                        } else {
                            let val = (next_rand(&mut rng_state) % 10000) as f64 / 100.0;
                            builder.append_value(val);
                        }
                    }
                    Arc::new(builder.finish())
                }
                DataType::Utf8 => {
                    let mut builder = StringBuilder::with_capacity(num_rows as usize, 32);
                    let sample_values = Self::get_sample_strings(field.name());
                    for _ in 0..num_rows {
                        if field.is_nullable() && next_rand(&mut rng_state) % 100 < 5 {
                            builder.append_null();
                        } else {
                            let idx = next_rand(&mut rng_state) as usize % sample_values.len();
                            builder.append_value(sample_values[idx]);
                        }
                    }
                    Arc::new(builder.finish())
                }
                DataType::Boolean => {
                    let mut builder = BooleanBuilder::with_capacity(num_rows as usize);
                    for _ in 0..num_rows {
                        if field.is_nullable() && next_rand(&mut rng_state) % 100 < 5 {
                            builder.append_null();
                        } else {
                            builder.append_value(next_rand(&mut rng_state) % 2 == 0);
                        }
                    }
                    Arc::new(builder.finish())
                }
                DataType::Timestamp(TimeUnit::Microsecond, _) => {
                    let mut builder = TimestampMicrosecondBuilder::with_capacity(num_rows as usize);
                    // Base timestamp: 2024-01-01 00:00:00 UTC in microseconds
                    let base_ts: i64 = 1_704_067_200_000_000;
                    for i in 0..num_rows {
                        if field.is_nullable() && next_rand(&mut rng_state) % 100 < 5 {
                            builder.append_null();
                        } else {
                            let offset = (i as i64 * 1_000_000)
                                + (next_rand(&mut rng_state) % 31_536_000_000_000) as i64;
                            builder.append_value(base_ts + offset);
                        }
                    }
                    Arc::new(builder.finish())
                }
                _ => {
                    // Use pre-defined sample values to avoid format! allocations in hot path
                    let sample_values = ["value_a", "value_b", "value_c", "value_d", "value_e"];
                    let mut builder = StringBuilder::with_capacity(num_rows as usize, 16);
                    for _ in 0..num_rows {
                        let idx = next_rand(&mut rng_state) as usize % sample_values.len();
                        builder.append_value(sample_values[idx]);
                    }
                    Arc::new(builder.finish())
                }
            };
            columns.push(array);
        }

        let batch = RecordBatch::try_new(schema.clone(), columns)?;

        // Release memory tracking for batch generation (actual memory will be tracked by Arrow)
        crate::utils::resources::release_memory(estimated_memory);

        Ok(batch)
    }

    fn get_sample_strings(field_name: &str) -> Vec<&'static str> {
        match field_name {
            "event_type" => vec!["click", "view", "purchase", "signup", "logout"],
            "currency" => vec!["USD", "EUR", "GBP", "JPY", "CHF"],
            "status" => vec!["pending", "completed", "failed", "cancelled"],
            "country" => vec!["US", "UK", "DE", "FR", "JP", "CN", "BR", "AU"],
            "method" => vec!["GET", "POST", "PUT", "DELETE", "PATCH"],
            "path" => vec![
                "/api/users",
                "/api/orders",
                "/api/products",
                "/health",
                "/metrics",
            ],
            "location" => vec![
                "warehouse-a",
                "warehouse-b",
                "factory-1",
                "factory-2",
                "office",
            ],
            "user_agent" => vec![
                "Mozilla/5.0 Chrome",
                "Mozilla/5.0 Firefox",
                "Mozilla/5.0 Safari",
                "curl/7.0",
            ],
            "name" => vec![
                "Alice Smith",
                "Bob Johnson",
                "Carol Williams",
                "David Brown",
                "Eve Davis",
            ],
            "email" => vec![
                "alice@example.com",
                "bob@example.com",
                "carol@example.com",
                "david@example.com",
            ],
            _ => vec!["value_a", "value_b", "value_c", "value_d", "value_e"],
        }
    }

    /// Estimate memory needed for a batch
    fn estimate_batch_memory(schema: &Arc<Schema>, num_rows: u64) -> u64 {
        // Conservative estimation: 64 bytes per row per column
        // This accounts for Arrow buffers, null bitmaps, etc.
        let columns = schema.fields().len() as u64;
        num_rows * columns * 64
    }

    fn write_parquet_bytes(batch: &RecordBatch) -> Result<Vec<u8>> {
        // Track memory for parquet writing (buffer + compression)
        let estimated_memory = batch.get_array_memory_size() as u64 * 2;
        track_memory_usage(estimated_memory)?;

        let mut buf = Vec::new();

        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .set_max_row_group_size(100_000)
            .build();

        let mut writer = ArrowWriter::try_new(&mut buf, batch.schema(), Some(props))?;
        writer.write(batch)?;
        writer.close()?;

        // Release memory tracking for parquet writing
        crate::utils::resources::release_memory(estimated_memory);

        Ok(buf)
    }
}

/// Schema templates for common use cases
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SchemaTemplate {
    /// Simple events: id, timestamp, event_type, user_id, value
    Events,
    /// Financial transactions: id, timestamp, amount, currency, account_from, account_to, status
    Transactions,
    /// IoT sensor data: sensor_id, timestamp, temperature, humidity, pressure, location
    Sensors,
    /// User profiles: user_id, created_at, name, email, country, age, active
    Users,
    /// Web logs: request_id, timestamp, method, path, status_code, response_time_ms, user_agent
    WebLogs,
}

impl SchemaTemplate {
    /// Get the Arrow schema for this template
    pub fn to_schema(self) -> Schema {
        match self {
            SchemaTemplate::Events => Schema::new(vec![
                Field::new("id", DataType::Int64, false),
                Field::new(
                    "timestamp",
                    DataType::Timestamp(TimeUnit::Microsecond, None),
                    false,
                ),
                Field::new("event_type", DataType::Utf8, false),
                Field::new("user_id", DataType::Int64, true),
                Field::new("value", DataType::Float64, true),
            ]),
            SchemaTemplate::Transactions => Schema::new(vec![
                Field::new("id", DataType::Int64, false),
                Field::new(
                    "timestamp",
                    DataType::Timestamp(TimeUnit::Microsecond, None),
                    false,
                ),
                Field::new("amount", DataType::Float64, false),
                Field::new("currency", DataType::Utf8, false),
                Field::new("account_from", DataType::Utf8, false),
                Field::new("account_to", DataType::Utf8, false),
                Field::new("status", DataType::Utf8, false),
            ]),
            SchemaTemplate::Sensors => Schema::new(vec![
                Field::new("sensor_id", DataType::Utf8, false),
                Field::new(
                    "timestamp",
                    DataType::Timestamp(TimeUnit::Microsecond, None),
                    false,
                ),
                Field::new("temperature", DataType::Float64, true),
                Field::new("humidity", DataType::Float64, true),
                Field::new("pressure", DataType::Float64, true),
                Field::new("location", DataType::Utf8, true),
            ]),
            SchemaTemplate::Users => Schema::new(vec![
                Field::new("user_id", DataType::Int64, false),
                Field::new(
                    "created_at",
                    DataType::Timestamp(TimeUnit::Microsecond, None),
                    false,
                ),
                Field::new("name", DataType::Utf8, false),
                Field::new("email", DataType::Utf8, false),
                Field::new("country", DataType::Utf8, true),
                Field::new("age", DataType::Int32, true),
                Field::new("active", DataType::Boolean, false),
            ]),
            SchemaTemplate::WebLogs => Schema::new(vec![
                Field::new("request_id", DataType::Utf8, false),
                Field::new(
                    "timestamp",
                    DataType::Timestamp(TimeUnit::Microsecond, None),
                    false,
                ),
                Field::new("method", DataType::Utf8, false),
                Field::new("path", DataType::Utf8, false),
                Field::new("status_code", DataType::Int32, false),
                Field::new("response_time_ms", DataType::Int64, false),
                Field::new("user_agent", DataType::Utf8, true),
            ]),
        }
    }
}

/// Parse a schema string like "id:int,name:string,ts:timestamp"
pub fn parse_schema_string(schema_str: &str) -> Result<Schema> {
    let mut fields = Vec::new();

    for part in schema_str.split(',') {
        let part = part.trim();
        let (name, type_str) = part
            .split_once(':')
            .ok_or_else(|| Error::SchemaValidation {
                message: format!("Invalid schema format '{}'. Expected 'name:type'", part),
            })?;

        let data_type = parse_arrow_type(type_str.trim())?;
        fields.push(Field::new(name.trim(), data_type, true));
    }

    if fields.is_empty() {
        return Err(Error::SchemaValidation {
            message: "Schema must have at least one column".to_string(),
        });
    }

    Ok(Schema::new(fields))
}

/// Parse a type string to Arrow DataType
pub fn parse_arrow_type(type_str: &str) -> Result<DataType> {
    match type_str.to_lowercase().as_str() {
        "int" | "int32" | "integer" => Ok(DataType::Int32),
        "long" | "int64" | "bigint" => Ok(DataType::Int64),
        "float" | "float32" => Ok(DataType::Float32),
        "double" | "float64" => Ok(DataType::Float64),
        "string" | "utf8" | "varchar" | "text" => Ok(DataType::Utf8),
        "bool" | "boolean" => Ok(DataType::Boolean),
        "timestamp" | "datetime" => Ok(DataType::Timestamp(TimeUnit::Microsecond, None)),
        "date" => Ok(DataType::Date32),
        _ => Err(Error::UnsupportedFeature {
            feature: format!(
                "Type '{}'. Supported: int, long, float, double, string, bool, timestamp, date",
                type_str
            ),
        }),
    }
}

//! Synthetic data generation for Iceberg tables
//!
//! This module provides functionality to generate synthetic test data
//! and create complete, valid Iceberg tables with proper manifests and snapshots
//! for benchmarking and integration testing.

use std::collections::HashMap;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, BooleanBuilder, Float64Builder, Int32Builder, Int64Builder, StringBuilder,
    TimestampMicrosecondBuilder,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use bytes::Bytes;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use crate::core::metadata::IcebergSnapshotWriter;
use crate::core::storage::{Storage, create_object_store, ObjectStoreExt, to_path};
use crate::error::{Error, Result};
use crate::utils::track_memory_usage;

use iceberg::io::FileIOBuilder;
use iceberg::spec::{DataContentType, DataFileBuilder, DataFileFormat, Struct, Summary};

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
}

/// Information about a generated data file
#[derive(Debug, Clone)]
pub struct DataFileInfo {
    /// Path to the data file
    pub path: String,
    /// Size of the file in bytes
    pub file_size: u64,
    /// Number of records in the file
    pub record_count: u64,
}

/// Operation for generating synthetic Iceberg tables
pub struct GenerateOperation;

impl GenerateOperation {
    /// Execute the generate operation - creates a complete valid Iceberg table
    pub async fn execute(config: GenerateConfig) -> Result<GenerateResult> {
        let storage = create_object_store(&config.path).await?;
        let base_path = config.path.trim_end_matches('/').to_string();

        let metadata_path = format!("{}/metadata", base_path);
        let data_path = format!("{}/data", base_path);

        let rows_per_file = (config.rows / config.files as u64).max(1);
        let mut data_files = Vec::new();
        let mut total_bytes = 0u64;

        // Step 1: Generate and write parquet data files
        for file_idx in 0..config.files {
            let file_rows = if file_idx == config.files - 1 {
                config.rows - (rows_per_file * (config.files - 1) as u64)
            } else {
                rows_per_file
            };

            let batch =
                Self::generate_batch(&config.schema, file_rows, config.seed + file_idx as u64)?;

            let file_id = format!(
                "{:016x}",
                config.seed.wrapping_mul(1000003).wrapping_add(file_idx as u64)
            );
            let file_name = format!("{:05}-{}.parquet", file_idx, file_id);
            let file_path = format!("{}/{}", data_path, file_name);

            let parquet_bytes = Self::write_parquet_bytes(&batch)?;
            let file_size = parquet_bytes.len() as u64;
            total_bytes += file_size;

            storage
                .put_bytes(&to_path(&file_path), Bytes::from(parquet_bytes))
                .await?;

            data_files.push(DataFileInfo {
                path: file_path,
                file_size,
                record_count: file_rows,
            });
        }

        // Step 2: Create complete Iceberg metadata with manifest, manifest list, and snapshot
        let (metadata_file_path, snapshot_id) = Self::create_complete_iceberg_table(
            &storage,
            &base_path,
            &metadata_path,
            &config.schema,
            &data_files,
            &config.partition_columns,
        )
        .await?;

        Ok(GenerateResult {
            table_path: base_path,
            total_rows: config.rows,
            files_created: config.files,
            total_bytes,
            data_files,
            metadata_path: metadata_file_path,
            snapshot_id,
        })
    }

    /// Create a complete Iceberg table with manifest, manifest list, snapshot, and metadata
    async fn create_complete_iceberg_table(
        storage: &Storage,
        base_path: &str,
        metadata_path: &str,
        arrow_schema: &Schema,
        data_files: &[DataFileInfo],
        partition_cols: &[String],
    ) -> Result<(String, i64)> {
        use crate::core::utils::{metadata_location_filename, new_metadata_location};

        // Convert Arrow schema to Iceberg schema
        let iceberg_schema = Self::arrow_to_iceberg_schema(arrow_schema)?;

        // Build partition spec
        let partition_spec = if !partition_cols.is_empty() {
            let mut unbound_fields = Vec::new();
            for (idx, col) in partition_cols.iter().enumerate() {
                let field_id = iceberg_schema
                    .as_struct()
                    .fields()
                    .iter()
                    .find(|f| f.name == *col)
                    .map(|f| f.id)
                    .ok_or_else(|| {
                        Error::General(format!("Partition column '{}' not found in schema", col))
                    })?;

                unbound_fields.push(
                    iceberg::spec::UnboundPartitionField::builder()
                        .source_id(field_id)
                        .field_id(1000 + idx as i32)
                        .name(col.clone())
                        .transform(iceberg::spec::Transform::Identity)
                        .build(),
                );
            }
            iceberg::spec::UnboundPartitionSpec::builder()
                .with_spec_id(0)
                .add_partition_fields(unbound_fields)
                .map_err(|e| Error::General(format!("Failed to add partition fields: {}", e)))?
                .build()
                .bind(iceberg_schema.clone())
                .map_err(|e| Error::General(format!("Failed to build partition spec: {}", e)))?
        } else {
            iceberg::spec::PartitionSpec::unpartition_spec()
        };

        let sort_order = iceberg::spec::SortOrder::unsorted_order();

        // Build initial table metadata (without snapshot)
        let build_result = iceberg::spec::TableMetadataBuilder::new(
            iceberg_schema.clone(),
            partition_spec.clone(),
            sort_order,
            base_path.to_string(),
            iceberg::spec::FormatVersion::V2,
            HashMap::new(),
        )
        .map_err(|e| Error::General(format!("Failed to create metadata builder: {}", e)))?
        .build()
        .map_err(|e| Error::General(format!("Failed to build table metadata: {}", e)))?;

        let initial_metadata = build_result.metadata;

        // Generate snapshot ID and sequence number
        let snapshot_id = chrono::Utc::now().timestamp_millis();
        let sequence_number = 1i64;
        let timestamp_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time is after UNIX epoch")
            .as_nanos();

        // Create FileIO for writing manifests
        let file_io = Self::create_file_io(base_path)?;

        // Create snapshot writer
        let snapshot_writer =
            IcebergSnapshotWriter::new(base_path.to_string(), file_io, storage.clone());

        // Convert our DataFileInfo to Iceberg DataFile
        let iceberg_data_files: Vec<iceberg::spec::DataFile> = data_files
            .iter()
            .map(|df| {
                DataFileBuilder::default()
                    .content(DataContentType::Data)
                    .file_path(df.path.clone())
                    .file_format(DataFileFormat::Parquet)
                    .partition(Struct::empty())
                    .partition_spec_id(partition_spec.spec_id())
                    .record_count(df.record_count)
                    .file_size_in_bytes(df.file_size)
                    .build()
                    .map_err(|e| Error::General(format!("Failed to build DataFile: {}", e)))
            })
            .collect::<Result<Vec<_>>>()?;

        // Write manifest file
        let manifest_file = snapshot_writer
            .write_manifest(
                &iceberg_data_files,
                snapshot_id,
                sequence_number,
                &initial_metadata,
                timestamp_nanos,
            )
            .await?;

        // Write manifest list
        let manifest_list_path = snapshot_writer
            .write_manifest_list(
                manifest_file,
                snapshot_id,
                None, // No parent snapshot
                sequence_number,
                timestamp_nanos,
            )
            .await?;

        // Calculate summary statistics
        let total_records: u64 = data_files.iter().map(|f| f.record_count).sum();
        let total_size: u64 = data_files.iter().map(|f| f.file_size).sum();

        let summary = Summary {
            operation: iceberg::spec::Operation::Append,
            additional_properties: HashMap::from([
                ("added-data-files".to_string(), data_files.len().to_string()),
                ("added-records".to_string(), total_records.to_string()),
                (
                    "added-files-size".to_string(),
                    total_size.to_string(),
                ),
                ("total-records".to_string(), total_records.to_string()),
                ("total-data-files".to_string(), data_files.len().to_string()),
            ]),
        };

        // Build snapshot
        let snapshot = snapshot_writer.build_snapshot(
            snapshot_id,
            None, // No parent
            sequence_number,
            manifest_list_path,
            summary,
            initial_metadata.current_schema().schema_id(),
        );

        // Update metadata with snapshot
        let metadata_location = new_metadata_location(base_path);
        let metadata_with_snapshot = iceberg::spec::TableMetadataBuilder::new_from_metadata(
            initial_metadata,
            Some(metadata_location_filename(&metadata_location)),
        )
        .set_branch_snapshot(snapshot, iceberg::spec::MAIN_BRANCH)
        .map_err(|e| Error::General(format!("Failed to set snapshot: {}", e)))?
        .build()
        .map_err(|e| Error::General(format!("Failed to build metadata with snapshot: {}", e)))?;

        let final_metadata = metadata_with_snapshot.metadata;

        // Serialize and write final metadata
        let metadata_json = serde_json::to_string_pretty(&final_metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;

        let metadata_filename = metadata_location_filename(&metadata_location);
        let metadata_file_path = format!("{}/{}", metadata_path, metadata_filename);

        storage
            .put_bytes(&to_path(&metadata_file_path), Bytes::from(metadata_json))
            .await?;

        // Write version-hint.text
        let version_hint_path = format!("{}/version-hint.text", metadata_path);
        storage
            .put_bytes(&to_path(&version_hint_path), Bytes::from("0"))
            .await?;

        Ok((metadata_file_path, snapshot_id))
    }

    /// Create FileIO based on path scheme
    fn create_file_io(base_path: &str) -> Result<iceberg::io::FileIO> {
        let scheme = if base_path.starts_with("s3://") {
            "s3"
        } else if base_path.starts_with("gs://") {
            "gs"
        } else if base_path.starts_with("az://") || base_path.starts_with("abfs://") {
            "az"
        } else {
            "file"
        };

        let mut builder = FileIOBuilder::new(scheme);

        if scheme == "s3" {
            // For S3, pass through environment variables
            if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
                builder = builder.with_prop("s3.endpoint", endpoint);
            }
            if let Ok(region) = std::env::var("AWS_REGION") {
                builder = builder.with_prop("s3.region", region);
            }
            if let Ok(access_key) = std::env::var("AWS_ACCESS_KEY_ID") {
                builder = builder.with_prop("s3.access-key-id", access_key);
            }
            if let Ok(secret_key) = std::env::var("AWS_SECRET_ACCESS_KEY") {
                builder = builder.with_prop("s3.secret-access-key", secret_key);
            }
            if std::env::var("AWS_ALLOW_HTTP").is_ok() {
                builder = builder.with_prop("s3.allow-http", "true");
            }
        }

        builder
            .build()
            .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))
    }

    /// Generate a record batch with synthetic data
    pub fn generate_batch(schema: &Arc<Schema>, num_rows: u64, seed: u64) -> Result<RecordBatch> {
        // Track memory for batch generation
        let estimated_memory = Self::estimate_batch_memory(schema, num_rows);
        track_memory_usage(estimated_memory)?;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

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
                            builder.append_value(i as i64 + next_rand(&mut rng_state) as i64 % 1000);
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
                    let mut builder = StringBuilder::with_capacity(num_rows as usize, 16);
                    for i in 0..num_rows {
                        builder.append_value(format!("value_{}", i));
                    }
                    Arc::new(builder.finish())
                }
            };
            columns.push(array);
        }

        let batch = RecordBatch::try_new(schema.clone(), columns)
            .map_err(|e| Error::General(format!("Failed to create record batch: {}", e)))?;
        
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

        let mut writer = ArrowWriter::try_new(&mut buf, batch.schema(), Some(props))
            .map_err(|e| Error::General(format!("Failed to create parquet writer: {}", e)))?;

        writer
            .write(batch)
            .map_err(|e| Error::General(format!("Failed to write batch: {}", e)))?;

        writer
            .close()
            .map_err(|e| Error::General(format!("Failed to close parquet writer: {}", e)))?;

        // Release memory tracking for parquet writing
        crate::utils::resources::release_memory(estimated_memory);
        
        Ok(buf)
    }

    fn arrow_to_iceberg_schema(arrow_schema: &Schema) -> Result<iceberg::spec::Schema> {
        use iceberg::spec::{NestedField, PrimitiveType, Type};

        let mut fields = Vec::new();

        for (idx, field) in arrow_schema.fields().iter().enumerate() {
            let field_id = (idx + 1) as i32;
            let iceberg_type = match field.data_type() {
                DataType::Int32 => Type::Primitive(PrimitiveType::Int),
                DataType::Int64 => Type::Primitive(PrimitiveType::Long),
                DataType::Float32 => Type::Primitive(PrimitiveType::Float),
                DataType::Float64 => Type::Primitive(PrimitiveType::Double),
                DataType::Utf8 => Type::Primitive(PrimitiveType::String),
                DataType::Boolean => Type::Primitive(PrimitiveType::Boolean),
                DataType::Timestamp(_, _) => Type::Primitive(PrimitiveType::Timestamp),
                DataType::Date32 => Type::Primitive(PrimitiveType::Date),
                DataType::Binary => Type::Primitive(PrimitiveType::Binary),
                _ => Type::Primitive(PrimitiveType::String),
            };

            let nested_field = if field.is_nullable() {
                NestedField::optional(field_id, field.name(), iceberg_type)
            } else {
                NestedField::required(field_id, field.name(), iceberg_type)
            };

            fields.push(nested_field.into());
        }

        iceberg::spec::Schema::builder()
            .with_fields(fields)
            .build()
            .map_err(|e| Error::General(format!("Failed to build Iceberg schema: {}", e)))
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
        let (name, type_str) = part.split_once(':').ok_or_else(|| {
            Error::General(format!(
                "Invalid schema format '{}'. Expected 'name:type'",
                part
            ))
        })?;

        let data_type = parse_arrow_type(type_str.trim())?;
        fields.push(Field::new(name.trim(), data_type, true));
    }

    if fields.is_empty() {
        return Err(Error::General(
            "Schema must have at least one column".to_string(),
        ));
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
        _ => Err(Error::General(format!(
            "Unsupported type '{}'. Supported: int, long, float, double, string, bool, timestamp, date",
            type_str
        ))),
    }
}

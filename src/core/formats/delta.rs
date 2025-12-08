//! Delta Lake format handler implementation
//!
//! This module provides support for reading Delta Lake tables.
//! Delta Lake is an open-source storage layer that brings ACID transactions,
//! scalable metadata handling, and time travel to data lakes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::datatypes::Schema;
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use bytes::Bytes;
use deltalake::DeltaTable;
use deltalake::kernel::StructField;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use crate::core::formats::table_utils;
use crate::core::formats::traits::*;
use crate::core::storage::{ObjectStoreExt, Storage, to_path};
use crate::error::{Error, Result};

/// Handler for Delta Lake tables
pub struct DeltaHandler {
    path: PathBuf,
    storage: Storage,
    time_travel: TimeTravelOptions,
}

impl DeltaHandler {
    /// Create a new Delta Lake handler
    pub fn new(path: &Path, storage: Storage) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            storage,
            time_travel: TimeTravelOptions::default(),
        })
    }

    /// Create a new Delta Lake handler with time-travel options
    pub fn with_time_travel(
        path: &Path,
        storage: Storage,
        time_travel: TimeTravelOptions,
    ) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            storage,
            time_travel,
        })
    }

    /// Open the Delta table (with time-travel support)
    async fn open_table(&self) -> Result<DeltaTable> {
        use deltalake::DeltaTableBuilder;

        let table_uri = self.path.to_string_lossy().to_string();

        // Parse as URL
        let url = if table_uri.starts_with("s3://")
            || table_uri.starts_with("gs://")
            || table_uri.starts_with("az://")
        {
            url::Url::parse(&table_uri)
                .map_err(|e| Error::General(format!("Invalid URL: {}", e)))?
        } else {
            // Try as file path
            url::Url::from_file_path(&table_uri)
                .map_err(|_| Error::General(format!("Invalid path: {}", table_uri)))?
        };

        // Build the Delta table with time-travel options
        let mut builder = DeltaTableBuilder::from_uri(url);

        // Apply time-travel options
        if let Some(version) = self.time_travel.version {
            builder = builder.with_version(version);
        } else if let Some(ref as_of) = self.time_travel.as_of {
            builder = builder
                .with_datestring(as_of)
                .map_err(|e| Error::General(format!("Invalid timestamp '{}': {}", as_of, e)))?;
        }

        // Load the table
        let table = builder
            .load()
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        Ok(table)
    }

    /// Convert Delta schema to Arrow schema
    pub fn delta_schema_to_arrow(delta_schema: &deltalake::kernel::StructType) -> Schema {
        let fields: Vec<arrow::datatypes::Field> = delta_schema
            .fields()
            .map(|field| Self::convert_field(field))
            .collect();

        Schema::new(fields)
    }

    /// Convert a single Delta field to Arrow field
    fn convert_field(field: &StructField) -> arrow::datatypes::Field {
        use arrow::datatypes::DataType;
        use deltalake::kernel::DataType as DeltaDataType;

        let arrow_type = match field.data_type() {
            DeltaDataType::Primitive(prim) => {
                use deltalake::kernel::PrimitiveType;
                match prim {
                    PrimitiveType::String => DataType::Utf8,
                    PrimitiveType::Long => DataType::Int64,
                    PrimitiveType::Integer => DataType::Int32,
                    PrimitiveType::Short => DataType::Int16,
                    PrimitiveType::Byte => DataType::Int8,
                    PrimitiveType::Float => DataType::Float32,
                    PrimitiveType::Double => DataType::Float64,
                    PrimitiveType::Boolean => DataType::Boolean,
                    PrimitiveType::Binary => DataType::Binary,
                    PrimitiveType::Date => DataType::Date32,
                    PrimitiveType::Timestamp => {
                        DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, None)
                    }
                    PrimitiveType::TimestampNtz => {
                        DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, None)
                    }
                    _ => DataType::Utf8, // Fallback for unknown types
                }
            }
            DeltaDataType::Struct(s) => {
                let fields: Vec<arrow::datatypes::Field> =
                    s.fields().map(Self::convert_field).collect();
                DataType::Struct(fields.into())
            }
            DeltaDataType::Array(arr) => {
                let inner_field =
                    Self::convert_delta_type_to_field("item", arr.element_type(), true);
                DataType::List(Arc::new(inner_field))
            }
            DeltaDataType::Map(map) => {
                // Arrow Map type: Map<K, V>
                let key_field = Self::convert_delta_type_to_field("key", map.key_type(), false);
                let value_field = Self::convert_delta_type_to_field(
                    "value",
                    map.value_type(),
                    map.value_contains_null(),
                );
                let entries = arrow::datatypes::Field::new(
                    "entries",
                    DataType::Struct(vec![key_field, value_field].into()),
                    false,
                );
                DataType::Map(Arc::new(entries), false)
            }
            DeltaDataType::Variant(_) => {
                // Variant types are represented as JSON strings in Arrow
                DataType::Utf8
            }
        };

        arrow::datatypes::Field::new(field.name(), arrow_type, field.is_nullable())
    }

    /// Helper to convert Delta DataType to Arrow Field
    fn convert_delta_type_to_field(
        name: &str,
        data_type: &deltalake::kernel::DataType,
        nullable: bool,
    ) -> arrow::datatypes::Field {
        use arrow::datatypes::DataType;
        use deltalake::kernel::DataType as DeltaDataType;

        let arrow_type = match data_type {
            DeltaDataType::Primitive(prim) => {
                use deltalake::kernel::PrimitiveType;
                match prim {
                    PrimitiveType::String => DataType::Utf8,
                    PrimitiveType::Long => DataType::Int64,
                    PrimitiveType::Integer => DataType::Int32,
                    PrimitiveType::Short => DataType::Int16,
                    PrimitiveType::Byte => DataType::Int8,
                    PrimitiveType::Float => DataType::Float32,
                    PrimitiveType::Double => DataType::Float64,
                    PrimitiveType::Boolean => DataType::Boolean,
                    PrimitiveType::Binary => DataType::Binary,
                    PrimitiveType::Date => DataType::Date32,
                    PrimitiveType::Timestamp => {
                        DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, None)
                    }
                    PrimitiveType::TimestampNtz => {
                        DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, None)
                    }
                    _ => DataType::Utf8,
                }
            }
            DeltaDataType::Struct(s) => {
                let fields: Vec<arrow::datatypes::Field> =
                    s.fields().map(Self::convert_field).collect();
                DataType::Struct(fields.into())
            }
            DeltaDataType::Array(arr) => {
                let inner_field =
                    Self::convert_delta_type_to_field("item", arr.element_type(), true);
                DataType::List(Arc::new(inner_field))
            }
            DeltaDataType::Map(map) => {
                let key_field = Self::convert_delta_type_to_field("key", map.key_type(), false);
                let value_field = Self::convert_delta_type_to_field(
                    "value",
                    map.value_type(),
                    map.value_contains_null(),
                );
                let entries = arrow::datatypes::Field::new(
                    "entries",
                    DataType::Struct(vec![key_field, value_field].into()),
                    false,
                );
                DataType::Map(Arc::new(entries), false)
            }
            DeltaDataType::Variant(_) => {
                // Variant types are represented as JSON strings in Arrow
                DataType::Utf8
            }
        };

        arrow::datatypes::Field::new(name, arrow_type, nullable)
    }
}

#[async_trait]
impl FormatHandler for DeltaHandler {
    async fn can_handle(&self, path: &Path) -> Result<bool> {
        // Check if this is a Delta table by looking for _delta_log directory
        let table_path = path.to_string_lossy().to_string();

        // Try to check for _delta_log subdirectory
        let delta_log_path = if table_path.ends_with('/') {
            format!("{}/_delta_log", table_path.trim_end_matches('/'))
        } else {
            format!("{}/_delta_log", table_path)
        };

        // For local storage, check if _delta_log exists
        if self.storage.storage_type() == "local" {
            let delta_log_dir = Path::new(&delta_log_path);
            if delta_log_dir.exists() && delta_log_dir.is_dir() {
                return Ok(true);
            }
        }

        // For cloud storage, try to open the table
        let url = if table_path.starts_with("s3://")
            || table_path.starts_with("gs://")
            || table_path.starts_with("az://")
        {
            match url::Url::parse(&table_path) {
                Ok(u) => u,
                Err(_) => return Ok(false),
            }
        } else {
            // Convert file path to URL
            match url::Url::from_file_path(&table_path) {
                Ok(u) => u,
                Err(_) => return Ok(false),
            }
        };

        match deltalake::open_table(url).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    fn format_name(&self) -> &str {
        "Delta Lake"
    }

    async fn read_schema(&self) -> Result<Arc<Schema>> {
        let table = self.open_table().await?;

        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        let delta_schema = snapshot.schema();

        let arrow_schema = Self::delta_schema_to_arrow(&delta_schema);
        Ok(Arc::new(arrow_schema))
    }

    async fn read_metadata(&self) -> Result<FileMetadata> {
        let table = self.open_table().await?;

        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        // Get Delta table metadata
        let version = table.version();
        let file_uris = table
            .get_file_uris()
            .map_err(|e| Error::General(format!("Failed to get file URIs: {}", e)))?;
        let num_files = file_uris.count();

        let mut metadata_map = std::collections::HashMap::new();
        metadata_map.insert(
            "version".to_string(),
            version
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
        );
        metadata_map.insert("num_files".to_string(), num_files.to_string());

        // Get table properties from metadata
        let table_metadata = snapshot.metadata();

        // Add metadata fields if available
        metadata_map.insert(
            "table_name".to_string(),
            table_metadata.name().unwrap_or("").to_string(),
        );
        metadata_map.insert(
            "description".to_string(),
            table_metadata.description().unwrap_or("").to_string(),
        );

        // Add configuration entries
        for (key, value) in table_metadata.configuration() {
            metadata_map.insert(format!("config.{}", key), value.clone());
        }

        Ok(FileMetadata {
            num_rows: None, // Delta doesn't track total rows in metadata
            compressed_size: None,
            uncompressed_size: None,
            compression: None,
            format_version: version.map(|v| v.to_string()),
            created_at: None,
            metadata: metadata_map,
        })
    }

    async fn read_batch(&self, options: &ReadOptions) -> Result<RecordBatch> {
        let batches = self.read_batches(options).await?;
        let schema = self.read_schema().await?;
        table_utils::merge_batches(batches, schema)
    }

    async fn read_batches(&self, options: &ReadOptions) -> Result<Vec<RecordBatch>> {
        let table = self.open_table().await?;

        // Get the list of active Parquet files in the Delta table
        let file_uris: Vec<String> = table
            .get_file_uris()
            .map_err(|e| Error::General(format!("Failed to get file URIs: {}", e)))?
            .collect();

        if file_uris.is_empty() {
            // Empty table - return empty result with schema
            let schema = self.read_schema().await?;
            return Ok(vec![RecordBatch::new_empty(schema)]);
        }

        // Read all Parquet files directly (applying column projection at file level)
        let mut all_batches = Vec::new();

        for file_uri in file_uris {
            // Read parquet file content using storage backend
            let data: Bytes = self
                .storage
                .get_bytes_str(&file_uri)
                .await
                .map_err(|e| Error::General(format!("Failed to read parquet file: {}", e)))?;

            // Build parquet reader
            let mut builder = ParquetRecordBatchReaderBuilder::try_new(data)
                .map_err(|e| Error::General(format!("Failed to create parquet reader: {}", e)))?;

            // Apply column projection if specified
            if let Some(columns) = options.columns() {
                let arrow_schema = builder.schema().clone();
                let mut projection_indices = Vec::new();
                for col_name in columns {
                    for (idx, field) in arrow_schema.fields().iter().enumerate() {
                        if field.name() == col_name {
                            projection_indices.push(idx);
                            break;
                        }
                    }
                }
                if !projection_indices.is_empty() {
                    let mask = parquet::arrow::ProjectionMask::roots(
                        builder.parquet_schema(),
                        projection_indices,
                    );
                    builder = builder.with_projection(mask);
                }
            }

            let reader = builder
                .build()
                .map_err(|e| Error::General(format!("Failed to build parquet reader: {}", e)))?;

            // Read all batches from this file
            for batch_result in reader {
                let batch = batch_result
                    .map_err(|e| Error::General(format!("Failed to read batch: {}", e)))?;
                all_batches.push(batch);
            }
        }

        // Apply pagination across all batches using table_utils
        let offset = options.offset().unwrap_or(0);
        let limit = options.limit().unwrap_or(usize::MAX);
        table_utils::apply_batch_pagination(all_batches, offset, limit)
    }

    async fn read_statistics(&self) -> Result<Vec<ColumnStats>> {
        let schema = self.read_schema().await?;

        // Initialize stats for all columns
        let stats: Vec<ColumnStats> = schema
            .fields()
            .iter()
            .map(|field| ColumnStats {
                name: field.name().clone(),
                null_count: None,
                distinct_count: None,
                min_value: None,
                max_value: None,
                mean: None,
                std_dev: None,
            })
            .collect();

        // Delta Lake stores statistics in the transaction log
        // FUTURE: Extract statistics from Delta transaction log when needed
        // For now, we return basic stats structure

        Ok(stats)
    }

    async fn validate(&self, _quick: bool) -> Result<ValidationReport> {
        let mut report = ValidationReport::success();

        // Try to open the table
        match self.open_table().await {
            Ok(table) => {
                // Check if we can read the snapshot
                match table.snapshot() {
                    Ok(_) => {
                        // Table is valid
                    }
                    Err(e) => {
                        report.errors.push(format!("Failed to get snapshot: {}", e));
                        report.is_valid = false;
                    }
                }
            }
            Err(e) => {
                report
                    .errors
                    .push(format!("Failed to open Delta table: {}", e));
                report.is_valid = false;
            }
        }

        Ok(report)
    }

    async fn write(&self, data: Vec<RecordBatch>, _options: &WriteOptions) -> Result<()> {
        use deltalake::kernel::transaction::CommitBuilder;
        use deltalake::protocol::{DeltaOperation, SaveMode};
        use deltalake::writer::{DeltaWriter, RecordBatchWriter};

        if data.is_empty() {
            return Ok(());
        }

        // Get schema from data
        let schema = data[0].schema();

        // Open or verify the table exists
        let table = self.open_table().await?;
        let table_path = self.path.to_string_lossy().to_string();

        // Create a RecordBatchWriter
        let mut writer = RecordBatchWriter::try_new(
            &table_path,
            schema,
            None, // no partitions
            None, // no storage options for local
        )
        .map_err(|e| Error::General(format!("Failed to create Delta writer: {}", e)))?;

        // Write all batches
        for batch in data {
            writer
                .write(batch)
                .await
                .map_err(|e| Error::General(format!("Failed to write batch: {}", e)))?;
        }

        // Flush to get Add actions
        let adds = writer
            .flush()
            .await
            .map_err(|e| Error::General(format!("Failed to flush writer: {}", e)))?;

        // Convert Add actions to kernel Actions
        let actions: Vec<deltalake::kernel::Action> = adds
            .into_iter()
            .map(deltalake::kernel::Action::Add)
            .collect();

        // Commit the adds to the table
        let log_store = table.log_store();
        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        CommitBuilder::default()
            .with_actions(actions)
            .build(
                Some(snapshot),
                log_store,
                DeltaOperation::Write {
                    mode: SaveMode::Append,
                    partition_by: None,
                    predicate: None,
                },
            )
            .await
            .map_err(|e| Error::General(format!("Failed to commit write: {}", e)))?;

        Ok(())
    }

    fn has_native_statistics(&self) -> bool {
        true // Delta Lake maintains statistics in transaction log
    }
}

//! Delta Lake format handler implementation
//!
//! This module provides support for reading Delta Lake tables.
//! Delta Lake is an open-source storage layer that brings ACID transactions,
//! scalable metadata handling, and time travel to data lakes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::record_batch::RecordBatch;
use deltalake::kernel::StructField;
use deltalake::DeltaTable;

use crate::core::formats::traits::*;
use crate::core::storage::StorageBackend;
use crate::error::{Error, Result};

/// Handler for Delta Lake tables
pub struct DeltaHandler {
    path: PathBuf,
    storage: Arc<dyn StorageBackend>,
}

impl DeltaHandler {
    /// Create a new Delta Lake handler
    pub fn new(path: &Path, storage: Arc<dyn StorageBackend>) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            storage,
        })
    }

    /// Open the Delta table
    async fn open_table(&self) -> Result<DeltaTable> {
        let table_uri = self.path.to_string_lossy().to_string();

        // Parse as URL
        let url = if table_uri.starts_with("s3://") || table_uri.starts_with("gs://") || table_uri.starts_with("az://") {
            url::Url::parse(&table_uri)
                .map_err(|e| Error::General(format!("Invalid URL: {}", e)))?
        } else {
            // Try as file path
            url::Url::from_file_path(&table_uri)
                .map_err(|_| Error::General(format!("Invalid path: {}", table_uri)))?
        };

        // Open the Delta table
        let table = deltalake::open_table(url)
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        Ok(table)
    }

    /// Convert Delta schema to Arrow schema
    fn delta_schema_to_arrow(delta_schema: &deltalake::kernel::StructType) -> Schema {
        let fields: Vec<datafusion::arrow::datatypes::Field> = delta_schema
            .fields()
            .map(|field| Self::convert_field(field))
            .collect();

        Schema::new(fields)
    }

    /// Convert a single Delta field to Arrow field
    fn convert_field(field: &StructField) -> datafusion::arrow::datatypes::Field {
        use datafusion::arrow::datatypes::DataType;
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
                        DataType::Timestamp(datafusion::arrow::datatypes::TimeUnit::Microsecond, None)
                    }
                    PrimitiveType::TimestampNtz => {
                        DataType::Timestamp(datafusion::arrow::datatypes::TimeUnit::Microsecond, None)
                    }
                    _ => DataType::Utf8, // Fallback for unknown types
                }
            }
            DeltaDataType::Struct(s) => {
                let fields: Vec<datafusion::arrow::datatypes::Field> =
                    s.fields().map(Self::convert_field).collect();
                DataType::Struct(fields.into())
            }
            DeltaDataType::Array(arr) => {
                let inner_field = Self::convert_delta_type_to_field("item", arr.element_type(), true);
                DataType::List(Arc::new(inner_field))
            }
            DeltaDataType::Map(map) => {
                // Arrow Map type: Map<K, V>
                let key_field = Self::convert_delta_type_to_field("key", map.key_type(), false);
                let value_field = Self::convert_delta_type_to_field("value", map.value_type(), map.value_contains_null());
                let entries = datafusion::arrow::datatypes::Field::new(
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

        datafusion::arrow::datatypes::Field::new(field.name(), arrow_type, field.is_nullable())
    }

    /// Helper to convert Delta DataType to Arrow Field
    fn convert_delta_type_to_field(
        name: &str,
        data_type: &deltalake::kernel::DataType,
        nullable: bool,
    ) -> datafusion::arrow::datatypes::Field {
        use datafusion::arrow::datatypes::DataType;
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
                        DataType::Timestamp(datafusion::arrow::datatypes::TimeUnit::Microsecond, None)
                    }
                    PrimitiveType::TimestampNtz => {
                        DataType::Timestamp(datafusion::arrow::datatypes::TimeUnit::Microsecond, None)
                    }
                    _ => DataType::Utf8,
                }
            }
            DeltaDataType::Struct(s) => {
                let fields: Vec<datafusion::arrow::datatypes::Field> =
                    s.fields().map(Self::convert_field).collect();
                DataType::Struct(fields.into())
            }
            DeltaDataType::Array(arr) => {
                let inner_field = Self::convert_delta_type_to_field("item", arr.element_type(), true);
                DataType::List(Arc::new(inner_field))
            }
            DeltaDataType::Map(map) => {
                let key_field = Self::convert_delta_type_to_field("key", map.key_type(), false);
                let value_field = Self::convert_delta_type_to_field("value", map.value_type(), map.value_contains_null());
                let entries = datafusion::arrow::datatypes::Field::new(
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

        datafusion::arrow::datatypes::Field::new(name, arrow_type, nullable)
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
        let url = if table_path.starts_with("s3://") || table_path.starts_with("gs://") || table_path.starts_with("az://") {
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

        let snapshot = table.snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        let delta_schema = snapshot.schema();

        let arrow_schema = Self::delta_schema_to_arrow(&delta_schema);
        Ok(Arc::new(arrow_schema))
    }

    async fn read_metadata(&self) -> Result<FileMetadata> {
        let table = self.open_table().await?;

        let snapshot = table.snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        // Get Delta table metadata
        let version = table.version();
        let file_uris = table.get_file_uris()
            .map_err(|e| Error::General(format!("Failed to get file URIs: {}", e)))?;
        let num_files = file_uris.count();

        let mut metadata_map = std::collections::HashMap::new();
        metadata_map.insert("version".to_string(), version.map(|v| v.to_string()).unwrap_or_else(|| "unknown".to_string()));
        metadata_map.insert("num_files".to_string(), num_files.to_string());

        // Get table properties from metadata
        let _table_metadata = snapshot.metadata();

        // Note: In deltalake 0.29, metadata fields are private
        // We would need to use accessor methods if they exist
        // For now, we skip adding these fields
        // TODO: Check if deltalake provides accessor methods for name, description, configuration

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

        if batches.is_empty() {
            let schema = self.read_schema().await?;
            Ok(RecordBatch::new_empty(schema))
        } else if batches.len() == 1 {
            Ok(batches.into_iter().next().unwrap())
        } else {
            let schema = batches[0].schema();
            datafusion::arrow::compute::concat_batches(&schema, &batches)
                .map_err(|e| Error::Arrow(e))
        }
    }

    async fn read_batches(&self, options: &ReadOptions) -> Result<Vec<RecordBatch>> {
        let table = self.open_table().await?;

        // Get the list of active Parquet files in the Delta table
        let file_uris: Vec<String> = table.get_file_uris()
            .map_err(|e| Error::General(format!("Failed to get file URIs: {}", e)))?
            .collect();

        if file_uris.is_empty() {
            // Empty table - return empty result with schema
            let schema = self.read_schema().await?;
            return Ok(vec![RecordBatch::new_empty(schema)]);
        }

        // Read each Parquet file and combine results
        let mut all_batches = Vec::new();
        let offset = options.offset().unwrap_or(0);
        let limit = options.limit().unwrap_or(usize::MAX);
        let mut total_rows_read = 0usize;

        for file_uri in file_uris {
            if total_rows_read >= offset + limit {
                break;
            }

            // Parse the file path from the URI
            let file_path = std::path::Path::new(&file_uri);

            // Create a Parquet handler for this file
            let parquet_handler = crate::core::formats::ParquetHandler::new(
                file_path,
                self.storage.clone()
            )?;

            // Read batches from this file with adjusted offset/limit
            let file_offset = if total_rows_read < offset {
                offset - total_rows_read
            } else {
                0
            };

            let file_limit = if total_rows_read >= offset {
                limit.saturating_sub(all_batches.iter().map(|b: &RecordBatch| b.num_rows()).sum())
            } else {
                usize::MAX
            };

            let mut file_options_builder = ReadOptions::builder()
                .offset(file_offset)
                .limit(file_limit);

            if let Some(columns) = options.columns() {
                file_options_builder = file_options_builder.columns(columns.to_vec());
            }

            let file_options = file_options_builder.build();

            let file_batches = parquet_handler.read_batches(&file_options).await?;

            for batch in file_batches {
                total_rows_read += batch.num_rows();
                all_batches.push(batch);

                if all_batches.iter().map(|b: &RecordBatch| b.num_rows()).sum::<usize>() >= limit {
                    return Ok(all_batches);
                }
            }
        }

        Ok(all_batches)
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
        // For now, we return basic stats structure
        // TODO: Extract statistics from Delta transaction log if available

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
                report.errors.push(format!("Failed to open Delta table: {}", e));
                report.is_valid = false;
            }
        }

        Ok(report)
    }

    async fn write(&self, _data: Vec<RecordBatch>, _options: &WriteOptions) -> Result<()> {
        // Writing to Delta Lake requires transaction handling
        // This is more complex and would use DeltaOps::write
        Err(Error::UnsupportedFeature {
            feature: "Writing to Delta Lake not yet implemented".to_string(),
        })
    }

    fn has_native_statistics(&self) -> bool {
        true // Delta Lake maintains statistics in transaction log
    }
}

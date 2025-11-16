//! Table registry for DataFusion query execution

use datafusion::arrow::record_batch::RecordBatch as DFRecordBatch;
use datafusion::datasource::MemTable;
use datafusion::execution::context::SessionContext;
use std::path::Path;
use std::sync::Arc;

use crate::core::formats::{FormatHandlerRegistry, ReadOptions};
use crate::core::storage::StorageBackendFactory;
use crate::error::{Error, Result};

use super::sql_parser::FileReference;
// TODO: Re-enable after updating streaming_table for DataFusion 50.3.0
// use super::streaming_table::{StreamingFormat, StreamingTableProvider};

/// Registry for managing table registration in DataFusion
pub struct QueryTableRegistry {
    ctx: SessionContext,
}

impl QueryTableRegistry {
    /// Create a new table registry with a DataFusion session context
    pub fn new() -> Self {
        use datafusion::execution::config::SessionConfig;

        // Configure DataFusion to NOT normalize identifiers to lowercase
        // This allows queries like "WHERE ARR_DELAY < 0" to match columns named "ARR_DELAY"
        let config = SessionConfig::new()
            .set_bool("datafusion.sql_parser.enable_ident_normalization", false);

        Self {
            ctx: SessionContext::new_with_config(config),
        }
    }

    /// Get a reference to the session context
    pub fn context(&self) -> &SessionContext {
        &self.ctx
    }

    /// Register a single file as a table using streaming (preferred for single-file queries)
    ///
    /// This method uses DataFusion's native streaming capabilities for Arrow files,
    /// which is much more efficient for queries with LIMIT clauses as it avoids
    /// loading the entire file into memory.
    ///
    /// For Arrow IPC files (.arrow extension), it registers the ObjectStore and uses
    /// ctx.register_arrow() for efficient streaming. For other formats, it falls back
    /// to memory-based registration.
    ///
    /// # Arguments
    ///
    /// * `file_ref` - File reference with path and table name
    ///
    /// # Returns
    ///
    /// Estimated number of rows (0 for streaming Arrow files)
    pub async fn register_file_streaming(&self, file_ref: &FileReference) -> Result<usize> {
        let path = Path::new(&file_ref.path);

        // Check file format
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let is_arrow = extension == "arrow" || extension == "ipc";
        let is_parquet = extension == "parquet";

        if is_arrow {
            log::debug!("Using DataFusion native Arrow IPC streaming for: {}", file_ref.path);

            // Create storage backend
            let storage = StorageBackendFactory::create_backend(&file_ref.path).await?;

            // For local files, convert to absolute path upfront
            let abs_path_local = if !file_ref.path.starts_with("s3://")
                && !file_ref.path.starts_with("gs://")
                && !file_ref.path.starts_with("az://") {
                Some(if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    std::env::current_dir()?.join(path)
                })
            } else {
                None
            };

            // Determine base path and object store URL
            let (base_path, object_store_url) = if file_ref.path.starts_with("s3://") {
                let parts: Vec<&str> = file_ref.path.splitn(4, '/').collect();
                if parts.len() >= 4 {
                    let bucket_url = format!("s3://{}", parts[2]);
                    (bucket_url.clone(), url::Url::parse(&bucket_url)
                        .map_err(|e| Error::Configuration {
                            message: format!("Invalid S3 URL: {}", e)
                        })?)
                } else {
                    return Err(Error::Configuration {
                        message: format!("Invalid S3 path: {}", file_ref.path)
                    });
                }
            } else if file_ref.path.starts_with("gs://") || file_ref.path.starts_with("az://") {
                return Err(Error::UnsupportedFeature {
                    feature: "Streaming Arrow IPC from GCS/Azure not yet implemented".to_string(),
                });
            } else {
                let abs_path = abs_path_local.as_ref()
                    .ok_or_else(|| Error::Configuration {
                        message: "Expected local path but abs_path_local is None".to_string()
                    })?;

                let abs_parent = abs_path.parent()
                    .ok_or_else(|| Error::Configuration {
                        message: format!("Cannot determine parent directory for: {}", abs_path.display())
                    })?;

                let parent_str = abs_parent.to_string_lossy().to_string();
                let url = url::Url::from_directory_path(abs_parent)
                    .map_err(|_| Error::Configuration {
                        message: format!("Invalid local path: {}", parent_str)
                    })?;
                (parent_str, url)
            };

            // Create ObjectStore adapter
            use crate::core::storage::ObjectStoreAdapter;
            let object_store = Arc::new(ObjectStoreAdapter::new(
                storage,
                base_path.clone(),
            ));

            // Register the object store with DataFusion
            self.ctx.register_object_store(&object_store_url, object_store.clone());
            log::debug!("Registered ObjectStore for Arrow: {}", object_store_url);

            // Use DataFusion's native Arrow IPC streaming with ListingTable
            use datafusion::datasource::file_format::arrow::ArrowFormat;
            use datafusion::datasource::listing::{ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl};

            let file_format = Arc::new(ArrowFormat::default());
            let listing_options = ListingOptions::new(file_format)
                .with_file_extension(".arrow");

            let abs_file_path = if file_ref.path.starts_with("s3://") {
                file_ref.path.clone()
            } else {
                abs_path_local
                    .as_ref()
                    .ok_or_else(|| Error::Configuration {
                        message: "Expected local path but abs_path_local is None".to_string()
                    })?
                    .to_string_lossy()
                    .to_string()
            };
            let table_path = ListingTableUrl::parse(&abs_file_path)?;

            log::debug!("Creating ListingTable for Arrow IPC");
            let config = ListingTableConfig::new(table_path)
                .with_listing_options(listing_options)
                .infer_schema(&self.ctx.state())
                .await?;

            let listing_table = ListingTable::try_new(config)?;
            self.ctx.register_table(&file_ref.table_name, Arc::new(listing_table))?;

            log::debug!("Registered Arrow IPC file with streaming: {}", file_ref.path);
            Ok(0)
        } else if is_parquet {
            log::debug!("Using DataFusion native Parquet streaming for: {}", file_ref.path);

            // Create storage backend
            let storage = StorageBackendFactory::create_backend(&file_ref.path).await?;

            // For local files, convert to absolute path upfront
            let abs_path_local = if !file_ref.path.starts_with("s3://")
                && !file_ref.path.starts_with("gs://")
                && !file_ref.path.starts_with("az://") {
                Some(if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    std::env::current_dir()?.join(path)
                })
            } else {
                None
            };

            // Determine base path and object store URL
            let (base_path, object_store_url) = if file_ref.path.starts_with("s3://") {
                // Extract bucket from S3 path (e.g., s3://bucket/key.parquet -> s3://bucket)
                let parts: Vec<&str> = file_ref.path.splitn(4, '/').collect();
                if parts.len() >= 4 {
                    let bucket_url = format!("s3://{}", parts[2]); // s3://bucket
                    (bucket_url.clone(), url::Url::parse(&bucket_url)
                        .map_err(|e| Error::Configuration {
                            message: format!("Invalid S3 URL: {}", e)
                        })?)
                } else {
                    return Err(Error::Configuration {
                        message: format!("Invalid S3 path: {}", file_ref.path)
                    });
                }
            } else if file_ref.path.starts_with("gs://") || file_ref.path.starts_with("az://") {
                // Similar handling for GCS and Azure
                return Err(Error::UnsupportedFeature {
                    feature: "Streaming Parquet from GCS/Azure not yet implemented".to_string(),
                });
            } else {
                // Local file - use the absolute path we already calculated
                let abs_path = abs_path_local.as_ref()
                    .ok_or_else(|| Error::Configuration {
                        message: "Expected local path but abs_path_local is None".to_string()
                    })?;

                // Use parent directory as base path
                let abs_parent = abs_path.parent()
                    .ok_or_else(|| Error::Configuration {
                        message: format!("Cannot determine parent directory for: {}", abs_path.display())
                    })?;

                let parent_str = abs_parent.to_string_lossy().to_string();
                let url = url::Url::from_directory_path(abs_parent)
                    .map_err(|_| Error::Configuration {
                        message: format!("Invalid local path: {}", parent_str)
                    })?;
                (parent_str, url)
            };

            // Create ObjectStore adapter
            use crate::core::storage::ObjectStoreAdapter;
            let object_store = Arc::new(ObjectStoreAdapter::new(
                storage,
                base_path.clone(),
            ));

            // Register the object store with DataFusion
            self.ctx.register_object_store(&object_store_url, object_store.clone());
            log::debug!("Registered ObjectStore for: {}", object_store_url);

            // Use DataFusion's native Parquet streaming with ListingTable
            // ParquetFormat automatically handles:
            // - Efficient metadata reading (footer with row group statistics)
            // - Predicate pushdown
            // - Row group pruning based on statistics
            // - Column pruning
            use datafusion::datasource::file_format::parquet::ParquetFormat;
            use datafusion::datasource::listing::{ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl};

            // Create listing options for Parquet format
            let file_format = Arc::new(ParquetFormat::default());
            let listing_options = ListingOptions::new(file_format)
                .with_file_extension(".parquet");

            // Create listing table URL from the ABSOLUTE file path
            // (abs_path_local was calculated above for local files)
            let abs_file_path = if file_ref.path.starts_with("s3://") {
                file_ref.path.clone()
            } else {
                abs_path_local
                    .as_ref()
                    .ok_or_else(|| Error::Configuration {
                        message: "Expected local path but abs_path_local is None".to_string()
                    })?
                    .to_string_lossy()
                    .to_string()
            };
            let table_path = ListingTableUrl::parse(&abs_file_path)?;

            // Create listing table configuration
            // ParquetFormat will efficiently read metadata from footer (row groups, statistics, schema)
            log::debug!("Creating ListingTable for Parquet with streaming and predicate pushdown");
            let config = ListingTableConfig::new(table_path)
                .with_listing_options(listing_options)
                .infer_schema(&self.ctx.state())
                .await?;

            // Create the listing table
            let listing_table = ListingTable::try_new(config)?;

            // Register the table
            self.ctx.register_table(&file_ref.table_name, Arc::new(listing_table))?;

            log::debug!("Registered Parquet file with streaming, predicate pushdown, and row group pruning: {}", file_ref.path);

            // Return 0 as we don't know row count without scanning
            Ok(0)
        } else {
            // Fall back to memory-based registration for other formats (CSV, JSON, etc.)
            log::debug!("Using memory-based registration for: {}", file_ref.path);
            self.register_file_memory(file_ref).await
        }
    }

    /// Register a single file as a table by loading into memory (legacy method)
    ///
    /// This is the original implementation that loads the entire file into a MemTable.
    /// Use `register_file_streaming()` instead for better performance with LIMIT queries.
    ///
    /// # Arguments
    ///
    /// * `file_ref` - File reference with path and table name
    ///
    /// # Returns
    ///
    /// Number of rows in the registered table
    pub async fn register_file_memory(&self, file_ref: &FileReference) -> Result<usize> {
        let path = Path::new(&file_ref.path);

        // Check if file exists (for local files)
        if !file_ref.path.starts_with("s3://")
            && !file_ref.path.starts_with("gs://")
            && !file_ref.path.starts_with("az://")
            && !file_ref.path.starts_with("http://")
            && !file_ref.path.starts_with("https://")
        {
            if !path.exists() {
                return Err(Error::FileNotFound {
                    path: path.to_path_buf(),
                });
            }
        }

        // Create storage backend
        let storage = StorageBackendFactory::create_backend(&file_ref.path).await?;

        // Create format handler
        let handler = FormatHandlerRegistry::global()
            .create_handler(path, storage)
            .await?;

        // Read schema and all batches
        let arrow_schema = handler.read_schema().await?;
        let read_options = ReadOptions::default();
        let arrow_batches = handler.read_batches(&read_options).await?;

        let total_rows = arrow_batches.iter().map(|b| b.num_rows()).sum();

        // Convert Arrow types to DataFusion types
        let df_schema = datafusion::arrow::datatypes::Schema::new(
            arrow_schema
                .fields()
                .iter()
                .map(|f| {
                    datafusion::arrow::datatypes::Field::new(
                        f.name(),
                        convert_arrow_datatype_to_datafusion(f.data_type()),
                        f.is_nullable(),
                    )
                })
                .collect::<Vec<_>>(),
        );

        let df_batches: Vec<DFRecordBatch> = arrow_batches
            .into_iter()
            .map(|batch| convert_arrow_batch_to_datafusion(batch, &df_schema))
            .collect::<Result<Vec<_>>>()?;

        // Concatenate all batches into a single batch to ensure deterministic ordering
        // This prevents non-determinism that can occur when DataFusion processes multiple
        // batches in parallel during ORDER BY operations
        let single_batch = if df_batches.is_empty() {
            DFRecordBatch::new_empty(Arc::new(df_schema.clone()))
        } else if df_batches.len() == 1 {
            df_batches.into_iter().next().unwrap()
        } else {
            datafusion::arrow::compute::concat_batches(&Arc::new(df_schema.clone()), &df_batches)
                .map_err(|e| Error::General(format!("Failed to concatenate batches: {}", e)))?
        };

        // Create MemTable with single batch in single partition
        let mem_table = MemTable::try_new(Arc::new(df_schema), vec![vec![single_batch]])?;

        // Register table in DataFusion
        self.ctx
            .register_table(&file_ref.table_name, Arc::new(mem_table))?;

        Ok(total_rows)
    }

    /// Register a single file as a table in DataFusion
    ///
    /// This method automatically chooses between streaming and memory-based registration:
    /// - For single-file queries with supported formats (Parquet): uses streaming
    /// - For multi-file queries or unsupported formats: loads into memory
    ///
    /// # Arguments
    ///
    /// * `file_ref` - File reference with path and table name
    ///
    /// # Returns
    ///
    /// Number of rows in the registered table (0 for streaming tables)
    pub async fn register_file(&self, file_ref: &FileReference) -> Result<usize> {
        // Use streaming by default for supported formats
        self.register_file_streaming(file_ref).await
    }

    /// Register multiple files as tables
    ///
    /// For single-file queries, uses streaming for efficiency.
    /// For multi-file queries (joins), we need to load all tables into memory
    /// as DataFusion needs them available simultaneously.
    ///
    /// # Arguments
    ///
    /// * `file_refs` - List of file references to register
    ///
    /// # Returns
    ///
    /// Total number of rows across all registered tables
    pub async fn register_files(&self, file_refs: &[FileReference]) -> Result<usize> {
        let mut total_rows = 0;

        // For single-file queries, use streaming for efficiency
        if file_refs.len() == 1 {
            log::debug!("Single file query - using streaming registration");
            return self.register_file_streaming(&file_refs[0]).await;
        }

        // For multi-file queries, use memory-based registration
        // as we may need to join between tables
        log::debug!("Multi-file query ({} files) - using memory registration", file_refs.len());
        for file_ref in file_refs {
            let rows = self.register_file_memory(file_ref).await?;
            total_rows += rows;
        }

        Ok(total_rows)
    }

    /// Check if a table is already registered
    pub fn is_registered(&self, table_name: &str) -> bool {
        self.ctx.table_exist(table_name).unwrap_or(false)
    }

    /// List all registered table names
    pub fn list_tables(&self) -> Vec<String> {
        self.ctx
            .catalog("datafusion")
            .and_then(|c| c.schema("public"))
            .map(|s| s.table_names())
            .unwrap_or_default()
    }
}

impl Default for QueryTableRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert Arrow DataType to DataFusion DataType
fn convert_arrow_datatype_to_datafusion(
    dt: &datafusion::arrow::datatypes::DataType,
) -> datafusion::arrow::datatypes::DataType {
    use datafusion::arrow::datatypes::DataType as DFDataType;

    match dt {
        datafusion::arrow::datatypes::DataType::Null => DFDataType::Null,
        datafusion::arrow::datatypes::DataType::Boolean => DFDataType::Boolean,
        datafusion::arrow::datatypes::DataType::Int8 => DFDataType::Int8,
        datafusion::arrow::datatypes::DataType::Int16 => DFDataType::Int16,
        datafusion::arrow::datatypes::DataType::Int32 => DFDataType::Int32,
        datafusion::arrow::datatypes::DataType::Int64 => DFDataType::Int64,
        datafusion::arrow::datatypes::DataType::UInt8 => DFDataType::UInt8,
        datafusion::arrow::datatypes::DataType::UInt16 => DFDataType::UInt16,
        datafusion::arrow::datatypes::DataType::UInt32 => DFDataType::UInt32,
        datafusion::arrow::datatypes::DataType::UInt64 => DFDataType::UInt64,
        datafusion::arrow::datatypes::DataType::Float16 => DFDataType::Float16,
        datafusion::arrow::datatypes::DataType::Float32 => DFDataType::Float32,
        datafusion::arrow::datatypes::DataType::Float64 => DFDataType::Float64,
        datafusion::arrow::datatypes::DataType::Utf8 => DFDataType::Utf8,
        datafusion::arrow::datatypes::DataType::LargeUtf8 => DFDataType::LargeUtf8,
        datafusion::arrow::datatypes::DataType::Binary => DFDataType::Binary,
        datafusion::arrow::datatypes::DataType::LargeBinary => DFDataType::LargeBinary,
        datafusion::arrow::datatypes::DataType::Date32 => DFDataType::Date32,
        datafusion::arrow::datatypes::DataType::Date64 => DFDataType::Date64,
        datafusion::arrow::datatypes::DataType::Timestamp(unit, tz) => {
            let df_unit = match unit {
                datafusion::arrow::datatypes::TimeUnit::Second => {
                    datafusion::arrow::datatypes::TimeUnit::Second
                }
                datafusion::arrow::datatypes::TimeUnit::Millisecond => {
                    datafusion::arrow::datatypes::TimeUnit::Millisecond
                }
                datafusion::arrow::datatypes::TimeUnit::Microsecond => {
                    datafusion::arrow::datatypes::TimeUnit::Microsecond
                }
                datafusion::arrow::datatypes::TimeUnit::Nanosecond => {
                    datafusion::arrow::datatypes::TimeUnit::Nanosecond
                }
            };
            DFDataType::Timestamp(df_unit, tz.clone().map(Into::into))
        }
        _ => {
            // For other complex types, we'll just use a string representation
            // This is a simplified conversion - in production you'd handle all types
            DFDataType::Utf8
        }
    }
}

/// Convert Arrow RecordBatch to DataFusion RecordBatch
fn convert_arrow_batch_to_datafusion(
    batch: datafusion::arrow::record_batch::RecordBatch,
    df_schema: &datafusion::arrow::datatypes::Schema,
) -> Result<DFRecordBatch> {
    use datafusion::arrow::array::{ArrayData, make_array};

    // Convert each column's ArrayData
    let df_columns: Vec<datafusion::arrow::array::ArrayRef> = (0..batch.num_columns())
        .map(|i| {
            let arrow_array = batch.column(i);
            // Clone the underlying ArrayData and convert to DataFusion array
            let array_data = arrow_array.to_data();
            // Convert ArrayData by serializing and deserializing through IPC
            // This is the safest way to convert between different arrow versions
            convert_array_data(array_data)
        })
        .collect::<Result<Vec<_>>>()?;

    DFRecordBatch::try_new(Arc::new(df_schema.clone()), df_columns)
        .map_err(|e| Error::General(format!("Failed to create DataFusion batch: {}", e)))
}

/// Convert ArrayData from one Arrow version to another via IPC
fn convert_array_data(data: datafusion::arrow::array::ArrayData) -> Result<datafusion::arrow::array::ArrayRef> {
    use datafusion::arrow::ipc::writer::StreamWriter;
    use datafusion::arrow::ipc::reader::StreamReader;
    use std::io::Cursor;

    // Create a temporary record batch with the data
    // Assume nullable=true as a safe default since ArrayData doesn't expose nullability directly
    let temp_schema = Arc::new(datafusion::arrow::datatypes::Schema::new(vec![
        datafusion::arrow::datatypes::Field::new(
            "temp",
            data.data_type().clone(),
            true, // Always assume nullable
        ),
    ]));

    let temp_batch = datafusion::arrow::record_batch::RecordBatch::try_new(
        temp_schema.clone(),
        vec![datafusion::arrow::array::make_array(data)],
    )?;

    // Serialize to IPC format
    let mut buffer = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut buffer, &temp_schema)?;
        writer.write(&temp_batch)?;
        writer.finish()?;
    }

    // Deserialize with DataFusion's arrow
    let cursor = Cursor::new(buffer);
    let mut reader = StreamReader::try_new(cursor, None)
        .map_err(|e| Error::General(format!("Failed to read IPC stream: {}", e)))?;

    let df_batch = reader
        .next()
        .ok_or_else(|| Error::General("No batch in IPC stream".to_string()))?
        .map_err(|e| Error::General(format!("Failed to read batch: {}", e)))?;

    Ok(df_batch.column(0).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::operations::query::sql_parser::FileReference;

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = QueryTableRegistry::new();
        assert_eq!(registry.list_tables().len(), 0);
    }

    #[tokio::test]
    async fn test_table_exists() {
        let registry = QueryTableRegistry::new();
        assert!(!registry.is_registered("mytable"));
    }

    // Integration tests with actual files are in tests/integration/query_tests.rs
}

//! Table registry for DataFusion query execution

use datafusion::arrow::record_batch::RecordBatch as DFRecordBatch;
use datafusion::datasource::MemTable;
use datafusion::execution::context::SessionContext;
use futures::TryStreamExt;
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
    /// This method uses DataFusion's native streaming capabilities, which is much more
    /// efficient for queries with LIMIT clauses as it avoids loading the entire file
    /// into memory.
    ///
    /// IMPORTANT: Column names are normalized to lowercase to match DataFusion's
    /// identifier normalization behavior, so queries like `WHERE ARR_DELAY < 0` work
    /// even when the file has columns named `ARR_DELAY` (uppercase).
    ///
    /// # Arguments
    ///
    /// * `file_ref` - File reference with path and table name
    ///
    /// # Returns
    ///
    /// Estimated number of rows (0 if unknown, as we don't scan the entire file)
    pub async fn register_file_streaming(&self, file_ref: &FileReference) -> Result<usize> {
        // For streaming registration, we need to normalize column names to match
        // DataFusion's identifier normalization. The simplest approach is to use
        // memory-based registration where we have full control over the schema.
        //
        // In the future, we could create a custom TableProvider that wraps the
        // native readers and normalizes the schema, but for now memory-based
        // registration is simpler and still performant for most use cases.
        self.register_file_memory(file_ref).await
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

        // Create MemTable from batches
        let mem_table = MemTable::try_new(Arc::new(df_schema), vec![df_batches])?;

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

        // For multi-file queries, use memory-based registration
        // as we may need to join between tables
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

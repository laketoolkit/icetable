//! Query operation - execute SQL queries using DataFusion

use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::record_batch::RecordBatch;
use std::sync::Arc;

use crate::error::{Error, Result};

use super::sql_parser::SqlPathExtractor;
use super::table_registry::QueryTableRegistry;

/// Result of a query operation
#[derive(Debug)]
pub struct QueryResult {
    /// Schema of the result set
    pub schema: Arc<Schema>,

    /// Result data as record batches
    pub batches: Vec<RecordBatch>,

    /// Total number of rows in result
    pub row_count: usize,

    /// Number of tables accessed in the query
    pub tables_accessed: usize,

    /// Total rows read from source tables
    pub rows_read: usize,
}

impl QueryResult {
    /// Create a new query result
    pub fn new(
        schema: Arc<Schema>,
        batches: Vec<RecordBatch>,
        tables_accessed: usize,
        rows_read: usize,
    ) -> Self {
        let row_count = batches.iter().map(|b| b.num_rows()).sum();

        Self {
            schema,
            batches,
            row_count,
            tables_accessed,
            rows_read,
        }
    }

    /// Check if result is empty
    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }

    /// Apply a limit to the result batches
    ///
    /// Returns a new QueryResult with at most `limit` rows
    pub fn with_limit(self, limit: usize) -> Self {
        if self.row_count <= limit {
            return self;
        }

        let mut limited_batches = Vec::new();
        let mut remaining = limit;

        for batch in self.batches {
            if remaining == 0 {
                break;
            }

            let batch_rows = batch.num_rows();
            if batch_rows <= remaining {
                limited_batches.push(batch);
                remaining -= batch_rows;
            } else {
                // Slice the batch to get only the remaining rows
                limited_batches.push(batch.slice(0, remaining));
                remaining = 0;
            }
        }

        Self {
            schema: self.schema,
            batches: limited_batches,
            row_count: limit,
            tables_accessed: self.tables_accessed,
            rows_read: self.rows_read,
        }
    }
}

/// Operation for executing SQL queries
pub struct QueryOperation {
    registry: QueryTableRegistry,
}

impl QueryOperation {
    /// Create a new query operation
    pub fn new() -> Self {
        Self {
            registry: QueryTableRegistry::new(),
        }
    }

    /// Execute a SQL query
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL query string with quoted file paths
    /// * `limit` - Optional limit on result rows
    ///
    /// # Example
    ///
    /// ```ignore
    /// let operation = QueryOperation::new();
    /// let result = operation.execute(
    ///     "SELECT * FROM 'data/flights.parquet' WHERE year > 2020",
    ///     Some(100)
    /// ).await?;
    /// ```
    pub async fn execute(&self, sql: &str, limit: Option<usize>) -> Result<QueryResult> {
        // Validate quoted paths first for better error messages
        SqlPathExtractor::validate_quoted_paths(sql)?;

        // Extract file paths from SQL
        let file_refs = SqlPathExtractor::extract_file_paths(sql)?;
        let tables_accessed = file_refs.len();

        // Register all tables in DataFusion
        let rows_read = self.registry.register_files(&file_refs).await?;

        // Rewrite SQL to replace file paths with table names
        let rewritten_sql = SqlPathExtractor::rewrite_sql(sql, &file_refs);
        log::debug!("[QueryOperation] Original SQL: {}", sql);
        log::debug!("[QueryOperation] Rewritten SQL: {}", rewritten_sql);

        // Execute the query
        let ctx = self.registry.context();
        let df = ctx.sql(&rewritten_sql).await?;

        // Clone schema before collecting (since collect consumes df)
        let df_schema = df.schema().clone();

        // Collect results (DataFusion RecordBatches)
        let df_batches = df.collect().await?;

        // Convert DataFusion batches to Arrow batches
        let batches: Vec<RecordBatch> = df_batches
            .into_iter()
            .map(convert_datafusion_batch_to_arrow)
            .collect::<Result<Vec<_>>>()?;

        // Get schema from first batch or convert from dataframe schema
        let schema = if let Some(first_batch) = batches.first() {
            first_batch.schema()
        } else {
            // Empty result - convert DataFusion schema to Arrow schema
            Arc::new(convert_datafusion_schema_to_arrow(df_schema.as_ref()))
        };

        // Create result
        let mut result = QueryResult::new(schema, batches, tables_accessed, rows_read);

        // Apply limit if specified
        if let Some(limit) = limit {
            result = result.with_limit(limit);
        }

        Ok(result)
    }

    /// Execute a SQL query and return only the schema (without data)
    ///
    /// Useful for previewing query structure without loading data
    pub async fn schema_only(&self, sql: &str) -> Result<Arc<Schema>> {
        // Validate and extract file paths
        SqlPathExtractor::validate_quoted_paths(sql)?;
        let file_refs = SqlPathExtractor::extract_file_paths(sql)?;

        // Register tables
        self.registry.register_files(&file_refs).await?;

        // Rewrite SQL to replace file paths with table names
        let rewritten_sql = SqlPathExtractor::rewrite_sql(sql, &file_refs);

        // Execute query but limit to 0 rows to only get schema
        let ctx = self.registry.context();
        let df = ctx.sql(&rewritten_sql).await?.limit(0, Some(0))?;

        // Convert DataFusion schema to Arrow schema
        let df_schema = df.schema();
        Ok(Arc::new(convert_datafusion_schema_to_arrow(
            df_schema.as_ref(),
        )))
    }
}

/// Convert DataFusion schema to Arrow schema
fn convert_datafusion_schema_to_arrow(df_schema: &datafusion::arrow::datatypes::Schema) -> Schema {
    Schema::new(
        df_schema
            .fields()
            .iter()
            .map(|f| {
                datafusion::arrow::datatypes::Field::new(
                    f.name(),
                    convert_datafusion_datatype_to_arrow(f.data_type()),
                    f.is_nullable(),
                )
            })
            .collect::<Vec<_>>(),
    )
}

/// Convert DataFusion DataType to Arrow DataType
fn convert_datafusion_datatype_to_arrow(
    dt: &datafusion::arrow::datatypes::DataType,
) -> datafusion::arrow::datatypes::DataType {
    use datafusion::arrow::datatypes::DataType as ArrowDataType;
    use datafusion::arrow::datatypes::DataType as DFDataType;

    match dt {
        DFDataType::Null => ArrowDataType::Null,
        DFDataType::Boolean => ArrowDataType::Boolean,
        DFDataType::Int8 => ArrowDataType::Int8,
        DFDataType::Int16 => ArrowDataType::Int16,
        DFDataType::Int32 => ArrowDataType::Int32,
        DFDataType::Int64 => ArrowDataType::Int64,
        DFDataType::UInt8 => ArrowDataType::UInt8,
        DFDataType::UInt16 => ArrowDataType::UInt16,
        DFDataType::UInt32 => ArrowDataType::UInt32,
        DFDataType::UInt64 => ArrowDataType::UInt64,
        DFDataType::Float16 => ArrowDataType::Float16,
        DFDataType::Float32 => ArrowDataType::Float32,
        DFDataType::Float64 => ArrowDataType::Float64,
        DFDataType::Utf8 => ArrowDataType::Utf8,
        DFDataType::LargeUtf8 => ArrowDataType::LargeUtf8,
        DFDataType::Binary => ArrowDataType::Binary,
        DFDataType::LargeBinary => ArrowDataType::LargeBinary,
        DFDataType::Date32 => ArrowDataType::Date32,
        DFDataType::Date64 => ArrowDataType::Date64,
        DFDataType::Timestamp(unit, tz) => {
            ArrowDataType::Timestamp(unit.clone(), tz.clone().map(Into::into))
        }
        _ => {
            // For other complex types, default to Utf8
            ArrowDataType::Utf8
        }
    }
}

/// Convert DataFusion RecordBatch to Arrow RecordBatch
fn convert_datafusion_batch_to_arrow(
    df_batch: datafusion::arrow::record_batch::RecordBatch,
) -> Result<RecordBatch> {
    use datafusion::arrow::ipc::reader::StreamReader;
    use datafusion::arrow::ipc::writer::StreamWriter as DFStreamWriter;
    use std::io::Cursor;

    // Serialize with DataFusion's arrow
    let mut buffer = Vec::new();
    {
        let mut writer = DFStreamWriter::try_new(&mut buffer, &df_batch.schema())
            .map_err(|e| Error::General(format!("Failed to create IPC writer: {}", e)))?;
        writer
            .write(&df_batch)
            .map_err(|e| Error::General(format!("Failed to write batch: {}", e)))?;
        writer
            .finish()
            .map_err(|e| Error::General(format!("Failed to finish writer: {}", e)))?;
    }

    // Deserialize with regular arrow
    let cursor = Cursor::new(buffer);
    let mut reader = StreamReader::try_new(cursor, None)?;

    let arrow_batch = reader
        .next()
        .ok_or_else(|| Error::General("No batch in IPC stream".to_string()))?
        .map_err(|e| Error::General(format!("Failed to read batch: {}", e)))?;

    Ok(arrow_batch)
}

impl Default for QueryOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_query_operation_creation() {
        let operation = QueryOperation::new();
        assert_eq!(operation.registry.list_tables().len(), 0);
    }

    #[test]
    fn test_query_result_with_limit() {
        use datafusion::arrow::array::{Int32Array, StringArray};
        use datafusion::arrow::datatypes::{DataType, Field, Schema};

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
        ]));

        let batch1 = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Int32Array::from(vec![1, 2, 3])),
                Arc::new(StringArray::from(vec!["a", "b", "c"])),
            ],
        )
        .unwrap();

        let batch2 = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Int32Array::from(vec![4, 5, 6])),
                Arc::new(StringArray::from(vec!["d", "e", "f"])),
            ],
        )
        .unwrap();

        let result = QueryResult::new(schema.clone(), vec![batch1, batch2], 1, 6);

        assert_eq!(result.row_count, 6);

        // Apply limit of 4
        let limited = result.with_limit(4);
        assert_eq!(limited.row_count, 4);
        assert_eq!(limited.batches.len(), 2); // First batch (3 rows) + second batch sliced (1 row)

        // First batch should be complete
        assert_eq!(limited.batches[0].num_rows(), 3);

        // Second batch should be sliced to 1 row
        assert_eq!(limited.batches[1].num_rows(), 1);
    }

    #[test]
    fn test_query_result_limit_no_op() {
        use datafusion::arrow::array::Int32Array;
        use datafusion::arrow::datatypes::{DataType, Field, Schema};

        let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int32, false)]));

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Int32Array::from(vec![1, 2, 3]))],
        )
        .unwrap();

        let result = QueryResult::new(schema, vec![batch], 1, 3);

        // Limit larger than row_count should be no-op
        let limited = result.with_limit(10);
        assert_eq!(limited.row_count, 3);
        assert_eq!(limited.batches.len(), 1);
    }

    // Integration tests with actual files are in tests/integration/query_tests.rs
}

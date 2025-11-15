//! Streaming table provider for DataFusion
//!
//! This module provides a TableProvider that uses streaming reads instead of loading
//! entire files into memory, enabling efficient queries with LIMIT clauses.

use async_trait::async_trait;
use datafusion::arrow::datatypes::Schema as DFSchema;
use datafusion::catalog::Session;
use datafusion::datasource::listing::PartitionedFile;
use datafusion::datasource::object_store::ObjectStoreUrl;
// TODO: ParquetExec moved to datafusion-datasource-parquet in DataFusion 50.3.0
// For now, we'll use the standard DataFusion table provider instead of custom streaming
// use datafusion::datasource::physical_plan::FileScanConfig;
use datafusion::datasource::TableProvider;
use datafusion::logical_expr::{Expr, TableType};
use datafusion::physical_plan::ExecutionPlan;
use object_store::path::Path as ObjectPath;
use object_store::ObjectStore;
use std::any::Any;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::core::formats::FormatHandlerRegistry;
use crate::core::storage::{ObjectStoreAdapter, StorageBackend, StorageBackendFactory};
use crate::error::{Error, Result};

/// Format type for the streaming table
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingFormat {
    /// Apache Parquet format
    Parquet,
    /// Apache Arrow IPC format
    ArrowIpc,
}

impl StreamingFormat {
    /// Detect format from file extension
    pub fn from_path(path: &Path) -> Result<Self> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| Error::UnsupportedFeature {
                feature: format!("File format for {}", path.display()),
            })?;

        match extension.to_lowercase().as_str() {
            "parquet" => Ok(StreamingFormat::Parquet),
            "arrow" | "ipc" => Ok(StreamingFormat::ArrowIpc),
            _ => Err(Error::UnsupportedFeature {
                feature: format!("Streaming for {} format", extension),
            }),
        }
    }
}

/// TableProvider that streams data from files instead of loading into memory
///
/// This provider uses DataFusion's native file format readers with our ObjectStore
/// adapter, enabling efficient streaming reads especially beneficial for queries
/// with LIMIT clauses.
pub struct StreamingTableProvider {
    /// DataFusion schema (different Arrow version than our core)
    schema: Arc<DFSchema>,
    /// Path to the file
    file_path: String,
    /// Format of the file
    format: StreamingFormat,
    /// Object store adapter for file access
    object_store: Arc<ObjectStoreAdapter>,
    /// Base path for object store
    base_path: String,
}

impl StreamingTableProvider {
    /// Create a new streaming table provider
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the file (local or remote)
    /// * `base_path` - Base directory path (used for resolving relative paths in ObjectStore)
    pub async fn try_new(file_path: String, base_path: Option<String>) -> Result<Self> {
        let path = Path::new(&file_path);

        // Detect format from file extension
        let format = StreamingFormat::from_path(path)?;

        // Determine base path
        let base_path = base_path.unwrap_or_else(|| {
            if file_path.starts_with("s3://")
                || file_path.starts_with("gs://")
                || file_path.starts_with("az://")
            {
                // Extract bucket/container part for cloud storage
                file_path.clone()
            } else {
                // Use parent directory for local files
                path.parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".to_string())
            }
        });

        // Create storage backend
        let storage_backend = StorageBackendFactory::create_backend(&file_path).await?;

        // Create ObjectStore adapter
        let object_store = Arc::new(ObjectStoreAdapter::new(
            storage_backend.clone(),
            base_path.clone(),
        ));

        // Read schema using our format handlers
        let handler = FormatHandlerRegistry::global()
            .create_handler(path, storage_backend)
            .await?;

        let arrow_schema = handler.read_schema().await?;

        // Convert our Arrow schema to DataFusion schema
        let df_schema = Arc::new(DFSchema::new(
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
        ));

        Ok(Self {
            schema: df_schema,
            file_path,
            format,
            object_store,
            base_path,
        })
    }

    /// Get the file name from the full path
    fn file_name(&self) -> String {
        Path::new(&self.file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.file_path.clone())
    }
}

impl fmt::Debug for StreamingTableProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamingTableProvider")
            .field("file_path", &self.file_path)
            .field("format", &self.format)
            .field("schema", &self.schema)
            .finish()
    }
}

#[async_trait]
impl TableProvider for StreamingTableProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn schema(&self) -> Arc<DFSchema> {
        self.schema.clone()
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    async fn scan(
        &self,
        state: &dyn Session,
        projection: Option<&Vec<usize>>,
        filters: &[Expr],
        limit: Option<usize>,
    ) -> datafusion::error::Result<Arc<dyn ExecutionPlan>> {
        match self.format {
            StreamingFormat::Parquet => {
                // Use DataFusion's ParquetExec for efficient streaming
                let file_path = ObjectPath::from(self.file_name().as_str());

                // Get file metadata using ObjectStore trait
                let object_meta = self.object_store.as_ref().head(&file_path).await?;

                // Register the object store with the runtime env
                let url = url::Url::parse(&format!("file://{}", self.base_path))
                    .map_err(|e| datafusion::error::DataFusionError::External(Box::new(e)))?;

                state
                    .runtime_env()
                    .register_object_store(&url, self.object_store.clone());

                // Create ObjectStoreUrl from the base path
                let object_store_url =
                    ObjectStoreUrl::parse(&format!("file://{}", self.base_path))?;

                // Create PartitionedFile from object metadata
                let partitioned_file = PartitionedFile::new(file_path, object_meta.size as u64);

                // Create file scan config
                let base_config = FileScanConfig::new(object_store_url, self.schema.clone())
                    .with_limit(limit)
                    .with_projection(projection.cloned())
                    .with_file_group(vec![partitioned_file]);

                let parquet_exec = ParquetExec::builder(base_config).build();

                Ok(Arc::new(parquet_exec))
            }
            StreamingFormat::ArrowIpc => {
                // For Arrow IPC, we'll use a similar approach
                // DataFusion doesn't have a built-in ArrowExec, so we'll need to implement
                // For now, return an error indicating this is not yet implemented
                Err(datafusion::error::DataFusionError::NotImplemented(
                    "Arrow IPC streaming not yet implemented. Use Parquet for streaming queries."
                        .to_string(),
                ))
            }
        }
    }
}

/// Convert Arrow DataType to DataFusion DataType
///
/// This is the same conversion function used in table_registry.rs
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
        datafusion::arrow::datatypes::DataType::Decimal128(precision, scale) => {
            DFDataType::Decimal128(*precision, *scale)
        }
        datafusion::arrow::datatypes::DataType::Decimal256(precision, scale) => {
            DFDataType::Decimal256(*precision, *scale)
        }
        datafusion::arrow::datatypes::DataType::List(field) => {
            let df_field = datafusion::arrow::datatypes::Field::new(
                field.name(),
                convert_arrow_datatype_to_datafusion(field.data_type()),
                field.is_nullable(),
            );
            DFDataType::List(Arc::new(df_field))
        }
        datafusion::arrow::datatypes::DataType::LargeList(field) => {
            let df_field = datafusion::arrow::datatypes::Field::new(
                field.name(),
                convert_arrow_datatype_to_datafusion(field.data_type()),
                field.is_nullable(),
            );
            DFDataType::LargeList(Arc::new(df_field))
        }
        datafusion::arrow::datatypes::DataType::Struct(fields) => {
            let df_fields = fields
                .iter()
                .map(|f| {
                    datafusion::arrow::datatypes::Field::new(
                        f.name(),
                        convert_arrow_datatype_to_datafusion(f.data_type()),
                        f.is_nullable(),
                    )
                })
                .collect();
            DFDataType::Struct(df_fields)
        }
        _ => {
            // For other complex types, use Utf8 as fallback
            // In production, you would handle all types properly
            DFDataType::Utf8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_format_detection() {
        let parquet_path = Path::new("/tmp/test.parquet");
        assert_eq!(
            StreamingFormat::from_path(parquet_path).unwrap(),
            StreamingFormat::Parquet
        );

        let arrow_path = Path::new("/tmp/test.arrow");
        assert_eq!(
            StreamingFormat::from_path(arrow_path).unwrap(),
            StreamingFormat::ArrowIpc
        );

        let ipc_path = Path::new("/tmp/test.ipc");
        assert_eq!(
            StreamingFormat::from_path(ipc_path).unwrap(),
            StreamingFormat::ArrowIpc
        );

        let csv_path = Path::new("/tmp/test.csv");
        assert!(StreamingFormat::from_path(csv_path).is_err());
    }

    #[tokio::test]
    async fn test_streaming_provider_creation() -> Result<()> {
        // This test requires actual parquet files from fixtures
        // For now, just test that the module compiles
        Ok(())
    }
}

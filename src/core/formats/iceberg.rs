//! Iceberg format handler implementation
//!
//! This module provides support for reading Apache Iceberg tables.
//! Apache Iceberg is a high-performance table format for huge analytic datasets.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use datafusion::arrow::datatypes::{DataType, Field, Fields, Schema as ArrowSchema, TimeUnit};
use datafusion::arrow::record_batch::RecordBatch;
use iceberg::TableIdent;
use iceberg::io::FileIOBuilder;
use iceberg::table::StaticTable;

use crate::core::formats::table_utils;
use crate::core::formats::traits::*;
use crate::core::storage::StorageBackend;
use crate::error::{Error, Result};

/// Handler for Apache Iceberg tables
pub struct IcebergHandler {
    path: PathBuf,
    storage: Arc<dyn StorageBackend>,
}

impl IcebergHandler {
    /// Create a new Iceberg handler
    pub fn new(path: &Path, storage: Arc<dyn StorageBackend>) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            storage,
        })
    }

    /// Open the Iceberg table using StaticTable
    ///
    /// This uses StaticTable which loads the table directly from the metadata file
    /// without requiring a catalog.
    async fn open_table(&self) -> Result<StaticTable> {
        let table_path = self.path.to_string_lossy().to_string();

        // Check if path points to metadata.json directly or to table root
        let metadata_location = if table_path.ends_with("metadata.json") {
            table_path.clone()
        } else if table_path.contains("/metadata/") {
            // Path already contains metadata directory
            table_path.clone()
        } else {
            // Assume table root directory, look for metadata directory
            let metadata_dir = format!("{}/metadata", table_path.trim_end_matches('/'));

            // For now, we'll construct the path to version-hint.text
            // In production, we'd read this file to get the current metadata file
            format!("{}/metadata.json", metadata_dir)
        };

        // Create FileIO for reading the metadata
        let file_io = FileIOBuilder::new_fs_io()
            .build()
            .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))?;

        // Create a table identifier (just for identification purposes)
        let table_ident = TableIdent::from_strs(&["iceberg", "table"])
            .map_err(|e| Error::General(format!("Failed to create table identifier: {}", e)))?;

        // Load the static table from the metadata file
        let table =
            StaticTable::from_metadata_file(&metadata_location, table_ident, file_io.clone())
                .await
                .map_err(|e| {
                    Error::General(format!(
                        "Failed to load Iceberg table from '{}': {}",
                        metadata_location, e
                    ))
                })?;

        Ok(table)
    }

    /// Parse Iceberg table path to extract warehouse, namespace, and table name
    ///
    /// Supports multiple path formats:
    /// 1. warehouse/namespace/table_name
    /// 2. warehouse/db.schema/table_name
    /// 3. /path/to/table_name (assumes warehouse=/path/to, namespace=default, table=table_name)
    fn parse_iceberg_path(path: &str) -> Result<(String, String, String)> {
        // Remove trailing slashes
        let path = path.trim_end_matches('/');

        // If path contains /metadata/, extract the table path before it
        let table_path = if path.contains("/metadata/") {
            let parts: Vec<&str> = path.splitn(2, "/metadata/").collect();
            parts[0]
        } else {
            path
        };

        // Split path into components
        let components: Vec<&str> = table_path.split('/').filter(|s| !s.is_empty()).collect();

        if components.is_empty() {
            return Err(Error::General(
                "Invalid Iceberg path: empty path".to_string(),
            ));
        }

        // Try to parse path structure
        // Common patterns:
        // - warehouse/namespace/table
        // - warehouse/table (namespace = default)
        // - /absolute/path/to/table (warehouse = parent, namespace = default)

        let (warehouse, namespace, table_name) = if components.len() >= 3 {
            // Format: warehouse/namespace/table
            let warehouse = components[..components.len() - 2].join("/");
            let namespace = components[components.len() - 2].to_string();
            let table = components[components.len() - 1].to_string();
            (warehouse, namespace, table)
        } else if components.len() == 2 {
            // Format: warehouse/table or namespace/table
            // Assume the first part is warehouse, use default namespace
            let warehouse = components[0].to_string();
            let table = components[1].to_string();
            (warehouse, "default".to_string(), table)
        } else {
            // Single component - use it as table name with defaults
            let table = components[0].to_string();
            (".".to_string(), "default".to_string(), table)
        };

        Ok((warehouse, namespace, table_name))
    }

    /// Convert Iceberg schema to Arrow schema
    fn iceberg_schema_to_arrow(iceberg_schema: &iceberg::spec::Schema) -> Result<ArrowSchema> {
        // Get the struct representation of the schema
        let struct_type = iceberg_schema.as_struct();

        // Convert each field
        let fields: Result<Vec<Field>> = struct_type
            .fields()
            .iter()
            .map(|field| {
                // Convert the iceberg field to an arrow field
                let data_type = Self::iceberg_type_to_arrow(&field.field_type)?;
                Ok(Field::new(field.name.clone(), data_type, field.required))
            })
            .collect();

        Ok(ArrowSchema::new(Fields::from(fields?)))
    }

    /// Convert Iceberg type to Arrow DataType
    fn iceberg_type_to_arrow(iceberg_type: &iceberg::spec::Type) -> Result<DataType> {
        use iceberg::spec::PrimitiveType;

        match iceberg_type {
            iceberg::spec::Type::Primitive(prim) => match prim {
                PrimitiveType::Boolean => Ok(DataType::Boolean),
                PrimitiveType::Int => Ok(DataType::Int32),
                PrimitiveType::Long => Ok(DataType::Int64),
                PrimitiveType::Float => Ok(DataType::Float32),
                PrimitiveType::Double => Ok(DataType::Float64),
                PrimitiveType::Date => Ok(DataType::Date32),
                PrimitiveType::Time => Ok(DataType::Time64(TimeUnit::Microsecond)),
                PrimitiveType::Timestamp => Ok(DataType::Timestamp(TimeUnit::Microsecond, None)),
                PrimitiveType::Timestamptz => Ok(DataType::Timestamp(
                    TimeUnit::Microsecond,
                    Some("UTC".into()),
                )),
                PrimitiveType::TimestampNs => Ok(DataType::Timestamp(TimeUnit::Nanosecond, None)),
                PrimitiveType::TimestamptzNs => Ok(DataType::Timestamp(
                    TimeUnit::Nanosecond,
                    Some("UTC".into()),
                )),
                PrimitiveType::String => Ok(DataType::Utf8),
                PrimitiveType::Uuid => Ok(DataType::FixedSizeBinary(16)),
                PrimitiveType::Fixed(size) => Ok(DataType::FixedSizeBinary(*size as i32)),
                PrimitiveType::Binary => Ok(DataType::Binary),
                PrimitiveType::Decimal { precision, scale } => {
                    Ok(DataType::Decimal128(*precision as u8, *scale as i8))
                }
            },
            iceberg::spec::Type::Struct(_) => {
                // For complex types, we'll return a placeholder for now
                Err(Error::UnsupportedFeature {
                    feature: "Nested struct types in Iceberg not yet fully supported".to_string(),
                })
            }
            iceberg::spec::Type::List(_) => Err(Error::UnsupportedFeature {
                feature: "List types in Iceberg not yet fully supported".to_string(),
            }),
            iceberg::spec::Type::Map(_) => Err(Error::UnsupportedFeature {
                feature: "Map types in Iceberg not yet fully supported".to_string(),
            }),
        }
    }
}

#[async_trait]
impl FormatHandler for IcebergHandler {
    async fn can_handle(&self, path: &Path) -> Result<bool> {
        // Check if this is an Iceberg table by looking for metadata directory
        let table_path = path.to_string_lossy().to_string();

        // Look for metadata directory or metadata.json
        let metadata_dir = if table_path.ends_with('/') {
            format!("{}/metadata", table_path.trim_end_matches('/'))
        } else {
            format!("{}/metadata", table_path)
        };

        // For local storage, check if metadata directory exists
        if self.storage.storage_type() == "local" {
            let metadata_path = Path::new(&metadata_dir);
            if metadata_path.exists() && metadata_path.is_dir() {
                return Ok(true);
            }
        }

        // For cloud storage or uncertain cases, try to open the table
        match self.open_table().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    fn format_name(&self) -> &str {
        "Apache Iceberg"
    }

    async fn read_schema(&self) -> Result<Arc<ArrowSchema>> {
        let table = self.open_table().await?;
        let metadata = table.metadata().clone();
        let iceberg_schema = metadata.current_schema();

        let arrow_schema = Self::iceberg_schema_to_arrow(iceberg_schema)?;
        Ok(Arc::new(arrow_schema))
    }

    async fn read_metadata(&self) -> Result<FileMetadata> {
        let table = self.open_table().await?;
        let metadata = table.metadata();

        let mut metadata_map = std::collections::HashMap::new();

        // Get table UUID
        metadata_map.insert("table_uuid".to_string(), metadata.uuid().to_string());

        // Get format version
        let format_version = metadata.format_version();
        metadata_map.insert("format_version".to_string(), format_version.to_string());

        // Get current snapshot info
        if let Some(snapshot) = metadata.current_snapshot() {
            metadata_map.insert(
                "snapshot_id".to_string(),
                snapshot.snapshot_id().to_string(),
            );
            metadata_map.insert(
                "timestamp_ms".to_string(),
                snapshot.timestamp_ms().to_string(),
            );

            // snapshot.summary() returns &Summary, iterate over additional_properties
            let summary = snapshot.summary();
            for (key, value) in summary.additional_properties.iter() {
                metadata_map.insert(format!("snapshot.{}", key), value.clone());
            }
        }

        // Get table properties
        for (key, value) in metadata.properties() {
            metadata_map.insert(format!("property.{}", key), value.clone());
        }

        Ok(FileMetadata {
            num_rows: None, // Would need to parse from snapshot summary
            compressed_size: None,
            uncompressed_size: None,
            compression: None,
            format_version: Some(format_version.to_string()),
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

        // Build a table scan
        let scan_builder = table.scan();

        // Apply column selection if specified
        let scan_builder = if let Some(columns) = options.columns() {
            scan_builder.select(columns.to_vec())
        } else {
            scan_builder
        };

        // Execute the scan (build is now synchronous in 0.7)
        let scan = scan_builder
            .build()
            .map_err(|e| Error::General(format!("Failed to build Iceberg scan: {}", e)))?;

        let stream = scan
            .to_arrow()
            .await
            .map_err(|e| Error::General(format!("Failed to execute Iceberg scan: {}", e)))?;

        // Read all batches from the stream
        use futures::stream::StreamExt;
        let mut batches = Vec::new();

        let mut stream = std::pin::pin!(stream);
        while let Some(batch_result) = stream.next().await {
            let batch =
                batch_result.map_err(|e| Error::General(format!("Failed to read batch: {}", e)))?;
            batches.push(batch);
        }

        // Apply pagination using table_utils
        let offset = options.offset().unwrap_or(0);
        let limit = options.limit().unwrap_or(usize::MAX);
        table_utils::apply_batch_pagination(batches, offset, limit)
    }

    async fn read_statistics(&self) -> Result<Vec<ColumnStats>> {
        let schema = self.read_schema().await?;

        // Initialize stats for all columns
        // Iceberg stores statistics in manifest files, but extracting them
        // requires more complex logic. For now, return basic structure.
        let stats: Vec<ColumnStats> = schema
            .fields
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

        Ok(stats)
    }

    async fn validate(&self, _quick: bool) -> Result<ValidationReport> {
        let mut report = ValidationReport::success();

        // Try to open the table
        match self.open_table().await {
            Ok(table) => {
                // Check if we can read the schema
                let metadata = table.metadata();
                if metadata.current_schema().as_struct().fields().is_empty() {
                    report
                        .errors
                        .push("Iceberg table has empty schema".to_string());
                    report.is_valid = false;
                }

                // Check if table has a current snapshot
                if metadata.current_snapshot().is_none() {
                    report
                        .warnings
                        .push("Iceberg table has no current snapshot (empty table)".to_string());
                }
            }
            Err(e) => {
                report
                    .errors
                    .push(format!("Failed to open Iceberg table: {}", e));
                report.is_valid = false;
            }
        }

        Ok(report)
    }

    async fn write(&self, _data: Vec<RecordBatch>, _options: &WriteOptions) -> Result<()> {
        // Writing to Iceberg requires transaction handling and manifest management
        Err(Error::UnsupportedFeature {
            feature: "Writing to Iceberg not yet implemented".to_string(),
        })
    }

    fn has_native_statistics(&self) -> bool {
        true // Iceberg maintains statistics in manifest files
    }
}

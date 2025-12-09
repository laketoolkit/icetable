//! Iceberg table inspection
//!
//! This module provides inspection capabilities for Apache Iceberg tables,
//! including metadata extraction, statistics, layout information, and
//! orphan file detection.

mod factory;

pub use factory::IcebergInspectorFactory;

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::core::inspection::traits::{
    ColumnInfo, ColumnStatistics, FileBasedLayout, FileInfo, LayoutInfo,
    OrphanFilesInfo, PhysicalInspectOptions, PhysicalInspector, PhysicalMetadata, SchemaInfo,
    StatisticsInfo, VerbosityLevel,
};
use crate::core::storage::Storage;
use crate::core::TableLoader;
use crate::error::Result;

/// Parse a JSON value that may be stored as string, integer, or float
///
/// Iceberg writers may serialize numeric values in different formats:
/// - As strings: "1234567890" (most common)
/// - As integers: 1234567890 (serde_json i64/u64)
/// - As floats: 1234567890.0 (some writers, e.g., certain Spark/PyIceberg configs)
///
/// This function handles all three cases to ensure robust parsing.
pub(crate) fn parse_summary_value<T>(value: Option<&serde_json::Value>) -> Option<T>
where
    T: std::str::FromStr + TryFrom<i64> + TryFrom<u64>,
{
    value.and_then(|v| {
        // Try string first (most common in Iceberg)
        v.as_str()
            .and_then(|s| s.parse::<T>().ok())
            // Try integer types
            .or_else(|| v.as_i64().and_then(|n| T::try_from(n).ok()))
            .or_else(|| v.as_u64().and_then(|n| T::try_from(n).ok()))
            // Try float (some writers serialize numbers as floats)
            .or_else(|| {
                v.as_f64().and_then(|f| {
                    // Only convert if it's a whole number (no fractional part)
                    if f.fract() == 0.0 && f >= 0.0 && f <= u64::MAX as f64 {
                        // Try to convert through u64 first for unsigned types
                        T::try_from(f as u64).ok()
                    } else if f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                        // Fall back to i64 for signed types
                        T::try_from(f as i64).ok()
                    } else {
                        None
                    }
                })
            })
    })
}

/// Iceberg table inspector using TableLoader
pub struct IcebergInspector {
    path: PathBuf,
}

impl IcebergInspector {
    /// Create a new Iceberg inspector
    pub fn new(path: PathBuf, _storage: Storage) -> Self {
        Self { path }
    }

    /// Get the table path as a string
    fn table_path(&self) -> &str {
        self.path.to_str().unwrap_or("")
    }

    /// Load table using TableLoader
    async fn load_table(&self) -> Result<std::sync::Arc<iceberg::table::Table>> {
        TableLoader::load_table(self.table_path(), None).await
    }

    /// Extract file info from table using iceberg API directly
    async fn extract_file_info_from_table(
        &self,
        table: &iceberg::table::Table,
    ) -> Result<FileInfo> {
        let metadata = table.metadata();

        // Get metadata file size
        let metadata_size = self.get_metadata_size().await.unwrap_or(0);

        let mut file_metadata = HashMap::new();
        file_metadata.insert("format_version".to_string(), (metadata.format_version() as i32).to_string());
        file_metadata.insert("current_schema_id".to_string(), metadata.current_schema_id().to_string());
        file_metadata.insert("default_sort_order_id".to_string(), (metadata.default_sort_order_id() as i32).to_string());

        // Get table UUID (public in iceberg 0.7)
        file_metadata.insert("table_uuid".to_string(), metadata.uuid().to_string());

        // Get location (public in iceberg 0.7)
        file_metadata.insert("location".to_string(), metadata.location().to_string());

        // Get snapshot count
        let snapshot_count = metadata.snapshots().count();
        file_metadata.insert("snapshot_count".to_string(), snapshot_count.to_string());

        // Get current snapshot ID if available
        if let Some(current_snapshot_id) = metadata.current_snapshot_id() {
            file_metadata.insert("current_snapshot_id".to_string(), current_snapshot_id.to_string());
        }

        // Get properties (public in iceberg 0.7)
        let properties = metadata.properties();
        if !properties.is_empty() {
            for (key, value) in properties {
                file_metadata.insert(format!("property.{}", key), value.clone());
            }
        }

        // Get created_by from properties
        let created_by = properties.get("created-by").cloned();

        Ok(FileInfo {
            path: self.table_path().to_string(),
            file_size: metadata_size,
            format_version: (metadata.format_version() as i32).to_string(),
            created_by,
            metadata: file_metadata,
        })
    }

    /// Get metadata file size
    async fn get_metadata_size(&self) -> Result<u64> {
        let metadata_dir = self.path.join("metadata");
        if metadata_dir.exists() {
            // Find latest metadata file
            let mut max_version = 0;
            let mut latest_file = None;
            
            for entry in std::fs::read_dir(metadata_dir)? {
                let entry = entry?;
                let path = entry.path();
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str())
                    && let Some(version_str) = file_name.split('-').next()
                        && let Ok(version) = version_str.parse::<u64>()
                            && version > max_version {
                                max_version = version;
                                latest_file = Some(path);
                            }
            }
            
            if let Some(file_path) = latest_file {
                let metadata = std::fs::metadata(file_path)?;
                return Ok(metadata.len());
            }
        }
        
        Ok(0)
    }

    /// Extract schema from table
    fn extract_schema_from_table(&self, table: &iceberg::table::Table) -> Result<SchemaInfo> {
        let metadata = table.metadata();
        let schema = metadata.current_schema();
        
        let mut columns = Vec::new();
        
        let struct_type = schema.as_struct();
        for (index, field) in struct_type.fields().iter().enumerate() {
            columns.push(ColumnInfo {
                name: field.name.clone(),
                column_type: format!("{:?}", field.field_type),
                nullable: !field.required,
                index,
            });
        }
        
        Ok(SchemaInfo {
            num_columns: columns.len(),
            columns,
        })
    }

    /// Extract layout info from table using iceberg API directly
    async fn extract_layout_from_table(
        &self,
        table: &iceberg::table::Table,
        _options: &PhysicalInspectOptions,
    ) -> Result<LayoutInfo> {
        let metadata = table.metadata();

        let mut details = HashMap::new();
        details.insert("format_version".to_string(), (metadata.format_version() as i32).to_string());

        // Get table UUID (public in iceberg 0.7)
        details.insert("table_uuid".to_string(), metadata.uuid().to_string());

        details.insert("current_schema_id".to_string(), metadata.current_schema_id().to_string());

        // Get partition spec info (public in iceberg 0.7)
        let partition_spec = metadata.default_partition_spec();
        details.insert("partition_spec_id".to_string(), partition_spec.spec_id().to_string());

        // Get partition fields (public in iceberg 0.7 - PartitionField has public fields)
        let partition_fields = partition_spec.fields();
        if !partition_fields.is_empty() {
            let fields_display: Vec<String> = partition_fields
                .iter()
                .map(|f| format!("{}: {:?}", f.name, f.transform))
                .collect();
            details.insert("partition_fields".to_string(), fields_display.join(", "));
        }

        // Get sort order
        details.insert("default_sort_order_id".to_string(), (metadata.default_sort_order_id() as i32).to_string());

        // Get last updated timestamp (public in iceberg 0.7)
        details.insert("last_updated_ms".to_string(), metadata.last_updated_ms().to_string());

        // Get last column ID (public in iceberg 0.7)
        details.insert("last_column_id".to_string(), metadata.last_column_id().to_string());

        // For now, return basic file-based layout
        // In a real implementation, we would scan manifests to get actual file counts
        Ok(LayoutInfo::FileBased(FileBasedLayout {
            num_files: 0, // Would need to scan manifests
            total_size: 0, // Would need to scan manifests
            partitioning: None,
            details,
        }))
    }

    /// Extract statistics from table
    async fn extract_statistics_from_table(
        &self,
        table: &iceberg::table::Table,
    ) -> Result<StatisticsInfo> {
        let metadata = table.metadata();
        let current_snapshot = metadata.current_snapshot();
        
        let mut total_rows = 0;
        let mut column_stats = Vec::new();
        
        if let Some(snapshot) = current_snapshot {
            let summary = snapshot.summary();
            // Extract total rows from summary
            if let Some(added_rows) = summary.additional_properties.get("added-records") {
                let json_value = serde_json::Value::String(added_rows.clone());
                total_rows = parse_summary_value::<i64>(Some(&json_value)).unwrap_or(0);
            }
            
            // For now, create basic column stats
            // In a real implementation, we would parse manifest entries
            let schema = metadata.current_schema();
            let struct_type = schema.as_struct();
            for field in struct_type.fields() {
                column_stats.push(ColumnStatistics {
                    column_name: field.name.clone(),
                    null_count: None,
                    min_value: None,
                    max_value: None,
                    distinct_count: None,
                });
            }
        }
        
        Ok(StatisticsInfo {
            total_rows,
            compressed_size: 0, // Would need to scan files
            uncompressed_size: 0, // Would need to scan files
            column_stats,
        })
    }

    /// Detect orphan files
    async fn detect_orphan_files(
        &self,
        _options: &PhysicalInspectOptions,
    ) -> Result<Option<OrphanFilesInfo>> {
        // For now, return None - orphan detection would need manifest scanning
        // This is a complex operation that requires comparing data files with manifest entries
        Ok(None)
    }
}

#[async_trait]
impl PhysicalInspector for IcebergInspector {
    async fn extract_metadata(&self, options: &PhysicalInspectOptions) -> Result<PhysicalMetadata> {
        // Load table using TableLoader
        let table = self.load_table().await?;
        
        // Extract file info
        let file_info = self.extract_file_info_from_table(&table).await?;
        
        // Extract schema if requested
        let schema = if options.show_schema {
            Some(self.extract_schema_from_table(&table)?)
        } else {
            None
        };
        
        // Extract layout if requested
        let layout = if options.show_layout {
            Some(self.extract_layout_from_table(&table, options).await?)
        } else {
            None
        };
        
        // Extract statistics if requested
        let statistics = if options.show_stats {
            Some(self.extract_statistics_from_table(&table).await?)
        } else {
            None
        };
        
        // Detect orphan files only in verbose mode
        let orphan_files = if options.verbosity >= VerbosityLevel::Verbose {
            self.detect_orphan_files(options).await?
        } else {
            None
        };
        
        Ok(PhysicalMetadata {
            format_name: "Apache Iceberg".to_string(),
            file_info,
            schema,
            layout,
            statistics,
            orphan_files,
        })
    }

    fn format_name(&self) -> &str {
        "Apache Iceberg"
    }

    fn can_inspect(&self, path: &str) -> bool {
        // For local paths, check if metadata directory exists
        // For URLs, this is a quick heuristic - actual detection is done in factory
        let local_path = Path::new(path);
        let metadata_path = local_path.join("metadata");

        metadata_path.exists() || path.ends_with("/metadata") || path.contains("/metadata/")
    }
}
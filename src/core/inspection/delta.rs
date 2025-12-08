//! Delta Lake physical layout inspector

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::core::storage::{ObjectStoreExt, Storage, to_path};
use crate::error::{Error, Result};

use super::formatters::{format_bytes, format_number};
use super::registry::PhysicalInspectorFactory;
use super::traits::{
    ColumnInfo, FileBasedLayout, FileInfo, LayoutInfo, PhysicalInspectOptions, PhysicalInspector,
    PhysicalMetadata, SchemaInfo, StatisticsInfo,
};

#[cfg(feature = "delta")]
use deltalake::DeltaTableBuilder;
#[cfg(feature = "delta")]
use serde_json::Value as JsonValue;

/// Delta Lake physical inspector
pub struct DeltaInspector {
    path: PathBuf,
    storage: Storage,
}

impl DeltaInspector {
    /// Create a new Delta Lake inspector
    pub fn new(path: PathBuf, storage: Storage) -> Self {
        Self { path, storage }
    }

    #[cfg(feature = "delta")]
    /// Build storage options from environment variables
    fn build_storage_options() -> HashMap<String, String> {
        let mut storage_options = HashMap::new();

        if let Ok(access_key) = std::env::var("AWS_ACCESS_KEY_ID") {
            storage_options.insert("AWS_ACCESS_KEY_ID".to_string(), access_key);
        }
        if let Ok(secret_key) = std::env::var("AWS_SECRET_ACCESS_KEY") {
            storage_options.insert("AWS_SECRET_ACCESS_KEY".to_string(), secret_key);
        }
        if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
            storage_options.insert("AWS_ENDPOINT_URL".to_string(), endpoint);
            storage_options.insert("AWS_ALLOW_HTTP".to_string(), "true".to_string());
        }
        if let Ok(region) = std::env::var("AWS_REGION") {
            storage_options.insert("AWS_REGION".to_string(), region);
        }

        storage_options
    }

    #[cfg(feature = "delta")]
    /// Extract file information from Delta table
    fn extract_file_info(
        &self,
        table: &deltalake::DeltaTable,
        file_stats: &FileStats,
    ) -> Result<FileInfo> {
        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;
        let metadata = snapshot.metadata();
        let protocol = snapshot.protocol();

        let mut file_metadata = HashMap::new();
        file_metadata.insert(
            "protocol_version".to_string(),
            format!(
                "Reader: {}, Writer: {}",
                protocol.min_reader_version(),
                protocol.min_writer_version()
            ),
        );
        file_metadata.insert(
            "current_version".to_string(),
            table.version().unwrap_or(0).to_string(),
        );
        file_metadata.insert("table_id".to_string(), metadata.id().to_string());
        file_metadata.insert(
            "log_location".to_string(),
            format!("{}/_delta_log", self.path.display()),
        );

        if let Some(records) = file_stats.total_records {
            file_metadata.insert("total_rows".to_string(), format_number(records));
        }
        file_metadata.insert(
            "total_files".to_string(),
            file_stats.total_files.to_string(),
        );

        Ok(FileInfo {
            path: self.path.display().to_string(),
            file_size: file_stats.total_size as u64,
            format_version: format!(
                "Reader: {}, Writer: {}",
                protocol.min_reader_version(),
                protocol.min_writer_version()
            ),
            created_by: Some("Delta Lake".to_string()),
            metadata: file_metadata,
        })
    }

    #[cfg(feature = "delta")]
    /// Extract schema information from Delta table
    fn extract_schema(&self, table: &deltalake::DeltaTable) -> Result<SchemaInfo> {
        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;
        let schema = snapshot.schema();

        let columns = schema
            .fields()
            .enumerate()
            .map(|(idx, field)| ColumnInfo {
                name: field.name().to_string(),
                column_type: format!("{:?}", field.data_type()),
                nullable: field.is_nullable(),
                index: idx,
            })
            .collect();

        Ok(SchemaInfo {
            num_columns: schema.fields().len(),
            columns,
        })
    }

    #[cfg(feature = "delta")]
    /// Extract file-based layout information
    fn extract_layout(
        &self,
        table: &deltalake::DeltaTable,
        file_stats: &FileStats,
    ) -> Result<FileBasedLayout> {
        let snapshot = table
            .snapshot()
            .map_err(|e| Error::General(format!("Failed to get snapshot: {}", e)))?;

        let partition_columns = snapshot.metadata().partition_columns();
        let partitioning = if partition_columns.is_empty() {
            None
        } else {
            Some(partition_columns.join(", "))
        };

        let mut details = HashMap::new();
        details.insert(
            "current_version".to_string(),
            table.version().unwrap_or(0).to_string(),
        );

        if let Some(total_records) = file_stats.total_records {
            details.insert("total_rows".to_string(), format_number(total_records));
        }

        if file_stats.total_files > 0 {
            let avg_size = file_stats.total_size / file_stats.total_files as i64;
            details.insert("avg_file_size".to_string(), format_bytes(avg_size as u64));
        }

        if let Some(min) = file_stats.min_size {
            details.insert("min_file_size".to_string(), format_bytes(min as u64));
        }
        if let Some(max) = file_stats.max_size {
            details.insert("max_file_size".to_string(), format_bytes(max as u64));
        }

        Ok(FileBasedLayout {
            num_files: file_stats.total_files,
            total_size: file_stats.total_size as u64,
            partitioning,
            details,
        })
    }

    #[cfg(feature = "delta")]
    /// Extract statistics information
    fn extract_statistics(&self, file_stats: &FileStats) -> Result<StatisticsInfo> {
        let total_rows = file_stats.total_records.unwrap_or(0);
        let compressed_size = file_stats.total_size as u64;

        // Delta doesn't provide uncompressed size in the same way as Parquet,
        // so we use compressed size for both
        Ok(StatisticsInfo {
            total_rows,
            compressed_size,
            uncompressed_size: compressed_size,
            column_stats: Vec::new(), // Column stats would require parsing all Parquet files
        })
    }
}

#[async_trait]
impl PhysicalInspector for DeltaInspector {
    #[cfg(feature = "delta")]
    async fn extract_metadata(
        &self,
        _options: &PhysicalInspectOptions,
    ) -> Result<PhysicalMetadata> {
        let storage_options = Self::build_storage_options();

        // Load the Delta table
        let path_str = self
            .path
            .to_str()
            .ok_or_else(|| Error::General("Path contains invalid UTF-8".to_string()))?;
        let table = DeltaTableBuilder::from_uri(path_str)
            .with_storage_options(storage_options)
            .load()
            .await
            .map_err(|e| Error::General(format!("Failed to load Delta table: {}", e)))?;

        // Get current version
        let current_version = table.version().unwrap_or(0) as i64;

        // Read file statistics from transaction log
        let file_stats = read_file_stats(self.storage.clone(), path_str, current_version).await?;

        // Extract all metadata components
        let file_info = self.extract_file_info(&table, &file_stats)?;
        let schema = Some(self.extract_schema(&table)?);
        let layout = Some(LayoutInfo::FileBased(
            self.extract_layout(&table, &file_stats)?,
        ));
        let statistics = Some(self.extract_statistics(&file_stats)?);

        Ok(PhysicalMetadata {
            format_name: "Delta Lake".to_string(),
            file_info,
            schema,
            layout,
            statistics,
            orphan_files: None, // FUTURE: Implement orphan detection for Delta when needed
        })
    }

    #[cfg(not(feature = "delta"))]
    async fn extract_metadata(
        &self,
        _options: &PhysicalInspectOptions,
    ) -> Result<PhysicalMetadata> {
        Err(Error::General(
            "Delta Lake support not enabled. Rebuild with --features delta".to_string(),
        ))
    }

    fn format_name(&self) -> &str {
        "Delta Lake"
    }

    fn can_inspect(&self, path: &str) -> bool {
        // For local paths, check if _delta_log directory exists
        let local_path = std::path::Path::new(path);
        let delta_log_path = local_path.join("_delta_log");
        delta_log_path.exists() && delta_log_path.is_dir()
    }
}

/// Factory for creating Delta Lake inspectors
pub struct DeltaInspectorFactory;

#[async_trait]
impl PhysicalInspectorFactory for DeltaInspectorFactory {
    fn create(&self, path: &str, storage: Storage) -> Result<Box<dyn PhysicalInspector>> {
        Ok(Box::new(DeltaInspector::new(PathBuf::from(path), storage)))
    }

    async fn can_handle(&self, _path: &str, storage: &Storage) -> bool {
        // The storage is already configured with the table path as prefix.
        // We just need to check if there's a _delta_log/ directory.
        use futures::TryStreamExt;

        let prefix_path = to_path("_delta_log/");
        let mut stream = storage.list(Some(&prefix_path));

        // Check if we can get at least one item in _delta_log/
        matches!(stream.try_next().await, Ok(Some(_)))
    }

    fn priority(&self) -> i32 {
        80 // Higher than Iceberg (75), checked before
    }
}

// ============================================================================
// Helper structs for parsing Delta transaction log
// ============================================================================

#[cfg(feature = "delta")]
#[derive(Debug)]
struct FileStats {
    total_files: usize,
    total_size: i64,
    total_records: Option<i64>,
    min_size: Option<i64>,
    max_size: Option<i64>,
}

#[cfg(feature = "delta")]
/// Read file statistics from Delta transaction log
async fn read_file_stats(storage: Storage, table_path: &str, version: i64) -> Result<FileStats> {
    let log_file = format!("{}/_delta_log/{:020}.json", table_path, version);

    let get_opts = GetOptions {
        range: None,
        if_modified_since: None,
        if_none_match: None,
    };

    let content = storage
        .get(&log_file, &get_opts)
        .await
        .map_err(|e| Error::General(format!("Failed to read log file: {}", e)))?;

    let content_str = String::from_utf8(content.to_vec())
        .map_err(|e| Error::General(format!("Invalid UTF-8 in log file: {}", e)))?;

    let mut total_files = 0;
    let mut total_size: i64 = 0;
    let mut total_records: Option<i64> = Some(0);
    let mut min_size: Option<i64> = None;
    let mut max_size: Option<i64> = None;

    // Parse all Add and Remove actions
    for line in content_str.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let action: JsonValue = serde_json::from_str(line)
            .map_err(|e| Error::General(format!("Failed to parse log action: {}", e)))?;

        // Look for "add" actions
        if let Some(add) = action.get("add").and_then(|a| a.as_object()) {
            total_files += 1;

            // Get file size
            if let Some(size) = add.get("size").and_then(|s| s.as_i64()) {
                total_size += size;
                min_size = Some(min_size.map_or(size, |min| min.min(size)));
                max_size = Some(max_size.map_or(size, |max| max.max(size)));
            }

            // Get num_records from stats if available
            if let Some(stats_str) = add.get("stats").and_then(|s| s.as_str()) {
                if let Ok(stats) = serde_json::from_str::<JsonValue>(stats_str) {
                    if let Some(num_records) = stats.get("numRecords").and_then(|n| n.as_i64()) {
                        if let Some(ref mut total) = total_records {
                            *total += num_records;
                        }
                    }
                }
            } else {
                total_records = None;
            }
        }
    }

    Ok(FileStats {
        total_files,
        total_size,
        total_records,
        min_size,
        max_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::local::LocalFileSystem;

    #[test]
    fn test_can_inspect_delta_table() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let table_path = temp_dir.path();
        let delta_log = table_path.join("_delta_log");
        fs::create_dir(&delta_log).unwrap();

        let storage: Arc<dyn object_store::ObjectStore> = Arc::new(LocalFileSystem::new());
        let inspector = DeltaInspector::new(table_path.to_path_buf(), storage);
        assert!(inspector.can_inspect(table_path));
    }

    #[test]
    fn test_cannot_inspect_non_delta_table() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let table_path = temp_dir.path();

        let storage: Arc<dyn object_store::ObjectStore> = Arc::new(LocalFileSystem::new());
        let inspector = DeltaInspector::new(table_path.to_path_buf(), storage);
        assert!(!inspector.can_inspect(table_path));
    }

    #[test]
    fn test_factory_priority() {
        let factory = DeltaInspectorFactory;
        assert_eq!(factory.priority(), 80);
    }

    #[test]
    fn test_format_name() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let table_path = temp_dir.path();

        let storage: Arc<dyn object_store::ObjectStore> = Arc::new(LocalFileSystem::new());
        let inspector = DeltaInspector::new(table_path.to_path_buf(), storage);
        assert_eq!(inspector.format_name(), "Delta Lake");
    }

    #[cfg(feature = "delta")]
    #[test]
    fn test_build_storage_options() {
        unsafe {
            std::env::set_var("AWS_ACCESS_KEY_ID", "test_key");
            std::env::set_var("AWS_SECRET_ACCESS_KEY", "test_secret");
            std::env::set_var("AWS_ENDPOINT_URL", "http://localhost:9000");
            std::env::set_var("AWS_REGION", "us-east-1");
        }

        let options = DeltaInspector::build_storage_options();

        assert_eq!(
            options.get("AWS_ACCESS_KEY_ID"),
            Some(&"test_key".to_string())
        );
        assert_eq!(
            options.get("AWS_SECRET_ACCESS_KEY"),
            Some(&"test_secret".to_string())
        );
        assert_eq!(
            options.get("AWS_ENDPOINT_URL"),
            Some(&"http://localhost:9000".to_string())
        );
        assert_eq!(options.get("AWS_REGION"), Some(&"us-east-1".to_string()));
        assert_eq!(options.get("AWS_ALLOW_HTTP"), Some(&"true".to_string()));

        // Cleanup
        unsafe {
            std::env::remove_var("AWS_ACCESS_KEY_ID");
            std::env::remove_var("AWS_SECRET_ACCESS_KEY");
            std::env::remove_var("AWS_ENDPOINT_URL");
            std::env::remove_var("AWS_REGION");
        }
    }
}

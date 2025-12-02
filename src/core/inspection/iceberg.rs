//! Iceberg inspector implementation

use super::traits::{
    ColumnInfo, FileBasedLayout, FileInfo, LayoutInfo, PhysicalInspectOptions,
    PhysicalInspector, PhysicalMetadata, SchemaInfo, StatisticsInfo, VerbosityLevel,
};
use super::PhysicalInspectorFactory;
use crate::core::storage::traits::{GetOptions, ListOptions};
use crate::core::storage::StorageBackend;
use crate::error::{Error, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(feature = "iceberg")]
use apache_avro::Reader;

#[cfg(feature = "iceberg")]
#[derive(Debug, Default)]
struct ManifestStats {
    total_files: i64,
    min_file_size: Option<i64>,
    max_file_size: Option<i64>,
    file_format_counts: HashMap<String, i64>,
    partition_stats: HashMap<String, PartitionInfo>,
}

#[cfg(feature = "iceberg")]
#[derive(Debug, Default)]
struct PartitionInfo {
    files: i64,
    records: i64,
    size: i64,
}

/// Iceberg table inspector
pub struct IcebergInspector {
    path: PathBuf,
    storage: Arc<dyn StorageBackend>,
}

impl IcebergInspector {
    /// Create a new Iceberg inspector
    pub fn new(path: PathBuf, storage: Arc<dyn StorageBackend>) -> Self {
        Self { path, storage }
    }

    #[cfg(feature = "iceberg")]
    async fn find_latest_metadata(&self) -> Result<String> {
        let metadata_prefix = format!("{}/metadata/", self.path.to_str().unwrap_or(""));

        let list_opts = ListOptions {
            prefix: Some(metadata_prefix.clone()),
            delimiter: None,
            max_results: Some(100),
            continuation_token: None,
        };

        let files = self.storage.list(&list_opts).await?;

        let metadata_file = files
            .objects
            .iter()
            .filter(|obj| obj.path.contains(".metadata.json"))
            .max_by_key(|obj| obj.last_modified)
            .ok_or_else(|| {
                Error::General(
                    "No metadata.json file found in metadata/ directory".to_string(),
                )
            })?;

        Ok(metadata_file.path.clone())
    }

    #[cfg(feature = "iceberg")]
    async fn read_metadata(&self, metadata_path: &str) -> Result<serde_json::Value> {
        let get_opts = GetOptions {
            range: None,
            if_modified_since: None,
            if_none_match: None,
        };

        let metadata_bytes = self.storage.get(metadata_path, &get_opts).await?;
        let metadata_str = String::from_utf8(metadata_bytes.to_vec()).map_err(|e| {
            Error::General(format!("Invalid UTF-8 in metadata file: {}", e))
        })?;

        serde_json::from_str(&metadata_str)
            .map_err(|e| Error::General(format!("Failed to parse metadata JSON: {}", e)))
    }

    #[cfg(feature = "iceberg")]
    fn extract_file_info(
        &self,
        metadata: &serde_json::Value,
        metadata_path: &str,
    ) -> FileInfo {
        let format_version = metadata
            .get("format-version")
            .and_then(|v| v.as_i64())
            .unwrap_or(1);

        let table_uuid = metadata
            .get("table-uuid")
            .and_then(|u| u.as_str())
            .unwrap_or("unknown");

        let location = metadata
            .get("location")
            .and_then(|l| l.as_str())
            .unwrap_or(self.path.to_str().unwrap_or(""));

        let mut metadata_map = HashMap::new();
        metadata_map.insert("table_uuid".to_string(), table_uuid.to_string());
        metadata_map.insert("location".to_string(), location.to_string());
        metadata_map.insert(
            "metadata_location".to_string(),
            metadata_path.to_string(),
        );

        let num_snapshots = metadata
            .get("snapshots")
            .and_then(|s| s.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0);

        let current_snapshot_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64())
            .unwrap_or(-1);

        metadata_map.insert("num_snapshots".to_string(), num_snapshots.to_string());
        metadata_map.insert(
            "current_snapshot_id".to_string(),
            if current_snapshot_id == -1 {
                "None".to_string()
            } else {
                current_snapshot_id.to_string()
            },
        );

        FileInfo {
            path: self.path.display().to_string(),
            file_size: 0, // Iceberg tables don't have a single file size
            format_version: format_version.to_string(),
            created_by: None,
            metadata: metadata_map,
        }
    }

    #[cfg(feature = "iceberg")]
    fn extract_schema(&self, metadata: &serde_json::Value) -> Result<SchemaInfo> {
        let schema = metadata
            .get("schema")
            .or_else(|| {
                metadata
                    .get("schemas")
                    .and_then(|s| s.as_array())
                    .and_then(|arr| arr.first())
            })
            .ok_or_else(|| Error::General("No schema found in metadata".to_string()))?;

        let fields = schema
            .get("fields")
            .and_then(|f| f.as_array())
            .ok_or_else(|| Error::General("No fields found in schema".to_string()))?;

        let columns: Vec<ColumnInfo> = fields
            .iter()
            .enumerate()
            .map(|(idx, field)| {
                let name = field
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let field_type = field
                    .get("type")
                    .and_then(|t| t.as_str())
                    .or_else(|| field.get("type").and_then(|t| t.as_object()).map(|_| "COMPLEX"))
                    .unwrap_or("unknown")
                    .to_string();

                let required = field
                    .get("required")
                    .and_then(|r| r.as_bool())
                    .unwrap_or(false);

                ColumnInfo {
                    name,
                    column_type: field_type,
                    nullable: !required,
                    index: idx,
                }
            })
            .collect();

        Ok(SchemaInfo {
            num_columns: columns.len(),
            columns,
        })
    }

    #[cfg(feature = "iceberg")]
    fn extract_layout_info(
        &self,
        metadata: &serde_json::Value,
        _options: &PhysicalInspectOptions,
    ) -> Result<LayoutInfo> {
        let partition_spec = metadata
            .get("partition-spec")
            .or_else(|| {
                metadata
                    .get("partition-specs")
                    .and_then(|s| s.as_array())
                    .and_then(|arr| arr.first())
            })
            .ok_or_else(|| Error::General("No partition spec found in metadata".to_string()))?;

        let fields_opt = partition_spec.get("fields").and_then(|f| f.as_array());

        let partitioning = if let Some(fields) = fields_opt {
            if fields.is_empty() {
                Some("(Unpartitioned)".to_string())
            } else {
                let partition_names: Vec<String> = fields
                    .iter()
                    .filter_map(|f| f.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                    .collect();
                Some(partition_names.join(", "))
            }
        } else {
            Some("(Unpartitioned)".to_string())
        };

        // Extract file statistics from current snapshot
        let current_snapshot_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64())
            .unwrap_or(-1);

        let mut num_files = 0;
        let mut total_size = 0u64;

        if current_snapshot_id != -1 {
            if let Some(snapshots) = metadata.get("snapshots").and_then(|s| s.as_array()) {
                if let Some(snapshot) = snapshots.iter().find(|s| {
                    s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_snapshot_id)
                }) {
                    if let Some(summary) = snapshot.get("summary").and_then(|s| s.as_object()) {
                        num_files = summary
                            .get("total-data-files")
                            .and_then(|f| f.as_str())
                            .and_then(|f| f.parse::<usize>().ok())
                            .unwrap_or(0);

                        total_size = summary
                            .get("total-files-size")
                            .and_then(|s| s.as_str())
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);
                    }
                }
            }
        }

        let mut details = HashMap::new();
        let spec_id = partition_spec
            .get("spec-id")
            .and_then(|id| id.as_i64())
            .unwrap_or(0);
        details.insert("spec_id".to_string(), spec_id.to_string());

        Ok(LayoutInfo::FileBased(FileBasedLayout {
            num_files,
            total_size,
            partitioning,
            details,
        }))
    }

    #[cfg(feature = "iceberg")]
    async fn extract_statistics(
        &self,
        metadata: &serde_json::Value,
        options: &PhysicalInspectOptions,
    ) -> Result<StatisticsInfo> {
        let current_snapshot_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64())
            .unwrap_or(-1);

        if current_snapshot_id == -1 {
            return Ok(StatisticsInfo {
                total_rows: 0,
                compressed_size: 0,
                uncompressed_size: 0,
                column_stats: vec![],
            });
        }

        let snapshots = metadata.get("snapshots").and_then(|s| s.as_array());
        let snapshot = snapshots.and_then(|snaps| {
            snaps.iter().find(|s| {
                s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_snapshot_id)
            })
        });

        if let Some(snapshot) = snapshot {
            if let Some(summary) = snapshot.get("summary").and_then(|s| s.as_object()) {
                let total_rows = summary
                    .get("total-records")
                    .and_then(|r| r.as_str())
                    .and_then(|r| r.parse::<i64>().ok())
                    .unwrap_or(0);

                let compressed_size = summary
                    .get("total-files-size")
                    .and_then(|s| s.as_str())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);

                // If verbose mode, try to read manifest stats
                if options.verbosity >= VerbosityLevel::Verbose {
                    if let Some(manifest_list) = snapshot.get("manifest-list").and_then(|ml| ml.as_str()) {
                        let table_location = metadata
                            .get("location")
                            .and_then(|l| l.as_str())
                            .unwrap_or(self.path.to_str().unwrap_or(""));

                        let manifest_list_path = if manifest_list.starts_with("s3://")
                            || manifest_list.starts_with("gs://")
                            || manifest_list.starts_with("abfs://")
                            || manifest_list.starts_with("file://")
                        {
                            manifest_list.to_string()
                        } else {
                            format!(
                                "{}/{}",
                                table_location.trim_end_matches('/'),
                                manifest_list.trim_start_matches('/')
                            )
                        };

                        // Try to read manifest stats, but don't fail if it doesn't work
                        let _ = self
                            .read_manifest_stats(table_location, &manifest_list_path)
                            .await;
                    }
                }

                return Ok(StatisticsInfo {
                    total_rows,
                    compressed_size,
                    uncompressed_size: compressed_size, // Iceberg doesn't track uncompressed separately
                    column_stats: vec![],
                });
            }
        }

        Ok(StatisticsInfo {
            total_rows: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            column_stats: vec![],
        })
    }

    #[cfg(feature = "iceberg")]
    async fn read_manifest_stats(
        &self,
        table_location: &str,
        manifest_list_path: &str,
    ) -> Result<ManifestStats> {
        let mut stats = ManifestStats::default();

        let get_opts = GetOptions {
            range: None,
            if_modified_since: None,
            if_none_match: None,
        };

        let manifest_list_bytes = self
            .storage
            .get(manifest_list_path, &get_opts)
            .await
            .map_err(|e| Error::General(format!("Failed to read manifest list: {}", e)))?;

        let manifest_list_reader = Reader::new(&manifest_list_bytes[..])
            .map_err(|e| Error::General(format!("Failed to parse manifest list: {}", e)))?;

        for value_result in manifest_list_reader {
            let value = value_result
                .map_err(|e| Error::General(format!("Failed to read manifest entry: {}", e)))?;

            if let apache_avro::types::Value::Record(fields) = value {
                let manifest_path = fields
                    .iter()
                    .find(|(name, _)| name == "manifest-path" || name == "manifest_path")
                    .and_then(|(_, v)| {
                        if let apache_avro::types::Value::String(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    });

                if let Some(path) = manifest_path {
                    let full_manifest_path = if path.starts_with("s3://")
                        || path.starts_with("gs://")
                        || path.starts_with("abfs://")
                        || path.starts_with("file://")
                    {
                        path
                    } else {
                        format!(
                            "{}/{}",
                            table_location.trim_end_matches('/'),
                            path.trim_start_matches('/')
                        )
                    };

                    if let Ok(manifest_bytes) = self.storage.get(&full_manifest_path, &get_opts).await
                    {
                        if let Ok(manifest_reader) = Reader::new(&manifest_bytes[..]) {
                            self.process_manifest_entries(manifest_reader, &mut stats)?;
                        }
                    }
                }
            }
        }

        Ok(stats)
    }

    #[cfg(feature = "iceberg")]
    fn process_manifest_entries(
        &self,
        manifest_reader: Reader<&[u8]>,
        stats: &mut ManifestStats,
    ) -> Result<()> {
        for data_file_result in manifest_reader {
            if let Ok(apache_avro::types::Value::Record(data_fields)) = data_file_result {
                let data_file_record = data_fields
                    .iter()
                    .find(|(name, _)| name == "data_file" || name == "data-file")
                    .and_then(|(_, v)| {
                        if let apache_avro::types::Value::Record(fields) = v {
                            Some(fields)
                        } else {
                            None
                        }
                    });

                let fields_to_process = data_file_record.unwrap_or(&data_fields);

                stats.total_files += 1;

                // Extract file size
                if let Some((_, apache_avro::types::Value::Long(size))) = fields_to_process
                    .iter()
                    .find(|(name, _)| name == "file-size-in-bytes" || name == "file_size_in_bytes")
                {
                    let size = *size;
                    stats.min_file_size = Some(stats.min_file_size.map_or(size, |min| min.min(size)));
                    stats.max_file_size = Some(stats.max_file_size.map_or(size, |max| max.max(size)));
                }

                // Extract file format
                if let Some((_, format_value)) = fields_to_process
                    .iter()
                    .find(|(name, _)| name == "file-format" || name == "file_format")
                {
                    let format = match format_value {
                        apache_avro::types::Value::Int(format_id) => match format_id {
                            0 => "AVRO",
                            1 => "PARQUET",
                            2 => "ORC",
                            _ => "UNKNOWN",
                        },
                        apache_avro::types::Value::String(s) => s.as_str(),
                        _ => "UNKNOWN",
                    };
                    *stats
                        .file_format_counts
                        .entry(format.to_string())
                        .or_insert(0) += 1;
                }

                // Extract partition data
                if let Some((_, apache_avro::types::Value::Map(partition_data))) =
                    fields_to_process.iter().find(|(name, _)| name == "partition")
                {
                    let partition_key = if partition_data.is_empty() {
                        "{}".to_string()
                    } else {
                        format!("{:?}", partition_data)
                    };

                    let entry = stats.partition_stats.entry(partition_key).or_default();
                    entry.files += 1;

                    if let Some((_, apache_avro::types::Value::Long(records))) = fields_to_process
                        .iter()
                        .find(|(name, _)| name == "record-count" || name == "record_count")
                    {
                        entry.records += *records;
                    }

                    if let Some((_, apache_avro::types::Value::Long(size))) = fields_to_process
                        .iter()
                        .find(|(name, _)| name == "file-size-in-bytes" || name == "file_size_in_bytes")
                    {
                        entry.size += *size;
                    }
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl PhysicalInspector for IcebergInspector {
    #[cfg(feature = "iceberg")]
    async fn extract_metadata(
        &self,
        options: &PhysicalInspectOptions,
    ) -> Result<PhysicalMetadata> {
        let metadata_path = self.find_latest_metadata().await?;
        let metadata = self.read_metadata(&metadata_path).await?;

        let file_info = self.extract_file_info(&metadata, &metadata_path);

        let schema = if options.show_schema {
            Some(self.extract_schema(&metadata)?)
        } else {
            None
        };

        let layout = if options.show_layout {
            Some(self.extract_layout_info(&metadata, options)?)
        } else {
            None
        };

        let statistics = if options.show_stats {
            Some(self.extract_statistics(&metadata, options).await?)
        } else {
            None
        };

        Ok(PhysicalMetadata {
            format_name: "Apache Iceberg".to_string(),
            file_info,
            schema,
            layout,
            statistics,
        })
    }

    #[cfg(not(feature = "iceberg"))]
    async fn extract_metadata(
        &self,
        _options: &PhysicalInspectOptions,
    ) -> Result<PhysicalMetadata> {
        Err(Error::General(
            "Iceberg support not enabled. Rebuild with --features iceberg".to_string(),
        ))
    }

    fn format_name(&self) -> &str {
        "Apache Iceberg"
    }

    fn can_inspect(&self, path: &Path) -> bool {
        // Check if path contains a metadata/ directory
        let path_str = path.to_str().unwrap_or("");
        let metadata_path = path.join("metadata");

        // Check if metadata directory exists OR if path ends with/contains metadata
        metadata_path.exists()
            || path_str.ends_with("/metadata")
            || path_str.contains("/metadata/")
    }
}

/// Factory for creating Iceberg inspectors
pub struct IcebergInspectorFactory;

#[async_trait]
impl PhysicalInspectorFactory for IcebergInspectorFactory {
    fn create(
        &self,
        path: &Path,
        storage: Arc<dyn StorageBackend>,
    ) -> Result<Box<dyn PhysicalInspector>> {
        Ok(Box::new(IcebergInspector::new(
            path.to_path_buf(),
            storage,
        )))
    }

    async fn can_handle(&self, path: &Path, storage: &Arc<dyn StorageBackend>) -> bool {
        // Check for metadata directory by trying to list files in it
        let path_str = path.to_str().unwrap_or("");

        // Strip the scheme (s3://, file://, etc.) if present, then strip bucket/container
        let clean_path = if let Some(pos) = path_str.find("://") {
            let after_scheme = &path_str[pos + 3..];
            // For cloud storage, strip the bucket/container name (first path segment)
            if let Some(slash_pos) = after_scheme.find('/') {
                &after_scheme[slash_pos + 1..]
            } else {
                // Just the bucket name, no path
                ""
            }
        } else {
            path_str
        };

        let metadata_prefix = if clean_path.is_empty() {
            "metadata/".to_string()
        } else {
            format!("{}/metadata/", clean_path)
        };

        let list_opts = crate::core::storage::traits::ListOptions {
            prefix: Some(metadata_prefix),
            delimiter: None,
            max_results: Some(1),
            continuation_token: None,
        };

        match storage.list(&list_opts).await {
            Ok(result) => !result.objects.is_empty(),
            Err(_) => false,
        }
    }

    fn priority(&self) -> i32 {
        75
    }
}

#[cfg(test)]
#[cfg(feature = "iceberg")]
mod tests {
    use super::*;

    #[test]
    fn test_iceberg_factory_priority() {
        let factory = IcebergInspectorFactory;
        assert_eq!(factory.priority(), 75);
    }

    #[test]
    fn test_iceberg_inspector_format_name() {
        let storage = Arc::new(crate::core::storage::LocalBackend::new().unwrap());
        let inspector = IcebergInspector::new(PathBuf::from("/test"), storage);
        assert_eq!(inspector.format_name(), "Apache Iceberg");
    }

    #[test]
    fn test_iceberg_inspector_can_inspect() {
        let storage = Arc::new(crate::core::storage::LocalBackend::new().unwrap());
        let inspector = IcebergInspector::new(PathBuf::from("/test"), storage);

        assert!(inspector.can_inspect(Path::new("/path/to/table/metadata")));
        assert!(!inspector.can_inspect(Path::new("/path/to/file.parquet")));
    }
}

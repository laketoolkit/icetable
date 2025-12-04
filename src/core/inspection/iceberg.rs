//! Iceberg inspector implementation

use super::PhysicalInspectorFactory;
use super::traits::{
    ColumnInfo, FileBasedLayout, FileInfo, LayoutInfo, OrphanFileEntry, OrphanFilesInfo,
    PhysicalInspectOptions, PhysicalInspector, PhysicalMetadata, SchemaInfo, StatisticsInfo,
    VerbosityLevel,
};
use crate::core::metadata::{IcebergMetadataService, MetadataService};
use crate::core::storage::StorageBackend;
use crate::core::storage::traits::{GetOptions, ListOptions};
use crate::error::{Error, Result};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
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
        let metadata_dir = format!(
            "{}/metadata",
            self.path.to_str().unwrap_or("").trim_end_matches('/')
        );

        // First try to read version-hint.text for authoritative version
        // This avoids S3 eventual consistency issues with list operations
        let version_hint_path = format!("{}/version-hint.text", metadata_dir);
        let get_opts = GetOptions::default();

        if let Ok(version_bytes) = self.storage.get(&version_hint_path, &get_opts).await {
            if let Ok(version_str) = String::from_utf8(version_bytes.to_vec()) {
                if let Ok(version) = version_str.trim().parse::<i32>() {
                    let metadata_path = format!("{}/v{}.metadata.json", metadata_dir, version);
                    // Verify file exists by trying to read it
                    if self.storage.get(&metadata_path, &get_opts).await.is_ok() {
                        return Ok(metadata_path);
                    }
                }
            }
        }

        // Fallback to listing if version-hint doesn't exist or is invalid
        let list_opts = ListOptions {
            prefix: Some(format!("{}/", metadata_dir)),
            delimiter: None,
            max_results: Some(500),
            continuation_token: None,
        };

        let files = self.storage.list(&list_opts).await?;

        // Find latest metadata by version number in filename
        // Supports both formats: v1.metadata.json and 00001-uuid.metadata.json
        let metadata_file = files
            .objects
            .iter()
            .filter(|obj| obj.path.contains(".metadata.json"))
            .max_by_key(|obj| {
                let name = obj.path.rsplit('/').next().unwrap_or("");
                if name.starts_with('v') {
                    name.trim_start_matches('v')
                        .split('.')
                        .next()
                        .and_then(|n| n.parse::<i64>().ok())
                        .unwrap_or(0)
                } else {
                    name.split('-')
                        .next()
                        .and_then(|n| n.parse::<i64>().ok())
                        .unwrap_or(0)
                }
            })
            .ok_or_else(|| {
                Error::General("No metadata.json file found in metadata/ directory".to_string())
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
        let metadata_str = String::from_utf8(metadata_bytes.to_vec())
            .map_err(|e| Error::General(format!("Invalid UTF-8 in metadata file: {}", e)))?;

        serde_json::from_str(&metadata_str)
            .map_err(|e| Error::General(format!("Failed to parse metadata JSON: {}", e)))
    }

    #[cfg(feature = "iceberg")]
    fn extract_file_info(
        &self,
        metadata: &serde_json::Value,
        metadata_path: &str,
        options: &PhysicalInspectOptions,
        iceberg_metadata: Option<&iceberg::spec::TableMetadata>,
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
        metadata_map.insert("Table UUID".to_string(), table_uuid.to_string());
        metadata_map.insert("Location".to_string(), location.to_string());
        metadata_map.insert("Metadata Location".to_string(), metadata_path.to_string());

        // Use iceberg-rs TableMetadata for snapshot count if available (for consistency with vacuum)
        let num_snapshots = if let Some(ice_meta) = iceberg_metadata {
            ice_meta.snapshots().count()
        } else {
            metadata
                .get("snapshots")
                .and_then(|s| s.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0)
        };

        let current_snapshot_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64())
            .unwrap_or(-1);

        metadata_map.insert("Snapshots".to_string(), num_snapshots.to_string());
        metadata_map.insert(
            "Current Snapshot".to_string(),
            if current_snapshot_id == -1 {
                "None".to_string()
            } else {
                current_snapshot_id.to_string()
            },
        );

        // Extract last updated timestamp from current snapshot
        let snapshots = metadata.get("snapshots").and_then(|s| s.as_array());
        if let Some(snaps) = snapshots {
            if let Some(current) = snaps.iter().find(|s| {
                s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_snapshot_id)
            }) {
                if let Some(ts) = current.get("timestamp-ms").and_then(|t| t.as_i64()) {
                    let datetime = chrono::DateTime::from_timestamp_millis(ts)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| ts.to_string());
                    metadata_map.insert("Last Updated".to_string(), datetime);
                }
            }
        }

        // Extract table properties in verbose mode
        if options.verbosity >= VerbosityLevel::Verbose {
            let properties = metadata.get("properties").and_then(|p| p.as_object());

            // Relevant default properties to show
            let default_props = [
                ("write.format.default", "parquet"),
                ("write.target-file-size-bytes", "134217728"), // 128MB
                ("write.parquet.compression-codec", "zstd"),
                ("write.delete.mode", "copy-on-write"),
            ];

            if let Some(props) = properties {
                if props.is_empty() {
                    // Show defaults when no properties configured
                    metadata_map.insert("property._using_defaults".to_string(), "true".to_string());
                    for (key, default_value) in &default_props {
                        metadata_map.insert(
                            format!("property.{}", key),
                            format!("{} (default)", default_value),
                        );
                    }
                } else {
                    // Show actual properties
                    for (key, value) in props {
                        if let Some(v) = value.as_str() {
                            metadata_map.insert(format!("property.{}", key), v.to_string());
                        }
                    }
                    // Also show relevant defaults that aren't explicitly set
                    for (key, default_value) in &default_props {
                        if !props.contains_key(*key) {
                            metadata_map.insert(
                                format!("property.{}", key),
                                format!("{} (default)", default_value),
                            );
                        }
                    }
                }
            } else {
                // No properties object at all - show defaults
                metadata_map.insert("property._using_defaults".to_string(), "true".to_string());
                for (key, default_value) in &default_props {
                    metadata_map.insert(
                        format!("property.{}", key),
                        format!("{} (default)", default_value),
                    );
                }
            }
        }

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
                    .or_else(|| {
                        field
                            .get("type")
                            .and_then(|t| t.as_object())
                            .map(|_| "COMPLEX")
                    })
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
    fn extract_sort_order(&self, metadata: &serde_json::Value) -> String {
        let sort_orders = match metadata.get("sort-orders").and_then(|s| s.as_array()) {
            Some(orders) => orders,
            None => return "(unsorted)".to_string(),
        };

        let default_sort_order_id = metadata
            .get("default-sort-order-id")
            .and_then(|id| id.as_i64())
            .unwrap_or(0);

        let sort_order = match sort_orders
            .iter()
            .find(|so| so.get("order-id").and_then(|id| id.as_i64()) == Some(default_sort_order_id))
        {
            Some(order) => order,
            None => return "(unsorted)".to_string(),
        };

        let fields = match sort_order.get("fields").and_then(|f| f.as_array()) {
            Some(f) if !f.is_empty() => f,
            _ => return "(unsorted)".to_string(),
        };

        // Get schema to resolve column names from source-id
        let schema = metadata.get("schema").or_else(|| {
            metadata
                .get("schemas")
                .and_then(|s| s.as_array())
                .and_then(|arr| arr.first())
        });

        let schema_fields = schema
            .and_then(|s| s.get("fields"))
            .and_then(|f| f.as_array());

        let sort_cols: Vec<String> = fields
            .iter()
            .filter_map(|f| {
                let source_id = f.get("source-id").and_then(|id| id.as_i64())?;
                let direction = f.get("direction").and_then(|d| d.as_str()).unwrap_or("asc");
                let null_order = f
                    .get("null-order")
                    .and_then(|n| n.as_str())
                    .unwrap_or("nulls-first");

                // Try to find column name from schema
                let col_name = schema_fields
                    .and_then(|fields| {
                        fields.iter().find(|field| {
                            field.get("id").and_then(|id| id.as_i64()) == Some(source_id)
                        })
                    })
                    .and_then(|field| field.get("name").and_then(|n| n.as_str()))
                    .unwrap_or("unknown");

                let dir_symbol = if direction == "desc" { "↓" } else { "↑" };
                Some(format!("{} {}", col_name, dir_symbol))
            })
            .collect();

        if sort_cols.is_empty() {
            "(unsorted)".to_string()
        } else {
            sort_cols.join(", ")
        }
    }

    #[cfg(feature = "iceberg")]
    fn extract_layout_info(
        &self,
        metadata: &serde_json::Value,
        options: &PhysicalInspectOptions,
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
                Some("Unpartitioned".to_string())
            } else {
                let partition_names: Vec<String> = fields
                    .iter()
                    .filter_map(|f| {
                        f.get("name")
                            .and_then(|n| n.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();
                Some(partition_names.join(", "))
            }
        } else {
            Some("Unpartitioned".to_string())
        };

        // Extract file statistics from current snapshot
        let current_snapshot_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64())
            .unwrap_or(-1);

        let mut num_files = 0;
        let mut total_size = 0u64;
        let mut details = HashMap::new();

        // Get table properties for file size target
        let properties = metadata.get("properties").and_then(|p| p.as_object());
        let target_file_size: u64 = properties
            .and_then(|p| p.get("write.target-file-size-bytes"))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(128 * 1024 * 1024); // Default 128MB

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

                        // Extract delta info (files added/deleted in last snapshot)
                        let added_files = summary
                            .get("added-data-files")
                            .and_then(|f| f.as_str())
                            .and_then(|f| f.parse::<i64>().ok())
                            .unwrap_or(0);
                        let deleted_files = summary
                            .get("deleted-data-files")
                            .and_then(|f| f.as_str())
                            .and_then(|f| f.parse::<i64>().ok())
                            .unwrap_or(0);

                        if added_files > 0 || deleted_files > 0 {
                            details.insert(
                                "Last Change".to_string(),
                                format!("+{} / -{} files", added_files, deleted_files),
                            );
                        }

                        // Get operation type (from summary.operation)
                        if let Some(op) = summary.get("operation").and_then(|o| o.as_str()) {
                            details.insert("Last Operation".to_string(), op.to_string());
                        }

                        // === Verbose mode fields ===
                        if options.verbosity >= VerbosityLevel::Verbose {
                            // Delete Files: X (position: Y, equality: Z)
                            let total_delete_files = summary
                                .get("total-delete-files")
                                .and_then(|f| f.as_str())
                                .and_then(|f| f.parse::<i64>().ok())
                                .unwrap_or(0);
                            let equality_deletes = summary
                                .get("total-equality-deletes")
                                .and_then(|f| f.as_str())
                                .and_then(|f| f.parse::<i64>().ok())
                                .unwrap_or(0);
                            let position_deletes = summary
                                .get("total-position-deletes")
                                .and_then(|f| f.as_str())
                                .and_then(|f| f.parse::<i64>().ok())
                                .unwrap_or(0);

                            details.insert(
                                "Delete Files".to_string(),
                                format!(
                                    "{} (position: {}, equality: {})",
                                    total_delete_files, position_deletes, equality_deletes
                                ),
                            );

                            // Manifest Files count (from manifest-list if available)
                            // Note: actual manifest count requires reading manifest-list avro file
                            // For now we show added manifests from summary
                            let added_manifests = summary
                                .get("manifests-created")
                                .or_else(|| summary.get("added-files-size"))
                                .and_then(|f| f.as_str())
                                .and_then(|f| f.parse::<i64>().ok());

                            // Try to get manifest count from snapshot
                            if let Some(manifest_list) =
                                snapshot.get("manifest-list").and_then(|m| m.as_str())
                            {
                                // Count manifests by counting entries (approximate from path)
                                details.insert(
                                    "Manifest List".to_string(),
                                    manifest_list
                                        .split('/')
                                        .last()
                                        .unwrap_or("unknown")
                                        .to_string(),
                                );
                            }

                            // Avg File Size with warning
                            if num_files > 0 {
                                let avg_size = total_size / num_files as u64;
                                let avg_size_mb = avg_size as f64 / (1024.0 * 1024.0);
                                let target_mb = target_file_size as f64 / (1024.0 * 1024.0);
                                let threshold = target_file_size / 2; // 50% of target

                                let avg_display = if avg_size < threshold {
                                    format!(
                                        "{:.2} MB (below target: {:.0} MB)",
                                        avg_size_mb, target_mb
                                    )
                                } else {
                                    format!("{:.2} MB", avg_size_mb)
                                };
                                details.insert("Avg File Size".to_string(), avg_display);
                            }

                            // File Format with codec
                            let format = properties
                                .and_then(|p| p.get("write.format.default"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("parquet");
                            let codec = properties
                                .and_then(|p| p.get("write.parquet.compression-codec"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("zstd");
                            details.insert(
                                "File Format".to_string(),
                                format!("{} ({})", format, codec),
                            );
                        }
                    }
                }
            }
        }

        // Sort order info (always show in verbose, show (unsorted) if not defined)
        let sort_order_str = self.extract_sort_order(metadata);
        if options.verbosity >= VerbosityLevel::Verbose {
            details.insert("Sort Order".to_string(), sort_order_str);
        } else if sort_order_str != "(unsorted)" {
            details.insert("Sort Order".to_string(), sort_order_str);
        }

        let spec_id = partition_spec
            .get("spec-id")
            .and_then(|id| id.as_i64())
            .unwrap_or(0);
        details.insert("Partition Spec ID".to_string(), spec_id.to_string());

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
                    if let Some(manifest_list) =
                        snapshot.get("manifest-list").and_then(|ml| ml.as_str())
                    {
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

                    if let Ok(manifest_bytes) =
                        self.storage.get(&full_manifest_path, &get_opts).await
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
                    stats.min_file_size =
                        Some(stats.min_file_size.map_or(size, |min| min.min(size)));
                    stats.max_file_size =
                        Some(stats.max_file_size.map_or(size, |max| max.max(size)));
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
                if let Some((_, apache_avro::types::Value::Map(partition_data))) = fields_to_process
                    .iter()
                    .find(|(name, _)| name == "partition")
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

                    if let Some((_, apache_avro::types::Value::Long(size))) =
                        fields_to_process.iter().find(|(name, _)| {
                            name == "file-size-in-bytes" || name == "file_size_in_bytes"
                        })
                    {
                        entry.size += *size;
                    }
                }
            }
        }

        Ok(())
    }

    /// Detect orphan files - files in data/ not tracked in metadata
    ///
    /// - `deep_scan=true`: Check ALL snapshots (slow but accurate)
    /// - `deep_scan=false`: Check only current snapshot (fast but may have false positives)
    #[cfg(feature = "iceberg")]
    async fn detect_orphan_files(
        &self,
        metadata: &serde_json::Value,
        deep_scan: bool,
    ) -> Result<OrphanFilesInfo> {
        let table_path = self.path.to_str().unwrap_or("").to_string();
        let metadata_service = IcebergMetadataService::new_async(table_path.clone()).await?;

        // Get referenced files based on scan mode
        let reference_set: HashSet<String> = if deep_scan {
            // Deep scan: check ALL snapshots
            let all_referenced = metadata_service.get_all_referenced_files().await?;
            let mut set = HashSet::new();
            for path in &all_referenced {
                set.insert(path.clone());
                if let Some(filename) = path.rsplit('/').next() {
                    set.insert(filename.to_string());
                }
            }
            set
        } else {
            // Quick scan: only current snapshot
            let table_location = metadata
                .get("location")
                .and_then(|l| l.as_str())
                .unwrap_or(&table_path);
            self.get_tracked_files(metadata, table_location).await?
        };

        // Scan storage for parquet files
        let storage_files = metadata_service.scan_data_files_on_storage().await?;

        // Find orphan files
        let mut orphans: Vec<OrphanFileEntry> = Vec::new();
        let mut total_size: u64 = 0;
        let mut total_orphan_count: usize = 0;

        for file in &storage_files {
            let is_referenced = reference_set
                .iter()
                .any(|referenced| file.path.ends_with(referenced) || file.path == *referenced);

            if !is_referenced {
                total_orphan_count += 1;
                total_size += file.size;

                if orphans.len() < 10 {
                    orphans.push(OrphanFileEntry {
                        path: file.path.clone(),
                        size: file.size,
                    });
                }
            }
        }

        Ok(OrphanFilesInfo {
            count: total_orphan_count,
            total_size,
            files: orphans,
            truncated: total_orphan_count > 10,
            is_deep_scan: deep_scan,
        })
    }

    /// Get all file paths tracked in current snapshot manifests (parallelized)
    ///
    /// Note: For accurate orphan detection across ALL snapshots, use
    /// IcebergMetadataService::get_all_referenced_files() instead.
    /// This method is used for quick scans (current snapshot only).
    #[cfg(feature = "iceberg")]
    async fn get_tracked_files(
        &self,
        metadata: &serde_json::Value,
        table_location: &str,
    ) -> Result<HashSet<String>> {
        // Get current snapshot
        let current_snapshot_id = metadata
            .get("current-snapshot-id")
            .and_then(|id| id.as_i64());

        let snapshots = metadata
            .get("snapshots")
            .and_then(|s| s.as_array())
            .map(|arr| arr.as_slice())
            .unwrap_or(&[]);

        // Find current snapshot
        let current_snapshot = snapshots
            .iter()
            .find(|s| s.get("snapshot-id").and_then(|id| id.as_i64()) == current_snapshot_id);

        let Some(snapshot) = current_snapshot else {
            return Ok(HashSet::new());
        };

        let Some(manifest_list) = snapshot.get("manifest-list").and_then(|m| m.as_str()) else {
            return Ok(HashSet::new());
        };

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

        // Read manifest list
        let get_opts = GetOptions {
            range: None,
            if_modified_since: None,
            if_none_match: None,
        };

        let manifest_list_bytes = match self.storage.get(&manifest_list_path, &get_opts).await {
            Ok(bytes) => bytes,
            Err(_) => return Ok(HashSet::new()),
        };

        let manifest_list_reader = match Reader::new(&manifest_list_bytes[..]) {
            Ok(reader) => reader,
            Err(_) => return Ok(HashSet::new()),
        };

        // Collect all manifest paths first
        let mut manifest_paths: Vec<String> = Vec::new();
        for value_result in manifest_list_reader {
            if let Ok(apache_avro::types::Value::Record(fields)) = value_result {
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
                    let full_path = if path.starts_with("s3://")
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
                    manifest_paths.push(full_path);
                }
            }
        }

        // Read manifests in parallel with concurrency limit
        use futures::stream::{self, StreamExt};
        use indicatif::{ProgressBar, ProgressStyle};

        let total_manifests = manifest_paths.len();
        let pb = ProgressBar::new(total_manifests as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.cyan} Scanning manifests {bar:30.dim.white/dim} {pos}/{len}")
                .unwrap()
                .progress_chars("━━╺"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let results: Vec<HashSet<String>> = stream::iter(manifest_paths.into_iter())
            .map(|path| {
                let storage = Arc::clone(&self.storage);
                let pb = pb.clone();
                async move {
                    let get_opts = GetOptions {
                        range: None,
                        if_modified_since: None,
                        if_none_match: None,
                    };
                    let result = if let Ok(bytes) = storage.get(&path, &get_opts).await {
                        if let Ok(reader) = Reader::new(&bytes[..]) {
                            Self::extract_file_paths_static(reader)
                        } else {
                            HashSet::new()
                        }
                    } else {
                        HashSet::new()
                    };
                    pb.inc(1);
                    result
                }
            })
            .buffer_unordered(32) // Process up to 32 manifests concurrently
            .collect()
            .await;

        pb.finish_and_clear();

        // Merge all results
        let mut tracked_files: HashSet<String> = HashSet::new();
        for result in results {
            tracked_files.extend(result);
        }

        Ok(tracked_files)
    }

    /// Extract file paths from manifest (static version for parallel execution)
    #[cfg(feature = "iceberg")]
    #[allow(dead_code)]
    fn extract_file_paths_static(manifest_reader: Reader<&[u8]>) -> HashSet<String> {
        let mut files = HashSet::new();
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

                if let Some((_, file_path_value)) = fields_to_process
                    .iter()
                    .find(|(name, _)| name == "file-path" || name == "file_path")
                {
                    if let apache_avro::types::Value::String(path) = file_path_value {
                        let filename = path.split('/').last().unwrap_or(path);
                        files.insert(filename.to_string());
                        files.insert(path.clone());
                    }
                }
            }
        }
        files
    }
}

#[async_trait]
impl PhysicalInspector for IcebergInspector {
    #[cfg(feature = "iceberg")]
    async fn extract_metadata(&self, options: &PhysicalInspectOptions) -> Result<PhysicalMetadata> {
        let metadata_path = self.find_latest_metadata().await?;
        let metadata = self.read_metadata(&metadata_path).await?;

        // Load iceberg-rs TableMetadata for accurate snapshot count (consistent with vacuum)
        let iceberg_meta =
            match IcebergMetadataService::new_async(self.path.to_str().unwrap_or("").to_string())
                .await
            {
                Ok(service) => service.load_metadata().await.ok().map(|(m, _)| m),
                Err(_) => None,
            };

        let file_info = self.extract_file_info(
            &metadata,
            &metadata_path,
            options,
            iceberg_meta.as_ref().map(|m| m.as_ref()),
        );

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

        // Detect orphan files only in verbose mode
        let orphan_files = if options.verbosity >= VerbosityLevel::Verbose {
            match self.detect_orphan_files(&metadata, options.deep_scan).await {
                Ok(info) if info.count > 0 => Some(info),
                _ => None,
            }
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
        metadata_path.exists() || path_str.ends_with("/metadata") || path_str.contains("/metadata/")
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
        Ok(Box::new(IcebergInspector::new(path.to_path_buf(), storage)))
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

//! Manifest reading and statistics collection for Iceberg tables

use apache_avro::Reader;
use std::collections::HashMap;

use crate::core::storage::{ObjectStoreExt, Storage};
use crate::error::{Error, Result};

/// Statistics collected from manifest files
#[derive(Debug, Default)]
pub struct ManifestStats {
    pub total_files: i64,
    pub total_size: i64,
    pub min_file_size: Option<i64>,
    pub max_file_size: Option<i64>,
    pub file_format_counts: HashMap<String, i64>,
    pub partition_stats: HashMap<String, PartitionInfo>,
}

/// Statistics for a single partition
#[derive(Debug, Default)]
pub struct PartitionInfo {
    pub files: i64,
    pub records: i64,
    pub size: i64,
}

/// Read manifest statistics from a manifest list
pub async fn read_manifest_stats(
    storage: &Storage,
    table_location: &str,
    manifest_list_path: &str,
) -> Result<ManifestStats> {
    let mut stats = ManifestStats::default();

    let manifest_list_bytes = storage
        .get_bytes_str(manifest_list_path)
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
                let full_manifest_path = normalize_path(&path, table_location);

                if let Ok(manifest_bytes) = storage.get_bytes_str(&full_manifest_path).await
                    && let Ok(manifest_reader) = Reader::new(&manifest_bytes[..])
                {
                    process_manifest_entries(manifest_reader, &mut stats)?;
                }
            }
        }
    }

    Ok(stats)
}

/// Process entries from a single manifest file
pub fn process_manifest_entries(
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
                stats.total_size += size;
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

/// Normalize a path that may be relative or absolute
pub fn normalize_path(path: &str, table_location: &str) -> String {
    if path.starts_with("s3://")
        || path.starts_with("gs://")
        || path.starts_with("abfs://")
        || path.starts_with("file://")
    {
        path.to_string()
    } else {
        format!(
            "{}/{}",
            table_location.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}

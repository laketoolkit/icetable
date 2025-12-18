//! Pipeline utilities for file compaction
//!
//! Contains internal types and helper functions used by the optimization pipeline.

use std::collections::HashMap;

use arrow::array::RecordBatch;
use arrow::datatypes::SchemaRef;
use arrow_cast::cast;
use object_store::path::Path as ObjectPath;

use super::super::FileGroup;
use crate::core::metadata::DataFileInfo;
use crate::error::{Error, Result};
use crate::utils::core::normalize_relative_path;

/// Result of compacting a single partition group
pub(super) struct CompactionResult {
    pub added: Vec<DataFileInfo>,
    pub removed: Vec<DataFileInfo>,
}

/// Message type for the reader-writer pipeline channel
pub(super) enum BatchMessage {
    /// A record batch to write (already coerced if needed)
    Batch(RecordBatch),
    /// All readers are done - writers should finish and exit
    Done,
    /// An error occurred during reading
    Error(String),
}

/// Calculate optimal subgroup size based on workload characteristics
pub(super) fn calculate_optimal_subgroup_size(
    total_files: usize,
    total_bytes: u64,
    parallelism: usize,
) -> usize {
    if total_files == 0 {
        return 100;
    }

    let avg_file_size = total_bytes / total_files as u64;

    // Base size adjusted by file size (smaller files = smaller groups for more parallelism)
    let size_factor = match avg_file_size {
        0..=1_000_000 => 100,            // <1MB: small files, groups of ~100
        1_000_001..=10_000_000 => 200,   // 1-10MB: groups of ~200
        10_000_001..=50_000_000 => 350,  // 10-50MB: groups of ~350
        50_000_001..=100_000_000 => 500, // 50-100MB: groups of ~500
        _ => 750,                        // >100MB: large groups
    };

    // Ensure we have enough groups for parallelism (at least 2x parallelism)
    let min_groups = parallelism * 2;
    let max_subgroup_for_parallelism = total_files / min_groups.max(1);

    // Take the smaller of size-based and parallelism-based limits
    // but ensure at least 50 files per group to avoid excessive overhead
    size_factor.min(max_subgroup_for_parallelism).max(50)
}

/// Subdivide large groups into smaller sub-groups for parallel processing
///
/// This ensures that even non-partitioned tables with many small files
/// can benefit from parallel compaction.
pub(super) fn subdivide_groups(
    groups: Vec<FileGroup>,
    max_files_per_subgroup: usize,
) -> Vec<FileGroup> {
    let mut result = Vec::new();

    for group in groups {
        if group.files.len() <= max_files_per_subgroup {
            result.push(group);
        } else {
            // Split into sub-groups
            for (idx, chunk) in group.files.chunks(max_files_per_subgroup).enumerate() {
                let total_size: u64 = chunk.iter().map(|f| f.size).sum();
                let total_records: u64 = chunk.iter().map(|f| f.record_count).sum();

                result.push(FileGroup {
                    files: chunk.to_vec(),
                    total_size,
                    total_records,
                    // Add sub-group index to partition key for unique output files
                    partition_key: if group.partition_key.is_empty() {
                        format!("__subgroup_{}", idx)
                    } else {
                        format!("{}/__subgroup_{}", group.partition_key, idx)
                    },
                });
            }
        }
    }

    result
}

/// Convert a file path to an ObjectPath for object store operations
pub(super) fn path_to_object_path(
    path: &str,
    table_base: &str,
) -> std::result::Result<ObjectPath, String> {
    // For S3 URLs, extract the path within the bucket
    if let Some(s3_path) = path.strip_prefix("s3://")
        && let Some(slash_pos) = s3_path.find('/')
    {
        let object_path = &s3_path[slash_pos + 1..];
        return Ok(ObjectPath::from(object_path));
    }

    // For gs:// (GCS) URLs
    if let Some(gcs_path) = path.strip_prefix("gs://")
        && let Some(slash_pos) = gcs_path.find('/')
    {
        let object_path = &gcs_path[slash_pos + 1..];
        return Ok(ObjectPath::from(object_path));
    }

    // For az:// or azure:// URLs
    for prefix in ["az://", "azure://"] {
        if let Some(az_path) = path.strip_prefix(prefix)
            && let Some(slash_pos) = az_path.find('/')
        {
            let object_path = &az_path[slash_pos + 1..];
            return Ok(ObjectPath::from(object_path));
        }
    }

    // For local paths, use relative path from table base
    if let Some(relative) = normalize_relative_path(path, table_base) {
        return Ok(ObjectPath::from(relative));
    }

    // Fallback
    let clean_path = path.strip_prefix("file://").unwrap_or(path);
    Ok(ObjectPath::from(clean_path))
}

/// Parse a partition key string into a HashMap
pub(super) fn parse_partition_key(key: &str) -> HashMap<String, String> {
    if key.is_empty() {
        return HashMap::new();
    }

    key.split('/')
        .filter_map(|part| {
            let mut split = part.splitn(2, '=');
            match (split.next(), split.next()) {
                (Some(k), Some(v)) => Some((k.to_string(), v.to_string())),
                _ => None,
            }
        })
        .collect()
}

/// Coerce a batch to match the target schema
///
/// This handles cases where batches from different files have slightly
/// different schemas (e.g., different field metadata, different column order,
/// or missing columns). Columns are matched by name, not position.
pub(super) fn coerce_batch_to_schema(
    batch: &RecordBatch,
    target_schema: &SchemaRef,
) -> Result<RecordBatch> {
    // If schemas match exactly, return as-is
    if batch.schema() == *target_schema {
        return Ok(batch.clone());
    }

    let batch_schema = batch.schema();
    let num_rows = batch.num_rows();

    // Match columns by name, not by position
    let columns: Vec<_> = target_schema
        .fields()
        .iter()
        .map(|target_field| {
            let target_name = target_field.name();
            let target_type = target_field.data_type();

            // Find column in source batch by name
            match batch_schema.column_with_name(target_name) {
                Some((idx, _source_field)) => {
                    let source_column = batch.column(idx);
                    let source_type = source_column.data_type();

                    if source_type == target_type {
                        Ok(source_column.clone())
                    } else {
                        cast(source_column, target_type).map_err(|e| Error::DataValidation {
                            message: format!(
                                "Failed to cast column '{}' from {:?} to {:?}: {}",
                                target_name, source_type, target_type, e
                            ),
                        })
                    }
                }
                None => {
                    // Column missing in source - create null array
                    use arrow::array::new_null_array;
                    Ok(new_null_array(target_type, num_rows))
                }
            }
        })
        .collect::<Result<Vec<_>>>()?;

    RecordBatch::try_new(target_schema.clone(), columns).map_err(|e| Error::DataValidation {
        message: format!("Failed to create coerced batch: {}", e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_optimal_subgroup_size_empty() {
        assert_eq!(calculate_optimal_subgroup_size(0, 0, 4), 100);
    }

    #[test]
    fn test_calculate_optimal_subgroup_size_small_files() {
        // Small files (<1MB avg) should use smaller groups
        let size = calculate_optimal_subgroup_size(1000, 500_000_000, 4); // 500KB avg
        assert!(size <= 100);
    }

    #[test]
    fn test_calculate_optimal_subgroup_size_large_files() {
        // Large files (>100MB avg) should use larger groups
        let size = calculate_optimal_subgroup_size(100, 20_000_000_000, 4); // 200MB avg
        assert!(size >= 50);
    }

    #[test]
    fn test_subdivide_groups_small() {
        let group = FileGroup {
            files: vec![DataFileInfo {
                path: "f1.parquet".to_string(),
                size: 100,
                record_count: 10,
                partition: HashMap::new(),
            }],
            total_size: 100,
            total_records: 10,
            partition_key: "date=2024-01-01".to_string(),
        };

        let result = subdivide_groups(vec![group], 100);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].partition_key, "date=2024-01-01");
    }

    #[test]
    fn test_subdivide_groups_large() {
        let files: Vec<DataFileInfo> = (0..10)
            .map(|i| DataFileInfo {
                path: format!("f{}.parquet", i),
                size: 100,
                record_count: 10,
                partition: HashMap::new(),
            })
            .collect();

        let group = FileGroup {
            files,
            total_size: 1000,
            total_records: 100,
            partition_key: "date=2024-01-01".to_string(),
        };

        let result = subdivide_groups(vec![group], 3);
        assert_eq!(result.len(), 4); // 10 files / 3 per group = 4 groups
        assert!(result[0].partition_key.contains("__subgroup_0"));
    }

    #[test]
    fn test_parse_partition_key() {
        let partition = parse_partition_key("date=2024-01-01/region=us");
        assert_eq!(partition.get("date"), Some(&"2024-01-01".to_string()));
        assert_eq!(partition.get("region"), Some(&"us".to_string()));
    }

    #[test]
    fn test_parse_partition_key_empty() {
        let partition = parse_partition_key("");
        assert!(partition.is_empty());
    }

    #[test]
    fn test_path_to_object_path_s3() {
        let result = path_to_object_path("s3://bucket/path/to/file.parquet", "/local/table");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_ref(), "path/to/file.parquet");
    }

    #[test]
    fn test_path_to_object_path_local() {
        let result = path_to_object_path("/data/table/data/file.parquet", "/data/table");
        assert!(result.is_ok());
    }
}

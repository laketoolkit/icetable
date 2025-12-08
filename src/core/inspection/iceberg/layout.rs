//! Layout and statistics extraction for Iceberg tables

use std::collections::HashMap;

use super::manifest::normalize_path;
use super::parse_summary_value;
use crate::core::inspection::traits::{
    FileBasedLayout, LayoutInfo, PhysicalInspectOptions, StatisticsInfo, VerbosityLevel,
};
use crate::core::storage::Storage;
use crate::error::{Error, Result};

/// Extract layout information from Iceberg metadata
pub fn extract_layout_info(
    metadata: &serde_json::Value,
    options: &PhysicalInspectOptions,
    extract_sort_order: impl Fn(&serde_json::Value) -> String,
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

    if current_snapshot_id != -1
        && let Some(snapshots) = metadata.get("snapshots").and_then(|s| s.as_array())
        && let Some(snapshot) = snapshots.iter().find(|s| {
            s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_snapshot_id)
        })
        && let Some(summary) = snapshot.get("summary").and_then(|s| s.as_object())
    {
        num_files = parse_summary_value::<usize>(summary.get("total-data-files")).unwrap_or(0);
        total_size = parse_summary_value::<u64>(summary.get("total-files-size")).unwrap_or(0);

        // Extract delta info (files added/deleted in last snapshot)
        let added_files =
            parse_summary_value::<i64>(summary.get("added-data-files")).unwrap_or(0);
        let deleted_files =
            parse_summary_value::<i64>(summary.get("deleted-data-files")).unwrap_or(0);

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
            let total_delete_files =
                parse_summary_value::<i64>(summary.get("total-delete-files")).unwrap_or(0);
            let equality_deletes =
                parse_summary_value::<i64>(summary.get("total-equality-deletes")).unwrap_or(0);
            let position_deletes =
                parse_summary_value::<i64>(summary.get("total-position-deletes")).unwrap_or(0);

            details.insert(
                "Delete Files".to_string(),
                format!(
                    "{} (position: {}, equality: {})",
                    total_delete_files, position_deletes, equality_deletes
                ),
            );

            // Try to get manifest count from snapshot
            if let Some(manifest_list) = snapshot.get("manifest-list").and_then(|m| m.as_str()) {
                details.insert(
                    "Manifest List".to_string(),
                    manifest_list
                        .split('/')
                        .next_back()
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
                    format!("{:.2} MB (below target: {:.0} MB)", avg_size_mb, target_mb)
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
            details.insert("File Format".to_string(), format!("{} ({})", format, codec));
        }
    }

    // Sort order info (always show in verbose, or if actually sorted)
    let sort_order_str = extract_sort_order(metadata);
    if options.verbosity >= VerbosityLevel::Verbose || sort_order_str != "(unsorted)" {
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

/// Extract statistics from Iceberg metadata
pub async fn extract_statistics(
    storage: &Storage,
    table_path: &str,
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

    if let Some(snapshot) = snapshot
        && let Some(summary) = snapshot.get("summary").and_then(|s| s.as_object())
    {
        let total_rows = parse_summary_value::<i64>(summary.get("total-records")).unwrap_or(0);
        let compressed_size =
            parse_summary_value::<u64>(summary.get("total-files-size")).unwrap_or(0);

        // If verbose mode, try to read manifest stats
        if options.verbosity >= VerbosityLevel::Verbose
            && let Some(manifest_list) = snapshot.get("manifest-list").and_then(|ml| ml.as_str())
        {
            let table_location = metadata
                .get("location")
                .and_then(|l| l.as_str())
                .unwrap_or(table_path);

            let manifest_list_path = normalize_path(manifest_list, table_location);

            // Try to read manifest stats, but don't fail if it doesn't work
            let _ = super::manifest::read_manifest_stats(storage, table_location, &manifest_list_path).await;
        }

        return Ok(StatisticsInfo {
            total_rows,
            compressed_size,
            uncompressed_size: compressed_size, // Iceberg doesn't track uncompressed separately
            column_stats: vec![],
        });
    }

    Ok(StatisticsInfo {
        total_rows: 0,
        compressed_size: 0,
        uncompressed_size: 0,
        column_stats: vec![],
    })
}

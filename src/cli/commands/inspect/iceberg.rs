//! Iceberg inspection

use std::path::Path;
use std::sync::Arc;
use std::collections::HashMap;

use colored::Colorize;
use crate::cli::output::BoxItem;
use crate::core::storage::StorageBackend;
use crate::error::Result;

use super::common::*;

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

/// Inspect Iceberg table
#[cfg(feature = "iceberg")]
pub async fn inspect_iceberg_layout(
    path: &Path,
    storage: Arc<dyn StorageBackend>,
    options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    use crate::core::storage::traits::{GetOptions, ListOptions};

    // Find and read the latest metadata file
    let metadata_prefix = format!("{}/metadata/", path.to_str().unwrap_or(""));

    let list_opts = ListOptions {
        prefix: Some(metadata_prefix.clone()),
        delimiter: None,
        max_results: Some(100),
        continuation_token: None,
    };

    let files = storage.list(&list_opts).await?;

    // Find the latest v*.metadata.json file
    let metadata_file = files.objects.iter()
        .filter(|obj| obj.path.contains(".metadata.json"))
        .max_by_key(|obj| obj.last_modified)
        .ok_or_else(|| crate::error::Error::General(
            "No metadata.json file found in metadata/ directory".to_string()
        ))?;

    log::debug!("Reading Iceberg metadata from: {}", metadata_file.path);

    // Read the metadata file
    let get_opts = GetOptions {
        range: None,
        if_modified_since: None,
        if_none_match: None,
    };
    let metadata_bytes = storage.get(&metadata_file.path, &get_opts).await?;
    let metadata_str = String::from_utf8(metadata_bytes.to_vec())
        .map_err(|e| crate::error::Error::General(format!("Invalid UTF-8 in metadata file: {}", e)))?;

    // Parse JSON
    let metadata: serde_json::Value = serde_json::from_str(&metadata_str)
        .map_err(|e| crate::error::Error::General(format!("Failed to parse metadata JSON: {}", e)))?;

    // Build file info section
    let format_version = metadata.get("format-version")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);

    let table_uuid = metadata.get("table-uuid")
        .and_then(|u| u.as_str())
        .unwrap_or("unknown");

    let num_snapshots = metadata.get("snapshots")
        .and_then(|s| s.as_array())
        .map(|arr| arr.len())
        .unwrap_or(0);

    let current_snapshot_id = metadata.get("current-snapshot-id")
        .and_then(|id| id.as_i64())
        .unwrap_or(-1);

    let location = metadata.get("location")
        .and_then(|l| l.as_str())
        .unwrap_or(path.to_str().unwrap_or(""));

    let mut file_info = vec![
        kv_item("Path", path.display().to_string(), 20),
        kv_item("Format", "Apache Iceberg", 20),
        kv_item("Format Version", format_version, 20),
        kv_item("Table UUID", table_uuid, 20),
        kv_item("Current Snapshot ID", if current_snapshot_id == -1 { "None".to_string() } else { current_snapshot_id.to_string() }, 20),
        kv_item("Total Snapshots", num_snapshots, 20),
    ];

    // Add verbose info
    if options.verbosity >= VerbosityLevel::Verbose {
        file_info.push(kv_item("Location", location, 20));
        file_info.push(kv_item("Metadata Location", &metadata_file.path, 20));
    }

    // Build schema section
    let schema_items = build_schema_section(&metadata, options)?;

    // Build partitioning section
    let partition_items = build_partitioning_section(&metadata, options)?;

    // Build current snapshot section
    let snapshot_items = build_current_snapshot_section(&metadata, options)?;

    // Build statistics section
    let stats_items = build_statistics_section(&metadata, location, storage.clone(), options).await?;

    // Build sort order (verbose only)
    let sort_order_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_sort_order(&metadata)?)
    } else {
        None
    };

    // Build snapshot history (verbose only)
    let snapshot_history_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_snapshot_history(&metadata)?)
    } else {
        None
    };

    // Build metadata history (verbose only)
    let metadata_history_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_metadata_history(&metadata)?)
    } else {
        None
    };

    // Build manifests (verbose only)
    let manifests_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_manifests_section(&metadata, location, storage.clone()).await?)
    } else {
        None
    };

    // Build table properties (verbose only)
    let properties_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_table_properties(&metadata)?)
    } else {
        None
    };

    // Combine all sections
    let mut all_items = file_info;
    all_items.push(BoxItem::Empty);
    all_items.extend(schema_items);
    all_items.push(BoxItem::Empty);
    all_items.extend(partition_items);

    if let Some(sort_order) = sort_order_items {
        all_items.push(BoxItem::Empty);
        all_items.extend(sort_order);
    }

    all_items.push(BoxItem::Empty);
    all_items.extend(snapshot_items);

    if let Some(snapshot_history) = snapshot_history_items {
        all_items.push(BoxItem::Empty);
        all_items.extend(snapshot_history);
    }

    all_items.push(BoxItem::Empty);
    all_items.extend(stats_items);

    if let Some(metadata_history) = metadata_history_items {
        all_items.push(BoxItem::Empty);
        all_items.extend(metadata_history);
    }

    if let Some(manifests) = manifests_items {
        all_items.push(BoxItem::Empty);
        all_items.extend(manifests);
    }

    if let Some(properties) = properties_items {
        all_items.push(BoxItem::Empty);
        all_items.extend(properties);
    }

    Ok(PhysicalInspectResult {
        file_info: all_items,
        schema: None,
        layout: None,
        statistics: None,
        stats_title: None,
    })
}

#[cfg(feature = "iceberg")]
fn build_schema_section(metadata: &serde_json::Value, _options: &PhysicalInspectOptions) -> Result<Vec<BoxItem>> {
    let schema = metadata.get("schema")
        .or_else(|| metadata.get("schemas").and_then(|s| s.as_array()).and_then(|arr| arr.first()))
        .ok_or_else(|| crate::error::Error::General("No schema found in metadata".to_string()))?;

    let schema_id = schema.get("schema-id")
        .and_then(|id| id.as_i64())
        .unwrap_or(0);

    let fields = schema.get("fields")
        .and_then(|f| f.as_array())
        .ok_or_else(|| crate::error::Error::General("No fields found in schema".to_string()))?;

    let mut items = vec![
        text_item(format!("═══ {} ═══", "Schema".bold())),
        BoxItem::Empty,
        kv_item("Schema ID", schema_id, 20),
        kv_item("Columns", fields.len(), 20),
        BoxItem::Empty,
    ];

    for (idx, field) in fields.iter().enumerate() {
        let name = field.get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");

        let field_type = field.get("type")
            .and_then(|t| t.as_str())
            .or_else(|| field.get("type").and_then(|t| t.as_object()).map(|_| "COMPLEX"))
            .unwrap_or("unknown");

        let required = field.get("required")
            .and_then(|r| r.as_bool())
            .unwrap_or(false);

        let nullable_str = if required { "NOT NULL" } else { "" };

        items.push(text_item(format!(
            "  {}.  {:<30} {:<12} {}",
            idx + 1,
            name.bold(),
            field_type.to_uppercase(),
            nullable_str
        )));
    }

    Ok(items)
}

#[cfg(feature = "iceberg")]
fn build_partitioning_section(metadata: &serde_json::Value, options: &PhysicalInspectOptions) -> Result<Vec<BoxItem>> {
    let partition_spec = metadata.get("partition-spec")
        .or_else(|| metadata.get("partition-specs").and_then(|s| s.as_array()).and_then(|arr| arr.first()))
        .ok_or_else(|| crate::error::Error::General("No partition spec found in metadata".to_string()))?;

    let spec_id = partition_spec.get("spec-id")
        .and_then(|id| id.as_i64())
        .unwrap_or(0);

    let fields_opt = partition_spec.get("fields")
        .and_then(|f| f.as_array());

    let mut items = vec![
        text_item(format!("═══ {} ═══", "Partitioning".bold())),
        BoxItem::Empty,
        kv_item("Spec ID", spec_id, 20),
        kv_item("Partition Fields", fields_opt.map(|f| f.len()).unwrap_or(0), 20),
        BoxItem::Empty,
    ];

    if let Some(fields) = fields_opt {
        if !fields.is_empty() {
            for (idx, field) in fields.iter().enumerate() {
                let name = field.get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown");

                if options.verbosity >= VerbosityLevel::Verbose {
                    // Verbose mode: show detailed partition information
                    let field_id = field.get("field-id")
                        .and_then(|id| id.as_i64())
                        .unwrap_or(0);

                    let source_id = field.get("source-id")
                        .and_then(|id| id.as_i64())
                        .unwrap_or(0);

                    let transform = field.get("transform")
                        .and_then(|t| t.as_str())
                        .unwrap_or("identity");

                    items.push(text_item(format!(
                        "  {}.  {} (Field ID: {}, Source: {}, Transform: {})",
                        idx + 1,
                        name.bold(),
                        field_id,
                        source_id,
                        transform
                    )));
                } else {
                    // Normal mode: just show partition field name
                    items.push(text_item(format!("  {}.  {}", idx + 1, name.bold())));
                }
            }
        } else {
            items.push(text_item("  Table is not partitioned"));
        }
    } else {
        items.push(text_item("  Table is not partitioned"));
    }

    Ok(items)
}

#[cfg(feature = "iceberg")]
fn build_current_snapshot_section(metadata: &serde_json::Value, options: &PhysicalInspectOptions) -> Result<Vec<BoxItem>> {
    let current_snapshot_id = metadata.get("current-snapshot-id")
        .and_then(|id| id.as_i64())
        .unwrap_or(-1);

    let mut items = vec![
        text_item(format!("═══ {} ═══", "Current Snapshot".bold())),
        BoxItem::Empty,
    ];

    if current_snapshot_id == -1 {
        items.push(text_item("  No snapshots yet (empty table)"));
    } else {
        items.push(kv_item("Snapshot ID", current_snapshot_id, 20));

        // Try to find the snapshot in the snapshots array
        if let Some(snapshots) = metadata.get("snapshots").and_then(|s| s.as_array()) {
            if let Some(snapshot) = snapshots.iter().find(|s| {
                s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_snapshot_id)
            }) {
                // Add parent snapshot ID if present
                if let Some(parent_id) = snapshot.get("parent-snapshot-id").and_then(|id| id.as_i64()) {
                    items.push(kv_item("Parent Snapshot ID", parent_id, 20));
                } else {
                    items.push(kv_item("Parent Snapshot ID", "None", 20));
                }

                // Add partition spec ID
                if let Some(spec_id) = snapshot.get("schema-id").and_then(|id| id.as_i64()) {
                    items.push(kv_item("Partition Spec ID", spec_id, 20));
                }

                // Add timestamp
                if let Some(timestamp_ms) = snapshot.get("timestamp-ms").and_then(|ts| ts.as_i64()) {
                    let timestamp_dt = chrono::DateTime::from_timestamp_millis(timestamp_ms)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| "Unknown".to_string());
                    items.push(kv_item("Timestamp", timestamp_dt, 20));
                }

                // Add operation from summary
                if let Some(summary) = snapshot.get("summary").and_then(|s| s.as_object()) {
                    if let Some(operation) = summary.get("operation").and_then(|o| o.as_str()) {
                        items.push(kv_item("Operation", operation, 20));
                    }

                    // Add Files, Rows, Size in normal mode
                    if let Some(total_files) = summary.get("total-data-files")
                        .and_then(|f| f.as_str())
                        .and_then(|f| f.parse::<i64>().ok())
                    {
                        items.push(kv_item("Files", format_number(total_files), 20));
                    }

                    if let Some(total_records) = summary.get("total-records")
                        .and_then(|r| r.as_str())
                        .and_then(|r| r.parse::<i64>().ok())
                    {
                        items.push(kv_item("Rows", format_number(total_records), 20));
                    }

                    if let Some(total_size) = summary.get("total-files-size")
                        .and_then(|s| s.as_str())
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        items.push(kv_item("Size", format_bytes(total_size), 20));
                    }
                }

                // Add manifest list location
                if let Some(manifest_list) = snapshot.get("manifest-list").and_then(|ml| ml.as_str()) {
                    items.push(kv_item("Manifest List", manifest_list, 20));
                }

                // Verbose mode additions
                if options.verbosity >= VerbosityLevel::Verbose {
                    // Add Schema ID and Partition Spec ID
                    if let Some(schema_id) = snapshot.get("schema-id").and_then(|id| id.as_i64()) {
                        items.push(BoxItem::Empty);
                        items.push(kv_item("Schema ID", schema_id, 20));
                    }

                    if let Some(spec_id) = snapshot.get("partition-spec-id").and_then(|id| id.as_i64()) {
                        items.push(kv_item("Partition Spec ID", spec_id, 20));
                    }

                    // Add detailed summary properties
                    if let Some(summary) = snapshot.get("summary").and_then(|s| s.as_object()) {
                        items.push(BoxItem::Empty);
                        items.push(text_item("Summary Properties:"));

                        if let Some(added_files) = summary.get("added-data-files").and_then(|f| f.as_str()) {
                            items.push(text_item(format!("  added-data-files           {}", added_files)));
                        }
                        if let Some(added_records) = summary.get("added-records").and_then(|r| r.as_str()) {
                            items.push(text_item(format!("  added-records              {}", added_records)));
                        }
                        if let Some(added_size) = summary.get("added-files-size").and_then(|s| s.as_str()) {
                            items.push(text_item(format!("  added-files-size           {}", added_size)));
                        }
                        if let Some(deleted_files) = summary.get("deleted-data-files").and_then(|f| f.as_str()) {
                            items.push(text_item(format!("  deleted-data-files         {}", deleted_files)));
                        }
                        if let Some(deleted_records) = summary.get("deleted-records").and_then(|r| r.as_str()) {
                            items.push(text_item(format!("  deleted-records            {}", deleted_records)));
                        }
                    }
                }
            }
        }
    }

    Ok(items)
}

#[cfg(feature = "iceberg")]
async fn build_statistics_section(
    metadata: &serde_json::Value,
    table_location: &str,
    storage: Arc<dyn StorageBackend>,
    options: &PhysicalInspectOptions,
) -> Result<Vec<BoxItem>> {
    let mut items = vec![
        text_item(format!("═══ {} ═══", "Summary Statistics".bold())),
        BoxItem::Empty,
    ];

    // Get current snapshot if available
    let current_snapshot_id = metadata.get("current-snapshot-id")
        .and_then(|id| id.as_i64())
        .unwrap_or(-1);

    if current_snapshot_id == -1 {
        items.push(text_item("  No data yet (empty table)"));
        return Ok(items);
    }

    // Try to find the current snapshot in the snapshots array
    if let Some(snapshots) = metadata.get("snapshots").and_then(|s| s.as_array()) {
        if let Some(snapshot) = snapshots.iter().find(|s| {
            s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_snapshot_id)
        }) {
            // Extract summary statistics from snapshot
            if let Some(summary) = snapshot.get("summary").and_then(|s| s.as_object()) {
                // Total data files
                let total_files = summary.get("total-data-files")
                    .and_then(|f| f.as_str())
                    .and_then(|f| f.parse::<i64>().ok());

                if let Some(files) = total_files {
                    items.push(kv_item("Data Files", format_number(files), 20));
                }

                // Total delete files
                let total_delete_files = summary.get("total-delete-files")
                    .and_then(|f| f.as_str())
                    .and_then(|f| f.parse::<i64>().ok())
                    .unwrap_or(0);

                items.push(kv_item("Delete Files", format_number(total_delete_files), 20));

                // Total records
                if let Some(total_records) = summary.get("total-records")
                    .and_then(|r| r.as_str())
                    .and_then(|r| r.parse::<i64>().ok())
                {
                    items.push(kv_item("Total Records", format_number(total_records), 20));
                }

                // Total size
                let total_size = summary.get("total-files-size")
                    .and_then(|s| s.as_str())
                    .and_then(|s| s.parse::<u64>().ok());

                if let Some(size) = total_size {
                    items.push(kv_item("Total Size", format_bytes(size), 20));

                    // Calculate average file size
                    if let Some(files) = total_files {
                        if files > 0 {
                            let avg_size = size / files as u64;
                            items.push(kv_item("Avg File Size", format_bytes(avg_size), 20));
                        }
                    }
                }

                // Verbose mode: read manifest files for detailed stats
                if options.verbosity >= VerbosityLevel::Verbose {
                    if let Some(manifest_list) = snapshot.get("manifest-list").and_then(|ml| ml.as_str()) {
                        // Resolve manifest list path
                        let manifest_list_path = if manifest_list.starts_with("s3://")
                            || manifest_list.starts_with("gs://")
                            || manifest_list.starts_with("abfs://")
                            || manifest_list.starts_with("file://") {
                            manifest_list.to_string()
                        } else {
                            format!("{}/{}", table_location.trim_end_matches('/'), manifest_list.trim_start_matches('/'))
                        };

                        if let Ok(manifest_stats) = read_manifest_stats(storage, table_location, &manifest_list_path).await {
                            items.push(BoxItem::Empty);

                            if let Some(min) = manifest_stats.min_file_size {
                                items.push(kv_item("Min File Size", format_bytes(min as u64), 20));
                            }
                            if let Some(max) = manifest_stats.max_file_size {
                                items.push(kv_item("Max File Size", format_bytes(max as u64), 20));
                            }

                            // File format distribution
                            if !manifest_stats.file_format_counts.is_empty() {
                                items.push(BoxItem::Empty);
                                items.push(text_item("File Format Distribution:"));
                                let total_format_files: i64 = manifest_stats.file_format_counts.values().sum();
                                for (format, count) in manifest_stats.file_format_counts.iter() {
                                    let percentage = if total_format_files > 0 {
                                        (*count as f64 / total_format_files as f64) * 100.0
                                    } else {
                                        0.0
                                    };
                                    items.push(text_item(format!(
                                        "  {:<10} {} ({:.1}%)",
                                        format,
                                        format_number(*count),
                                        percentage
                                    )));
                                }
                            }

                            // Per-partition statistics (top 10 by files)
                            if !manifest_stats.partition_stats.is_empty() {
                                items.push(BoxItem::Empty);
                                items.push(text_item("Per-Partition Statistics (top 10 by files):"));

                                let mut partitions: Vec<_> = manifest_stats.partition_stats.iter().collect();
                                partitions.sort_by(|a, b| b.1.files.cmp(&a.1.files));

                                for (idx, (partition_key, info)) in partitions.iter().take(10).enumerate() {
                                    // Display "(Unpartitioned)" for empty partition keys
                                    let display_key = if partition_key.trim() == "{}" || partition_key.trim().is_empty() {
                                        "(Unpartitioned)".to_string()
                                    } else {
                                        partition_key.to_string()
                                    };
                                    items.push(text_item(format!("  {}. Partition {}", idx + 1, display_key)));
                                    items.push(text_item(format!("     Files: {}, Records: {}, Size: {}",
                                        format_number(info.files),
                                        format_number(info.records),
                                        format_bytes(info.size as u64)
                                    )));
                                }

                                if manifest_stats.partition_stats.len() > 10 {
                                    items.push(text_item(format!("     ... and {} more partitions",
                                        manifest_stats.partition_stats.len() - 10)));
                                }
                            }
                        }
                    }
                }

                // Total position deletes
                if let Some(total_deletes) = summary.get("total-position-deletes")
                    .and_then(|d| d.as_str())
                    .and_then(|d| d.parse::<i64>().ok())
                {
                    if total_deletes > 0 {
                        items.push(BoxItem::Empty);
                        items.push(kv_item("Position Deletes", format_number(total_deletes), 20));
                    }
                }

                // Total equality deletes
                if let Some(total_eq_deletes) = summary.get("total-equality-deletes")
                    .and_then(|d| d.as_str())
                    .and_then(|d| d.parse::<i64>().ok())
                {
                    if total_eq_deletes > 0 {
                        items.push(kv_item("Equality Deletes", format_number(total_eq_deletes), 20));
                    }
                }

                return Ok(items);
            }
        }
    }

    // If no summary available
    items.push(text_item("  No statistics available in snapshot"));
    Ok(items)
}

#[cfg(feature = "iceberg")]
fn build_sort_order(metadata: &serde_json::Value) -> Result<Vec<BoxItem>> {
    let mut items = vec![
        text_item(format!("═══ {} ═══", "Sort Order".bold())),
        BoxItem::Empty,
    ];

    // Get the default sort order
    let default_sort_order_id = metadata.get("default-sort-order-id")
        .and_then(|id| id.as_i64())
        .unwrap_or(0);

    items.push(kv_item("Default Sort Order ID", default_sort_order_id, 25));

    // Get sort orders array
    if let Some(sort_orders) = metadata.get("sort-orders").and_then(|s| s.as_array()) {
        if let Some(sort_order) = sort_orders.iter().find(|s| {
            s.get("order-id").and_then(|id| id.as_i64()) == Some(default_sort_order_id)
        }) {
            if let Some(fields) = sort_order.get("fields").and_then(|f| f.as_array()) {
                items.push(kv_item("Sort Fields", fields.len(), 25));
                items.push(BoxItem::Empty);

                if fields.is_empty() {
                    items.push(text_item("  No sort order defined (unsorted table)"));
                } else {
                    for (idx, field) in fields.iter().enumerate() {
                        let source_id = field.get("source-id")
                            .and_then(|id| id.as_i64())
                            .unwrap_or(0);

                        let direction = field.get("direction")
                            .and_then(|d| d.as_str())
                            .unwrap_or("asc");

                        let null_order = field.get("null-order")
                            .and_then(|n| n.as_str())
                            .unwrap_or("nulls-first");

                        items.push(text_item(format!(
                            "  {}.  Field ID: {}, Direction: {}, Null Order: {}",
                            idx + 1,
                            source_id,
                            direction.to_uppercase(),
                            null_order
                        )));
                    }
                }
            }
        }
    } else {
        items.push(text_item("  No sort order information available"));
    }

    Ok(items)
}

#[cfg(feature = "iceberg")]
fn build_snapshot_history(metadata: &serde_json::Value) -> Result<Vec<BoxItem>> {
    let mut items = vec![
        text_item(format!("═══ {} ═══", "Snapshot History".bold())),
        BoxItem::Empty,
    ];

    if let Some(snapshots) = metadata.get("snapshots").and_then(|s| s.as_array()) {
        if snapshots.is_empty() {
            items.push(text_item("  No snapshots yet"));
            return Ok(items);
        }

        let current_snapshot_id = metadata.get("current-snapshot-id")
            .and_then(|id| id.as_i64());

        // Show last 5-10 snapshots (limit to last 10)
        let start_idx = if snapshots.len() > 10 {
            snapshots.len() - 10
        } else {
            0
        };

        items.push(text_item(format!(
            "Recent Snapshots (showing {} of {}):",
            snapshots.len() - start_idx,
            snapshots.len()
        )));
        items.push(BoxItem::Empty);

        for snapshot in snapshots.iter().skip(start_idx) {
            let snapshot_id = snapshot.get("snapshot-id")
                .and_then(|id| id.as_i64())
                .unwrap_or(0);

            let is_current = current_snapshot_id == Some(snapshot_id);
            let marker = if is_current { "→" } else { " " };

            let timestamp_ms = snapshot.get("timestamp-ms")
                .and_then(|ts| ts.as_i64())
                .unwrap_or(0);

            let timestamp_str = chrono::DateTime::from_timestamp_millis(timestamp_ms)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            let operation = snapshot.get("summary")
                .and_then(|s| s.as_object())
                .and_then(|s| s.get("operation"))
                .and_then(|o| o.as_str())
                .unwrap_or("unknown");

            items.push(text_item(format!(
                "  {} Snapshot {} - {} - {}",
                marker,
                format!("{}", snapshot_id).bold(),
                timestamp_str,
                operation
            )));

            // Add parent ID
            if let Some(parent_id) = snapshot.get("parent-snapshot-id").and_then(|id| id.as_i64()) {
                items.push(text_item(format!("      Parent: {}", parent_id)));
            } else {
                items.push(text_item("      Parent: None"));
            }

            // Add summary details with deltas if available
            if let Some(summary) = snapshot.get("summary").and_then(|s| s.as_object()) {
                // Records delta
                if let Some(added) = summary.get("added-records").and_then(|r| r.as_str()).and_then(|s| s.parse::<i64>().ok()) {
                    items.push(text_item(format!("      Records: +{}", format_number(added))));
                } else if let Some(records) = summary.get("total-records").and_then(|r| r.as_str()).and_then(|s| s.parse::<i64>().ok()) {
                    items.push(text_item(format!("      Records: +{}", format_number(records))));
                }

                // Files delta
                let added_files = summary.get("added-data-files").and_then(|f| f.as_str()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                let removed_files = summary.get("deleted-data-files").and_then(|f| f.as_str()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);

                if added_files > 0 || removed_files > 0 {
                    if removed_files == 0 {
                        items.push(text_item(format!("      Files: +{}", added_files)));
                    } else {
                        items.push(text_item(format!("      Files: +{}, -{}", added_files, removed_files)));
                    }
                }

                // Size delta
                let added_size = summary.get("added-files-size").and_then(|s| s.as_str()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let removed_size = summary.get("removed-files-size").and_then(|s| s.as_str()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);

                if added_size > 0 || removed_size > 0 {
                    if removed_size == 0 {
                        items.push(text_item(format!("      Size: +{}", format_bytes(added_size))));
                    } else {
                        items.push(text_item(format!("      Size: +{}, -{}", format_bytes(added_size), format_bytes(removed_size))));
                    }
                }
            }
        }
    } else {
        items.push(text_item("  No snapshot history available"));
    }

    Ok(items)
}

#[cfg(feature = "iceberg")]
fn build_metadata_history(metadata: &serde_json::Value) -> Result<Vec<BoxItem>> {
    let mut items = vec![
        text_item(format!("═══ {} ═══", "Metadata History".bold())),
        BoxItem::Empty,
    ];

    if let Some(metadata_log) = metadata.get("metadata-log").and_then(|m| m.as_array()) {
        if metadata_log.is_empty() {
            items.push(text_item("  No previous metadata files"));
            return Ok(items);
        }

        items.push(text_item(format!("Metadata Files ({} previous):", metadata_log.len())));
        items.push(BoxItem::Empty);

        for (idx, entry) in metadata_log.iter().enumerate() {
            let timestamp_ms = entry.get("timestamp-ms")
                .and_then(|ts| ts.as_i64())
                .unwrap_or(0);

            let timestamp_str = chrono::DateTime::from_timestamp_millis(timestamp_ms)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            let metadata_file = entry.get("metadata-file")
                .and_then(|f| f.as_str())
                .unwrap_or("unknown");

            items.push(text_item(format!(
                "  {}.  {} - {}",
                idx + 1,
                timestamp_str,
                metadata_file
            )));
        }

        // Add current metadata file
        items.push(BoxItem::Empty);
        items.push(text_item("  Current metadata file is the active one"));
    } else {
        items.push(text_item("  No metadata history available"));
    }

    Ok(items)
}

#[cfg(feature = "iceberg")]
async fn build_manifests_section(
    metadata: &serde_json::Value,
    table_location: &str,
    storage: Arc<dyn StorageBackend>,
) -> Result<Vec<BoxItem>> {
    let mut items = vec![
        text_item(format!("═══ {} ═══", "Manifests".bold())),
        BoxItem::Empty,
    ];

    // Get current snapshot
    let current_snapshot_id = metadata.get("current-snapshot-id")
        .and_then(|id| id.as_i64())
        .unwrap_or(-1);

    if current_snapshot_id == -1 {
        items.push(text_item("  No snapshots yet"));
        return Ok(items);
    }

    // Find the current snapshot
    if let Some(snapshots) = metadata.get("snapshots").and_then(|s| s.as_array()) {
        if let Some(snapshot) = snapshots.iter().find(|s| {
            s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_snapshot_id)
        }) {
            // Get manifest list location
            if let Some(manifest_list) = snapshot.get("manifest-list").and_then(|ml| ml.as_str()) {
                items.push(kv_item("Manifest List", manifest_list, 25));

                // Resolve manifest list path (may be relative to table location)
                let manifest_list_path = if manifest_list.starts_with("s3://")
                    || manifest_list.starts_with("gs://")
                    || manifest_list.starts_with("abfs://")
                    || manifest_list.starts_with("file://") {
                    manifest_list.to_string()
                } else {
                    format!("{}/{}", table_location.trim_end_matches('/'), manifest_list.trim_start_matches('/'))
                };

                log::debug!("Manifest list from metadata: {}", manifest_list);
                log::debug!("Resolved manifest list path: {}", manifest_list_path);

                // List files in metadata directory for debugging
                if let Ok(list_result) = storage.list(&crate::core::storage::traits::ListOptions {
                    prefix: Some(format!("{}/metadata/", table_location.trim_end_matches('/'))),
                    delimiter: None,
                    max_results: Some(50),
                    continuation_token: None,
                }).await {
                    log::debug!("Files in metadata directory:");
                    for obj in list_result.objects.iter() {
                        log::debug!("  - {}", obj.path);
                    }
                }

                // Read and parse manifest statistics
                match read_manifest_stats(storage.clone(), table_location, &manifest_list_path).await {
                    Ok(manifest_stats) => {
                        items.push(BoxItem::Empty);
                        items.push(kv_item("Total Data Files", format_number(manifest_stats.total_files), 25));

                        if let Some(min) = manifest_stats.min_file_size {
                            items.push(kv_item("Min File Size", format_bytes(min as u64), 25));
                        }
                        if let Some(max) = manifest_stats.max_file_size {
                            items.push(kv_item("Max File Size", format_bytes(max as u64), 25));
                        }

                        // File formats
                        if !manifest_stats.file_format_counts.is_empty() {
                            items.push(BoxItem::Empty);
                            items.push(text_item("File Formats:"));
                            for (format, count) in manifest_stats.file_format_counts.iter() {
                                items.push(text_item(format!("  {:<10} {}", format, format_number(*count))));
                            }
                        }

                        // Partitions summary
                        if !manifest_stats.partition_stats.is_empty() {
                            items.push(BoxItem::Empty);
                            items.push(text_item(format!("Partitions: {} unique partitions",
                                manifest_stats.partition_stats.len())));
                        }
                    }
                    Err(e) => {
                        items.push(BoxItem::Empty);
                        items.push(text_item(format!("  (Could not read manifest files: {})", e)));
                        items.push(text_item("  Note: Manifest parsing is experimental and may not work with all Iceberg versions"));
                    }
                }
            } else {
                items.push(text_item("  No manifest list found in snapshot"));
            }
        }
    }

    Ok(items)
}

#[cfg(feature = "iceberg")]
fn build_table_properties(metadata: &serde_json::Value) -> Result<Vec<BoxItem>> {
    let properties = metadata.get("properties")
        .and_then(|p| p.as_object());

    let mut items = vec![
        text_item(format!("═══ {} ═══", "Table Properties".bold())),
        BoxItem::Empty,
    ];

    if let Some(props) = properties {
        if props.is_empty() {
            items.push(text_item("  No properties set"));
        } else {
            for (key, value) in props.iter() {
                let value_str = value.as_str()
                    .unwrap_or_else(|| value.as_i64().map(|_| "N/A").unwrap_or("N/A"));
                items.push(kv_item(key, value_str, 35));
            }
        }
    } else {
        items.push(text_item("  No properties set"));
    }

    Ok(items)
}

#[cfg(feature = "iceberg")]
async fn read_manifest_stats(
    storage: Arc<dyn StorageBackend>,
    table_location: &str,
    manifest_list_path: &str,
) -> Result<ManifestStats> {
    use crate::core::storage::traits::GetOptions;
    use apache_avro::Reader;

    let mut stats = ManifestStats::default();

    // Read the manifest list file
    let get_opts = GetOptions {
        range: None,
        if_modified_since: None,
        if_none_match: None,
    };

    let manifest_list_bytes = storage.get(manifest_list_path, &get_opts).await
        .map_err(|e| crate::error::Error::General(format!("Failed to read manifest list: {}", e)))?;

    // Parse the manifest list (Avro format)
    let manifest_list_reader = Reader::new(&manifest_list_bytes[..])
        .map_err(|e| crate::error::Error::General(format!("Failed to parse manifest list: {}", e)))?;

    // Read each manifest entry
    for value_result in manifest_list_reader {
        let value = value_result
            .map_err(|e| crate::error::Error::General(format!("Failed to read manifest entry: {}", e)))?;

        // Extract manifest path from the Avro record
        if let apache_avro::types::Value::Record(fields) = value {
            log::debug!("Manifest list record fields: {:?}", fields.iter().map(|(name, _)| name).collect::<Vec<_>>());

            let manifest_path = fields.iter()
                .find(|(name, _)| name == "manifest-path" || name == "manifest_path")
                .and_then(|(_, v)| {
                    if let apache_avro::types::Value::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                });

            if let Some(path) = manifest_path {
                // Resolve manifest path (may be relative to table location)
                let full_manifest_path = if path.starts_with("s3://")
                    || path.starts_with("gs://")
                    || path.starts_with("abfs://")
                    || path.starts_with("file://") {
                    path
                } else {
                    format!("{}/{}", table_location.trim_end_matches('/'), path.trim_start_matches('/'))
                };

                // Read individual manifest file
                if let Ok(manifest_bytes) = storage.get(&full_manifest_path, &get_opts).await {
                    if let Ok(manifest_reader) = Reader::new(&manifest_bytes[..]) {
                        // Process each data file entry in the manifest
                        for data_file_result in manifest_reader {
                            if let Ok(apache_avro::types::Value::Record(data_fields)) = data_file_result {
                                log::debug!("Manifest entry fields: {:?}", data_fields.iter().map(|(name, _)| name).collect::<Vec<_>>());

                                // Try to extract data_file nested record
                                let data_file_record = data_fields.iter()
                                    .find(|(name, _)| name == "data_file" || name == "data-file")
                                    .and_then(|(_, v)| {
                                        if let apache_avro::types::Value::Record(fields) = v {
                                            Some(fields)
                                        } else {
                                            None
                                        }
                                    });

                                if let Some(df_fields) = data_file_record {
                                    log::debug!("Data file fields: {:?}", df_fields.iter().map(|(name, _)| name).collect::<Vec<_>>());
                                    stats.total_files += 1;

                                    // Extract file size from data_file record
                                    if let Some((_, apache_avro::types::Value::Long(size))) =
                                        df_fields.iter().find(|(name, _)| name == "file-size-in-bytes" || name == "file_size_in_bytes") {
                                        let size = *size;
                                        stats.min_file_size = Some(stats.min_file_size.map_or(size, |min| min.min(size)));
                                        stats.max_file_size = Some(stats.max_file_size.map_or(size, |max| max.max(size)));
                                    }

                                    // Extract file format from data_file record
                                    if let Some((_, format_value)) =
                                        df_fields.iter().find(|(name, _)| name == "file-format" || name == "file_format") {
                                        let format = match format_value {
                                            apache_avro::types::Value::Int(format_id) => {
                                                match format_id {
                                                    0 => "AVRO",
                                                    1 => "PARQUET",
                                                    2 => "ORC",
                                                    _ => "UNKNOWN",
                                                }
                                            }
                                            apache_avro::types::Value::String(s) => s.as_str(),
                                            _ => "UNKNOWN",
                                        };
                                        *stats.file_format_counts.entry(format.to_string()).or_insert(0) += 1;
                                    }

                                    // Extract partition data for per-partition stats
                                    if let Some((_, apache_avro::types::Value::Map(partition_data))) =
                                        df_fields.iter().find(|(name, _)| name == "partition") {
                                        let partition_key = format!("{:?}", partition_data);
                                        let entry = stats.partition_stats.entry(partition_key).or_default();
                                        entry.files += 1;

                                        if let Some((_, apache_avro::types::Value::Long(records))) =
                                            df_fields.iter().find(|(name, _)| name == "record-count" || name == "record_count") {
                                            entry.records += *records;
                                        }

                                        if let Some((_, apache_avro::types::Value::Long(size))) =
                                            df_fields.iter().find(|(name, _)| name == "file-size-in-bytes" || name == "file_size_in_bytes") {
                                            entry.size += *size;
                                        }
                                    }
                                } else {
                                    // Maybe the data file fields are directly in the record (not nested)
                                    log::debug!("No nested data_file found, trying direct fields");
                                    stats.total_files += 1;

                                    // Extract file size
                                    if let Some((_, apache_avro::types::Value::Long(size))) =
                                        data_fields.iter().find(|(name, _)| name == "file-size-in-bytes" || name == "file_size_in_bytes") {
                                        let size = *size;
                                        stats.min_file_size = Some(stats.min_file_size.map_or(size, |min| min.min(size)));
                                        stats.max_file_size = Some(stats.max_file_size.map_or(size, |max| max.max(size)));
                                    }

                                    // Extract file format
                                    if let Some((_, format_value)) =
                                        data_fields.iter().find(|(name, _)| name == "file-format" || name == "file_format") {
                                        let format = match format_value {
                                            apache_avro::types::Value::Int(format_id) => {
                                                match format_id {
                                                    0 => "AVRO",
                                                    1 => "PARQUET",
                                                    2 => "ORC",
                                                    _ => "UNKNOWN",
                                                }
                                            }
                                            apache_avro::types::Value::String(s) => s.as_str(),
                                            _ => "UNKNOWN",
                                        };
                                        *stats.file_format_counts.entry(format.to_string()).or_insert(0) += 1;
                                    }

                                    // Extract partition data for per-partition stats
                                    if let Some((_, apache_avro::types::Value::Map(partition_data))) =
                                        data_fields.iter().find(|(name, _)| name == "partition") {
                                        let partition_key = format!("{:?}", partition_data);
                                        let entry = stats.partition_stats.entry(partition_key).or_default();
                                        entry.files += 1;

                                        if let Some((_, apache_avro::types::Value::Long(records))) =
                                            data_fields.iter().find(|(name, _)| name == "record-count" || name == "record_count") {
                                            entry.records += *records;
                                        }

                                        if let Some((_, apache_avro::types::Value::Long(size))) =
                                            data_fields.iter().find(|(name, _)| name == "file-size-in-bytes" || name == "file_size_in_bytes") {
                                            entry.size += *size;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(stats)
}

#[cfg(not(feature = "iceberg"))]
pub async fn inspect_iceberg_layout(
    _path: &Path,
    _storage: Arc<dyn StorageBackend>,
    _options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    Err(crate::error::Error::General(
        "Iceberg support not enabled. Rebuild with --features iceberg".to_string(),
    ))
}

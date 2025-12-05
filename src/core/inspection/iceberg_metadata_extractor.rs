//! Iceberg metadata extraction utilities

use std::collections::HashMap;
use std::path::Path;

use crate::core::inspection::PhysicalInspectOptions;
use crate::core::inspection::traits::{ColumnInfo, FileInfo, VerbosityLevel};
use crate::error::{Error, Result};

/// Extract file information from Iceberg metadata
pub fn extract_file_info(
    path: &Path,
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
        .unwrap_or("");

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
    if let Some(snaps) = snapshots
        && let Some(current) = snaps
            .iter()
            .find(|s| s.get("snapshot-id").and_then(|id| id.as_i64()) == Some(current_snapshot_id))
        && let Some(ts) = current.get("timestamp-ms").and_then(|t| t.as_i64())
    {
        let datetime = chrono::DateTime::from_timestamp_millis(ts)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| ts.to_string());
        metadata_map.insert("Last Updated".to_string(), datetime);
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
        path: path.display().to_string(),
        file_size: 0, // Iceberg tables don't have a single file size
        format_version: format_version.to_string(),
        created_by: None,
        metadata: metadata_map,
    }
}

/// Extract schema information from Iceberg metadata
pub fn extract_schema(
    metadata: &serde_json::Value,
) -> Result<crate::core::inspection::traits::SchemaInfo> {
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

    Ok(crate::core::inspection::traits::SchemaInfo {
        num_columns: columns.len(),
        columns,
    })
}

/// Extract sort order from Iceberg metadata
pub fn extract_sort_order(metadata: &serde_json::Value) -> String {
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
            let _null_order = f
                .get("null-order")
                .and_then(|n| n.as_str())
                .unwrap_or("nulls-first");

            // Try to find column name from schema
            let col_name = schema_fields
                .and_then(|fields| {
                    fields
                        .iter()
                        .find(|field| field.get("id").and_then(|id| id.as_i64()) == Some(source_id))
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

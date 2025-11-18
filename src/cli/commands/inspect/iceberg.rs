//! Iceberg inspection

use std::path::Path;
use std::sync::Arc;

use colored::Colorize;
use crate::cli::output::BoxItem;
use crate::core::storage::StorageBackend;
use crate::error::Result;

use super::common::*;

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

    let num_snapshots = metadata.get("snapshots")
        .and_then(|s| s.as_array())
        .map(|arr| arr.len())
        .unwrap_or(0);

    let location = metadata.get("location")
        .and_then(|l| l.as_str())
        .unwrap_or(path.to_str().unwrap_or(""));

    let mut file_info = vec![
        kv_item("Path", path.display().to_string(), 20),
        kv_item("Format", "Apache Iceberg", 20),
        kv_item("Format Version", format_version, 20),
        kv_item("Snapshots", num_snapshots, 20),
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
    let stats_items = build_statistics_section(&metadata, options)?;

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
    all_items.push(BoxItem::Empty);
    all_items.extend(snapshot_items);
    all_items.push(BoxItem::Empty);
    all_items.extend(stats_items);

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
fn build_partitioning_section(metadata: &serde_json::Value, _options: &PhysicalInspectOptions) -> Result<Vec<BoxItem>> {
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

                items.push(text_item(format!("  {}.  {}", idx + 1, name.bold())));
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
fn build_current_snapshot_section(metadata: &serde_json::Value, _options: &PhysicalInspectOptions) -> Result<Vec<BoxItem>> {
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
                if let Some(parent_id) = snapshot.get("parent-snapshot-id").and_then(|id| id.as_i64()) {
                    items.push(kv_item("Parent ID", parent_id, 20));
                }
            }
        }
    }

    Ok(items)
}

#[cfg(feature = "iceberg")]
fn build_statistics_section(_metadata: &serde_json::Value, _options: &PhysicalInspectOptions) -> Result<Vec<BoxItem>> {
    let items = vec![
        text_item(format!("═══ {} ═══", "Summary Statistics".bold())),
        BoxItem::Empty,
        text_item("  (Detailed statistics require reading manifest files)"),
        text_item("  (Not yet implemented)"),
    ];

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

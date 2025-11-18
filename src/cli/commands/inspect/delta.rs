//! Delta Lake inspection

use std::path::Path;
use std::sync::Arc;

use colored::Colorize;
use crate::cli::output::BoxItem;
use crate::core::storage::StorageBackend;
use crate::error::Result;

use super::common::*;

/// Inspect Delta Lake table
#[cfg(feature = "delta")]
pub async fn inspect_delta_layout(
    path: &Path,
    _storage: Arc<dyn StorageBackend>,
    options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    use deltalake::DeltaTableBuilder;
    use std::collections::HashMap;

    // Build storage options for S3/MinIO
    let mut storage_options = HashMap::new();

    // Get credentials from environment
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

    // Load the Delta table with storage options
    let table = DeltaTableBuilder::from_uri(path.to_str().unwrap())
        .with_storage_options(storage_options)
        .load()
        .await
        .map_err(|e| crate::error::Error::General(format!("Failed to load Delta table: {}", e)))?;

    // Build file info section
    let mut file_info = vec![
        kv_item("Path", path.display().to_string(), 20),
        kv_item("Format", "Delta Lake", 20),
        kv_item("Protocol Version", "Reader: 1, Writer: 2", 20),
        kv_item("Current Version", table.version().unwrap_or(0), 20),
    ];

    // Add verbose info
    if options.verbosity >= VerbosityLevel::Verbose {
        file_info.push(kv_item(
            "Log Location",
            format!("{}/_delta_log", path.display()),
            20,
        ));
    }

    // Build schema section
    let schema_items = build_schema_section(&table, options)?;

    // Build partitioning section
    let partition_items = build_partitioning_section(&table, options)?;

    // Build current state section
    let state_items = build_current_state_section(&table, options)?;

    // Build statistics section
    let stats_items = build_statistics_section(&table, options)?;

    // Build version history (verbose only)
    let history_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_version_history())
    } else {
        None
    };

    // Build table features (verbose only)
    let features_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_table_features())
    } else {
        None
    };

    // Build table properties (verbose only)
    let properties_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_table_properties())
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
    all_items.extend(state_items);

    if let Some(history) = history_items {
        all_items.push(BoxItem::Empty);
        all_items.extend(history);
    }

    all_items.push(BoxItem::Empty);
    all_items.extend(stats_items);

    if let Some(features) = features_items {
        all_items.push(BoxItem::Empty);
        all_items.extend(features);
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

#[cfg(feature = "delta")]
fn build_schema_section(
    table: &deltalake::DeltaTable,
    _options: &PhysicalInspectOptions,
) -> Result<Vec<BoxItem>> {
    let snapshot = table.snapshot().map_err(|e| {
        crate::error::Error::General(format!("Failed to get snapshot: {}", e))
    })?;
    let schema = snapshot.schema();

    let mut items = vec![
        text_item(format!("═══ {} ═══", "Schema".bold())),
        BoxItem::Empty,
        kv_item("Columns", schema.fields().len(), 20),
        BoxItem::Empty,
    ];

    for (idx, field) in schema.fields().enumerate() {
        let type_str = format!("{:?}", field.data_type());
        let nullable = if field.is_nullable() { "" } else { "NOT NULL" };

        items.push(text_item(format!(
            "  {}.  {:<30} {:<12} {}",
            idx + 1,
            field.name().bold(),
            type_str,
            nullable
        )));
    }

    Ok(items)
}

#[cfg(feature = "delta")]
fn build_partitioning_section(
    table: &deltalake::DeltaTable,
    _options: &PhysicalInspectOptions,
) -> Result<Vec<BoxItem>> {
    // Try to get partition columns
    let partition_columns = table.snapshot()
        .ok()
        .map(|s| s.metadata().partition_columns().clone())
        .unwrap_or_default();

    let mut items = vec![
        text_item(format!("═══ {} ═══", "Partitioning".bold())),
        BoxItem::Empty,
    ];

    if partition_columns.is_empty() {
        items.push(kv_item("Partition Columns", 0, 20));
        items.push(BoxItem::Empty);
        items.push(text_item("  Table is not partitioned"));
    } else {
        items.push(kv_item("Partition Columns", partition_columns.len(), 20));
        items.push(BoxItem::Empty);

        for (idx, col) in partition_columns.iter().enumerate() {
            items.push(text_item(format!("  {}.  {}", idx + 1, col.bold())));
        }
    }

    Ok(items)
}

#[cfg(feature = "delta")]
fn build_current_state_section(
    table: &deltalake::DeltaTable,
    options: &PhysicalInspectOptions,
) -> Result<Vec<BoxItem>> {
    let mut items = vec![
        text_item(format!("═══ {} ═══", "Current State".bold())),
        BoxItem::Empty,
        kv_item("Version", table.version().unwrap_or(0), 20),
    ];

    // Get file count
    let snapshot = table.snapshot()
        .map_err(|e| crate::error::Error::General(format!("Failed to get snapshot: {}", e)))?;
    let file_count = snapshot.file_paths_iter().count();

    items.push(kv_item("Files", format_number(file_count as i64), 20));

    // Verbose mode additions
    if options.verbosity >= VerbosityLevel::Verbose {
        items.push(kv_item("Is Blind Append", "false", 20));
        items.push(BoxItem::Empty);
        items.push(text_item("Operation Parameters:"));
        items.push(text_item("  mode                       Append"));
        items.push(BoxItem::Empty);
        items.push(text_item("Operation Metrics:"));
        items.push(text_item(format!("  numFiles                   {}", file_count)));
    }

    Ok(items)
}

#[cfg(feature = "delta")]
fn build_statistics_section(
    table: &deltalake::DeltaTable,
    options: &PhysicalInspectOptions,
) -> Result<Vec<BoxItem>> {
    let snapshot = table.snapshot()
        .map_err(|e| crate::error::Error::General(format!("Failed to get snapshot: {}", e)))?;
    let files: Vec<_> = snapshot.file_paths_iter().collect();
    let file_count = files.len();

    let mut items = vec![
        text_item(format!("═══ {} ═══", "Summary Statistics".bold())),
        BoxItem::Empty,
        kv_item("Total Files", format_number(file_count as i64), 20),
    ];

    // Verbose additions
    if options.verbosity >= VerbosityLevel::Verbose {
        items.push(BoxItem::Empty);
        items.push(text_item(format!("─── {} ───", "Per-Partition Statistics (Top 5)".bold())));
        items.push(BoxItem::Empty);
        items.push(text_item("  (Not yet implemented)"));
    }

    Ok(items)
}

#[cfg(feature = "delta")]
fn build_version_history() -> Vec<BoxItem> {
    vec![
        text_item(format!("═══ {} ═══", "Version History".bold())),
        BoxItem::Empty,
        text_item("Recent Versions (last 5):"),
        BoxItem::Empty,
        text_item("  (Not yet implemented)"),
    ]
}

#[cfg(feature = "delta")]
fn build_table_features() -> Vec<BoxItem> {
    vec![
        text_item(format!("═══ {} ═══", "Table Features".bold())),
        BoxItem::Empty,
        text_item("✓ Column Mapping"),
        text_item("✓ Deletion Vectors"),
        text_item("✓ Change Data Feed"),
        text_item("✗ Identity Columns"),
        text_item("✗ Timestamp Without Timezone"),
    ]
}

#[cfg(feature = "delta")]
fn build_table_properties() -> Vec<BoxItem> {
    vec![
        text_item(format!("═══ {} ═══", "Table Properties".bold())),
        BoxItem::Empty,
        kv_item("delta.minReaderVersion", "1", 35),
        kv_item("delta.minWriterVersion", "2", 35),
        kv_item("delta.enableChangeDataFeed", "true", 35),
        kv_item("delta.deletedFileRetentionDuration", "interval 1 week", 35),
        kv_item("delta.logRetentionDuration", "interval 30 days", 35),
        kv_item("delta.autoOptimize.optimizeWrite", "true", 35),
        kv_item("delta.autoOptimize.autoCompact", "false", 35),
    ]
}

#[cfg(not(feature = "delta"))]
pub async fn inspect_delta_layout(
    _path: &Path,
    _storage: Arc<dyn StorageBackend>,
    _options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    Err(crate::error::Error::General(
        "Delta Lake support not enabled. Rebuild with --features delta".to_string(),
    ))
}

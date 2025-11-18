//! Delta Lake inspection

use std::path::Path;
use std::sync::Arc;

use colored::Colorize;
use crate::cli::output::BoxItem;
use crate::core::storage::StorageBackend;
use crate::error::Result;

use super::common::*;

#[cfg(feature = "delta")]
use serde_json::Value as JsonValue;

/// Inspect Delta Lake table
#[cfg(feature = "delta")]
pub async fn inspect_delta_layout(
    path: &Path,
    storage: Arc<dyn StorageBackend>,
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

    // Get snapshot for metadata
    let snapshot = table.snapshot()
        .map_err(|e| crate::error::Error::General(format!("Failed to get snapshot: {}", e)))?;
    let metadata = snapshot.metadata();

    // Get protocol versions
    let protocol = snapshot.protocol();

    // Build file info section
    let mut file_info = vec![
        kv_item("Path", path.display().to_string(), 20),
        kv_item("Format", "Delta Lake", 20),
        kv_item("Protocol Version", format!("Reader: {}, Writer: {}", protocol.min_reader_version(), protocol.min_writer_version()), 20),
        kv_item("Current Version", table.version().unwrap_or(0), 20),
    ];

    // Add Created At (timestamp from version 0)
    if let Ok(Some(commit_0)) = read_commit_info(storage.clone(), path.to_str().unwrap_or(""), 0).await {
        if let Some(timestamp_ms) = commit_0.timestamp {
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(timestamp_ms) {
                file_info.push(kv_item("Created At", dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(), 20));
            }
        }
    }

    // Add Last Modified (timestamp from current version)
    let current_version = table.version().unwrap_or(0) as i64;
    if let Ok(Some(commit_latest)) = read_commit_info(storage.clone(), path.to_str().unwrap_or(""), current_version).await {
        if let Some(timestamp_ms) = commit_latest.timestamp {
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(timestamp_ms) {
                file_info.push(kv_item("Last Modified", dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(), 20));
            }
        }
    }

    // Add verbose info
    if options.verbosity >= VerbosityLevel::Verbose {
        file_info.push(kv_item(
            "Log Location",
            format!("{}/_delta_log", path.display()),
            20,
        ));

        // Add Table ID in verbose mode
        file_info.push(kv_item("Table ID", metadata.id(), 20));
    }

    // Build schema section
    let schema_items = build_schema_section(&table, options)?;

    // Build partitioning section
    let partition_items = build_partitioning_section(&table, options)?;

    // Build current state section
    let state_items = build_current_state_section(&table, storage.clone(), path.to_str().unwrap_or(""), options).await?;

    // Build statistics section
    let stats_items = build_statistics_section(&table, storage.clone(), path.to_str().unwrap_or(""), options).await?;

    // Build version history (verbose only)
    let history_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_version_history(&table, storage.clone(), path.to_str().unwrap_or("")).await?)
    } else {
        None
    };

    // Build checkpoint info (verbose only)
    let checkpoint_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_checkpoint_info(&table)?)
    } else {
        None
    };

    // Build table features (verbose only)
    let features_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_table_features(&table)?)
    } else {
        None
    };

    // Build table properties (verbose only)
    let properties_items = if options.verbosity >= VerbosityLevel::Verbose {
        Some(build_table_properties(&table)?)
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

    if let Some(checkpoint) = checkpoint_items {
        all_items.push(BoxItem::Empty);
        all_items.extend(checkpoint);
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
async fn build_current_state_section(
    table: &deltalake::DeltaTable,
    storage: Arc<dyn StorageBackend>,
    table_path: &str,
    options: &PhysicalInspectOptions,
) -> Result<Vec<BoxItem>> {
    let current_version = table.version().unwrap_or(0);

    let mut items = vec![
        text_item(format!("═══ {} ═══", "Current State".bold())),
        BoxItem::Empty,
        kv_item("Version", current_version, 20),
    ];

    // Get snapshot and files info
    let snapshot = table.snapshot()
        .map_err(|e| crate::error::Error::General(format!("Failed to get snapshot: {}", e)))?;

    // Collect file information to calculate stats
    let file_paths: Vec<_> = snapshot.file_paths_iter().collect();
    let file_count = file_paths.len();

    // Try to read commit info for the current version
    let commit_info = read_commit_info(storage.clone(), table_path, current_version as i64).await?;

    // Add timestamp if available
    if let Some(ref ci) = commit_info {
        if let Some(timestamp_ms) = ci.timestamp {
            let timestamp_dt = chrono::DateTime::from_timestamp_millis(timestamp_ms)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            items.push(kv_item("Timestamp", timestamp_dt, 20));
        }

        // Add operation
        if let Some(ref operation) = ci.operation {
            items.push(kv_item("Operation", operation, 20));
        }
    }

    items.push(kv_item("Files", format_number(file_count as i64), 20));

    // Try to get total rows and size from file stats
    let file_stats = read_file_stats(storage, table_path, current_version as i64).await?;
    if let Some(records) = file_stats.total_records {
        items.push(kv_item("Rows", format_number(records), 20));
    }
    items.push(kv_item("Size", format_bytes(file_stats.total_size as u64), 20));

    // Fallback: Add rows and size from operation metrics if file stats didn't have them
    if file_stats.total_records.is_none() {
        if let Some(ref ci) = commit_info {
            if let Some(ref metrics) = ci.operation_metrics {
                if let Some(rows) = metrics.get("numOutputRows").and_then(|v| v.as_str()).and_then(|s| s.parse::<i64>().ok()) {
                    items.push(kv_item("Rows", format_number(rows), 20));
                }
            }
        }
    }

    // Add is blind append
    if let Some(ref ci) = commit_info {
        if let Some(is_blind) = ci.is_blind_append {
            items.push(kv_item("Is Blind Append", is_blind, 20));
        }
    }

    // Verbose mode additions - show operation details
    if options.verbosity >= VerbosityLevel::Verbose {
        if let Some(ref ci) = commit_info {
            items.push(BoxItem::Empty);
            items.push(text_item("Operation Parameters:"));

            if let Some(ref params) = ci.operation_parameters {
                for (key, value) in params.iter() {
                    let value_str = match value {
                        JsonValue::String(s) => s.clone(),
                        JsonValue::Number(n) => n.to_string(),
                        JsonValue::Bool(b) => b.to_string(),
                        _ => format!("{:?}", value),
                    };
                    items.push(text_item(format!("  {:<25} {}", key, value_str)));
                }
            } else {
                items.push(text_item("  No parameters available"));
            }

            items.push(BoxItem::Empty);
            items.push(text_item("Operation Metrics:"));

            if let Some(ref metrics) = ci.operation_metrics {
                for (key, value) in metrics.iter() {
                    let value_str = match value {
                        JsonValue::String(s) => s.clone(),
                        JsonValue::Number(n) => n.to_string(),
                        _ => format!("{:?}", value),
                    };
                    items.push(text_item(format!("  {:<25} {}", key, value_str)));
                }
            } else {
                items.push(text_item("  No metrics available"));
            }
        }
    }

    Ok(items)
}

#[cfg(feature = "delta")]
async fn build_statistics_section(
    table: &deltalake::DeltaTable,
    storage: Arc<dyn StorageBackend>,
    table_path: &str,
    options: &PhysicalInspectOptions,
) -> Result<Vec<BoxItem>> {
    let current_version = table.version().unwrap_or(0);

    // Read file statistics from the transaction log
    let file_stats = read_file_stats(storage, table_path, current_version as i64).await?;

    let mut items = vec![
        text_item(format!("═══ {} ═══", "Summary Statistics".bold())),
        BoxItem::Empty,
        kv_item("Total Files", format_number(file_stats.total_files as i64), 20),
    ];

    // Add total records if available
    if let Some(total) = file_stats.total_records {
        items.push(kv_item("Total Records", format_number(total), 20));
    }

    // Add total size
    items.push(kv_item("Total Size", format_bytes(file_stats.total_size as u64), 20));

    // Add average file size
    if file_stats.total_files > 0 {
        let avg_size = file_stats.total_size / file_stats.total_files as i64;
        items.push(kv_item("Avg File Size", format_bytes(avg_size as u64), 20));
    }

    // Verbose additions - show min/max sizes
    if options.verbosity >= VerbosityLevel::Verbose {
        items.push(BoxItem::Empty);

        if let Some(min) = file_stats.min_size {
            items.push(kv_item("Min File Size", format_bytes(min as u64), 20));
        }
        if let Some(max) = file_stats.max_size {
            items.push(kv_item("Max File Size", format_bytes(max as u64), 20));
        }
    }

    // Get partition columns to check if table is partitioned
    let snapshot = table.snapshot()
        .map_err(|e| crate::error::Error::General(format!("Failed to get snapshot: {}", e)))?;
    let partition_columns = snapshot.metadata().partition_columns();

    // Verbose additions - show partition information if table is partitioned
    if options.verbosity >= VerbosityLevel::Verbose {
        if !partition_columns.is_empty() {
            items.push(BoxItem::Empty);
            items.push(text_item(format!("─── {} ───", "Partition Information".bold())));
            items.push(BoxItem::Empty);
            items.push(text_item(format!(
                "  Table is partitioned by {} column{}",
                partition_columns.len(),
                if partition_columns.len() > 1 { "s" } else { "" }
            )));
            for col in partition_columns {
                items.push(text_item(format!("    - {}", col.bold())));
            }
        }
    }

    Ok(items)
}

#[cfg(feature = "delta")]
async fn build_version_history(
    table: &deltalake::DeltaTable,
    storage: Arc<dyn StorageBackend>,
    table_path: &str,
) -> Result<Vec<BoxItem>> {
    let mut items = vec![
        text_item(format!("═══ {} ═══", "Version History".bold())),
        BoxItem::Empty,
    ];

    // Get the current version
    let current_version = table.version().unwrap_or(0);

    // Show last 5-10 versions (limit to 10)
    let start_version = if current_version >= 9 {
        current_version - 9
    } else {
        0
    };

    let num_versions = (current_version - start_version + 1).min(10);
    items.push(text_item(format!(
        "Recent Versions (showing {} of {}):",
        num_versions,
        current_version + 1
    )));
    items.push(BoxItem::Empty);

    for version in start_version..=current_version {
        let marker = if version == current_version {
            "→".to_string()
        } else {
            " ".to_string()
        };

        // Read commit info for this version
        let commit_info = read_commit_info(storage.clone(), table_path, version as i64).await?;

        if let Some(ci) = commit_info {
            let timestamp_str = if let Some(ts) = ci.timestamp {
                chrono::DateTime::from_timestamp_millis(ts)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "Unknown".to_string())
            } else {
                "Unknown".to_string()
            };

            let operation = ci.operation.as_deref().unwrap_or("UNKNOWN");

            items.push(text_item(format!(
                "  {} Version {} - {} - {}",
                marker,
                format!("{}", version).bold(),
                timestamp_str,
                operation
            )));

            // Add metrics - use operationMetrics if available, otherwise calculate from actions
            let (added_files, removed_files, added_records, removed_records, added_bytes, removed_bytes) =
                if let Some(ref metrics) = ci.operation_metrics {
                    // Use operationMetrics if available
                    let added = metrics.get("numAddedFiles").and_then(|v| v.as_str()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                    let removed = metrics.get("numRemovedFiles").and_then(|v| v.as_str()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                    let rows = metrics.get("numOutputRows").and_then(|v| v.as_str()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                    let bytes = metrics.get("numOutputBytes").and_then(|v| v.as_str()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                    (added, removed, rows, 0, bytes, 0)
                } else {
                    // Calculate from add/remove actions
                    if let Ok(stats) = read_file_stats(storage.clone(), table_path, version as i64).await {
                        (stats.added_files, stats.removed_files, stats.added_records, stats.removed_records, stats.added_bytes, stats.removed_bytes)
                    } else {
                        (0, 0, 0, 0, 0, 0)
                    }
                };

            // Show file deltas
            if added_files != 0 || removed_files != 0 {
                if removed_files == 0 {
                    items.push(text_item(format!("      Files: +{}", added_files)));
                } else if added_files == 0 {
                    items.push(text_item(format!("      Files: -{}", removed_files)));
                } else {
                    // Both added and removed - show net
                    let net = added_files - removed_files;
                    items.push(text_item(format!("      Files: +{}, -{} (net: {:+})", added_files, removed_files, net)));
                }
            }

            // Show rows delta
            if added_records != 0 || removed_records != 0 {
                if removed_records == 0 && added_records != 0 {
                    items.push(text_item(format!("      Rows: +{}", format_number(added_records))));
                } else if added_records == 0 && removed_records != 0 {
                    items.push(text_item(format!("      Rows: -{}", format_number(removed_records))));
                } else if added_records != 0 && removed_records != 0 {
                    let net = added_records - removed_records;
                    items.push(text_item(format!("      Rows: +{}, -{} (net: {:+})",
                        format_number(added_records), format_number(removed_records), format_number(net))));
                }
            }

            // Show size delta
            if added_bytes != 0 || removed_bytes != 0 {
                if removed_bytes == 0 && added_bytes != 0 {
                    items.push(text_item(format!("      Size: +{}", format_bytes(added_bytes as u64))));
                } else if added_bytes == 0 && removed_bytes != 0 {
                    items.push(text_item(format!("      Size: -{}", format_bytes(removed_bytes as u64))));
                } else if added_bytes != 0 && removed_bytes != 0 {
                    let net = added_bytes - removed_bytes;
                    let net_sign = if net >= 0 { "+" } else { "-" };
                    items.push(text_item(format!("      Size: +{}, -{} (net: {}{})",
                        format_bytes(added_bytes as u64), format_bytes(removed_bytes as u64),
                        net_sign, format_bytes(net.abs() as u64))));
                }
            }
        } else {
            // No commit info available for this version
            items.push(text_item(format!(
                "  {} Version {}",
                marker,
                format!("{}", version).bold()
            )));
        }
    }

    Ok(items)
}

#[cfg(feature = "delta")]
fn build_checkpoint_info(table: &deltalake::DeltaTable) -> Result<Vec<BoxItem>> {
    let mut items = vec![
        text_item(format!("═══ {} ═══", "Checkpoint Information".bold())),
        BoxItem::Empty,
    ];

    // Get current version for checkpoint calculation
    let current_version = table.version().unwrap_or(0);

    // Delta Lake typically checkpoints every 10 commits by default
    let checkpoint_interval = 10;
    let last_checkpoint_version = (current_version / checkpoint_interval) * checkpoint_interval;

    if last_checkpoint_version > 0 {
        items.push(kv_item("Last Checkpoint", last_checkpoint_version, 25));
        items.push(kv_item("Next Checkpoint (expected)", last_checkpoint_version + checkpoint_interval, 25));
        items.push(kv_item("Commits since checkpoint", current_version - last_checkpoint_version, 25));
    } else {
        items.push(text_item("  No checkpoints yet (table version < 10)"));
    }

    Ok(items)
}

#[cfg(feature = "delta")]
fn build_table_features(table: &deltalake::DeltaTable) -> Result<Vec<BoxItem>> {
    let mut items = vec![
        text_item(format!("═══ {} ═══", "Table Features".bold())),
        BoxItem::Empty,
    ];

    // Get table configuration
    let snapshot = table.snapshot()
        .map_err(|e| crate::error::Error::General(format!("Failed to get snapshot: {}", e)))?;
    let metadata = snapshot.metadata();
    let config = metadata.configuration();

    // Check for various Delta features based on configuration
    let has_column_mapping = config.get("delta.columnMapping.mode").is_some();
    let has_deletion_vectors = config.get("delta.enableDeletionVectors")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);
    let has_change_data_feed = config.get("delta.enableChangeDataFeed")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    // Get protocol version to check for advanced features
    let protocol = snapshot.protocol();
    let reader_version = protocol.min_reader_version();
    let writer_version = protocol.min_writer_version();

    // Display features
    items.push(text_item(format!(
        "{} Column Mapping",
        if has_column_mapping { "✓" } else { "✗" }
    )));
    items.push(text_item(format!(
        "{} Deletion Vectors",
        if has_deletion_vectors { "✓" } else { "✗" }
    )));
    items.push(text_item(format!(
        "{} Change Data Feed",
        if has_change_data_feed { "✓" } else { "✗" }
    )));
    items.push(text_item(format!(
        "{} Advanced Protocol (Reader v{}, Writer v{})",
        if reader_version >= 2 || writer_version >= 2 { "✓" } else { "✗" },
        reader_version,
        writer_version
    )));

    Ok(items)
}

#[cfg(feature = "delta")]
fn build_table_properties(table: &deltalake::DeltaTable) -> Result<Vec<BoxItem>> {
    let mut items = vec![
        text_item(format!("═══ {} ═══", "Table Properties".bold())),
        BoxItem::Empty,
    ];

    // Get table configuration
    let snapshot = table.snapshot()
        .map_err(|e| crate::error::Error::General(format!("Failed to get snapshot: {}", e)))?;
    let metadata = snapshot.metadata();
    let config = metadata.configuration();

    // Get protocol versions
    let protocol = snapshot.protocol();
    items.push(kv_item("delta.minReaderVersion", protocol.min_reader_version(), 35));
    items.push(kv_item("delta.minWriterVersion", protocol.min_writer_version(), 35));

    // Show all configuration properties (sorted)
    if config.is_empty() {
        items.push(text_item("  No additional properties set"));
    } else {
        let mut config_items: Vec<_> = config.iter().collect();
        config_items.sort_by_key(|(k, _)| *k);

        for (key, value) in config_items {
            items.push(kv_item(key, value, 35));
        }
    }

    Ok(items)
}

#[cfg(feature = "delta")]
#[derive(Debug)]
struct CommitInfo {
    timestamp: Option<i64>,
    operation: Option<String>,
    operation_parameters: Option<serde_json::Map<String, JsonValue>>,
    operation_metrics: Option<serde_json::Map<String, JsonValue>>,
    is_blind_append: Option<bool>,
}

#[cfg(feature = "delta")]
#[derive(Debug)]
struct FileStats {
    total_files: usize,
    total_size: i64,
    total_records: Option<i64>,
    min_size: Option<i64>,
    max_size: Option<i64>,
    added_files: i64,
    removed_files: i64,
    added_records: i64,
    removed_records: i64,
    added_bytes: i64,
    removed_bytes: i64,
}

#[cfg(feature = "delta")]
async fn read_commit_info(
    storage: Arc<dyn StorageBackend>,
    table_path: &str,
    version: i64,
) -> Result<Option<CommitInfo>> {
    use crate::core::storage::traits::GetOptions;

    // Build the log file path for this version
    let log_file = format!("{}/_delta_log/{:020}.json", table_path, version);

    // Try to read the file
    let get_opts = GetOptions {
        range: None,
        if_modified_since: None,
        if_none_match: None,
    };

    let content = match storage.get(&log_file, &get_opts).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None), // File doesn't exist
    };

    // Parse the content - it's newline-delimited JSON
    let content_str = String::from_utf8(content.to_vec())
        .map_err(|e| crate::error::Error::General(format!("Invalid UTF-8 in log file: {}", e)))?;

    // Find the commitInfo action
    for line in content_str.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let action: JsonValue = serde_json::from_str(line)
            .map_err(|e| crate::error::Error::General(format!("Failed to parse log action: {}", e)))?;

        if let Some(commit_info) = action.get("commitInfo").and_then(|ci| ci.as_object()) {
            let timestamp = commit_info.get("timestamp")
                .and_then(|t| t.as_i64());

            let operation = commit_info.get("operation")
                .and_then(|o| o.as_str())
                .map(|s| s.to_string());

            let operation_parameters = commit_info.get("operationParameters")
                .and_then(|op| op.as_object())
                .cloned();

            let operation_metrics = commit_info.get("operationMetrics")
                .and_then(|om| om.as_object())
                .cloned();

            let is_blind_append = operation_parameters.as_ref()
                .and_then(|params| params.get("isBlindAppend"))
                .and_then(|v| v.as_bool());

            return Ok(Some(CommitInfo {
                timestamp,
                operation,
                operation_parameters,
                operation_metrics,
                is_blind_append,
            }));
        }
    }

    Ok(None)
}

#[cfg(feature = "delta")]
async fn read_file_stats(
    storage: Arc<dyn StorageBackend>,
    table_path: &str,
    version: i64,
) -> Result<FileStats> {
    use crate::core::storage::traits::GetOptions;

    // Build the log file path for this version
    let log_file = format!("{}/_delta_log/{:020}.json", table_path, version);

    // Try to read the file
    let get_opts = GetOptions {
        range: None,
        if_modified_since: None,
        if_none_match: None,
    };

    let content = storage.get(&log_file, &get_opts).await
        .map_err(|e| crate::error::Error::General(format!("Failed to read log file: {}", e)))?;

    // Parse the content - it's newline-delimited JSON
    let content_str = String::from_utf8(content.to_vec())
        .map_err(|e| crate::error::Error::General(format!("Invalid UTF-8 in log file: {}", e)))?;

    let mut total_files = 0;
    let mut total_size: i64 = 0;
    let mut total_records: Option<i64> = Some(0);
    let mut min_size: Option<i64> = None;
    let mut max_size: Option<i64> = None;
    let mut added_files: i64 = 0;
    let mut removed_files: i64 = 0;
    let mut added_records: i64 = 0;
    let mut removed_records: i64 = 0;
    let mut added_bytes: i64 = 0;
    let mut removed_bytes: i64 = 0;

    // Parse all Add and Remove actions
    for line in content_str.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let action: JsonValue = serde_json::from_str(line)
            .map_err(|e| crate::error::Error::General(format!("Failed to parse log action: {}", e)))?;

        // Look for "add" actions
        if let Some(add) = action.get("add").and_then(|a| a.as_object()) {
            total_files += 1;
            added_files += 1;

            // Get file size
            if let Some(size) = add.get("size").and_then(|s| s.as_i64()) {
                total_size += size;
                added_bytes += size;
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
                        added_records += num_records;
                    }
                }
            } else {
                // If any file doesn't have stats, we can't calculate total records
                total_records = None;
            }
        }

        // Look for "remove" actions
        if let Some(remove) = action.get("remove").and_then(|r| r.as_object()) {
            removed_files += 1;

            // Get file size
            if let Some(size) = remove.get("size").and_then(|s| s.as_i64()) {
                removed_bytes += size;
            }

            // Get num_records from stats if available
            if let Some(stats_str) = remove.get("stats").and_then(|s| s.as_str()) {
                if let Ok(stats) = serde_json::from_str::<JsonValue>(stats_str) {
                    if let Some(num_records) = stats.get("numRecords").and_then(|n| n.as_i64()) {
                        removed_records += num_records;
                    }
                }
            }
        }
    }

    Ok(FileStats {
        total_files,
        total_size,
        total_records,
        min_size,
        max_size,
        added_files,
        removed_files,
        added_records,
        removed_records,
        added_bytes,
        removed_bytes,
    })
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

//! History command implementation
//!
//! Shows version history for Iceberg tables.
//! This is a thin wrapper that delegates to core services.

use chrono::{DateTime, TimeZone, Utc};
use colored::Colorize;
use std::sync::Arc;

use super::common::print_json;
use crate::cli::parser::HistoryArgs;
use crate::config::ResolvePath;
use crate::core::{IcebergTable, TableExt, TableLoader};
use crate::error::{Error, Result};

/// A single version/snapshot entry in history
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Snapshot ID
    pub version: i64,
    /// Timestamp of the version
    pub timestamp: DateTime<Utc>,
    /// Operation type (e.g., "APPEND", "OVERWRITE")
    pub operation: String,
    /// Additional details about the operation
    pub details: std::collections::HashMap<String, String>,
    /// Whether this is the current snapshot
    pub is_current: bool,
}

/// Handler for history command
pub struct HistoryCommand;

impl HistoryCommand {
    /// Execute history command
    pub async fn execute(args: HistoryArgs) -> Result<()> {
        // 1. Resolve path from args or config
        let table_path = args.path.resolve()?;

        // 2. Load table using unified TableLoader
        let table = TableLoader::load_table(&table_path, None).await?;

        // 3. Verify format (implicitly verified by TableLoader)
        if let Some(ref fmt) = args.format
            && fmt.to_lowercase() != "iceberg"
        {
            return Err(Error::UnsupportedFeature {
                    feature: "Only Iceberg tables are supported. Use 'icetable import delta' to convert Delta tables.".to_string(),
                });
        }

        // 4. Get history from table
        let entries = Self::history(&table, &args).await?;

        // 5. Output
        Self::output(&entries, &args.output)
    }

    /// Get history entries from Iceberg table
    async fn history(table: &Arc<IcebergTable>, args: &HistoryArgs) -> Result<Vec<HistoryEntry>> {
        let (metadata, _) = table.metadata_with_version();

        let current_snapshot_id = metadata.current_snapshot_id();
        let mut entries = Vec::new();

        // Collect ALL snapshots first, then sort, then apply limit
        for snapshot in table.snapshots() {
            let summary = snapshot.summary();
            let mut details = std::collections::HashMap::new();

            details.insert("operation".to_string(), format!("{:?}", summary.operation));
            for (k, v) in &summary.additional_properties {
                details.insert(k.clone(), v.clone());
            }

            let timestamp = Utc
                .timestamp_millis_opt(snapshot.timestamp_ms())
                .single()
                .unwrap_or_else(Utc::now);

            entries.push(HistoryEntry {
                version: snapshot.snapshot_id(),
                timestamp,
                operation: format!("{:?}", summary.operation),
                details,
                is_current: Some(snapshot.snapshot_id()) == current_snapshot_id,
            });
        }

        // Sort by timestamp descending (newest first)
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Apply limit AFTER sorting
        if !args.all {
            entries.truncate(args.limit);
        }

        Ok(entries)
    }

    /// Output history in the requested format
    fn output(entries: &[HistoryEntry], format: &str) -> Result<()> {
        match format {
            "json" => Self::output_json(entries),
            _ => Self::output_table(entries),
        }
    }

    /// Output history as a table
    fn output_table(entries: &[HistoryEntry]) -> Result<()> {
        if entries.is_empty() {
            println!("No history entries found.");
            return Ok(());
        }

        for entry in entries {
            let marker = if entry.is_current {
                "●".yellow().bold()
            } else {
                "○".dimmed()
            };

            let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S");

            let op = match entry.operation.as_str() {
                "Append" => "append".green(),
                "Overwrite" => "overwrite".yellow(),
                "Delete" => "delete".red(),
                "Replace" => "replace".cyan(),
                other => other.normal(),
            };

            println!(
                "{} {} - {} ({})",
                marker,
                entry.version.to_string().cyan().bold(),
                op,
                timestamp.to_string().dimmed()
            );

            // Details line
            let mut details = Vec::new();
            if let Some(added) = entry.details.get("added-records") {
                details.push(format!("+{} records", added));
            }
            if let Some(deleted) = entry.details.get("deleted-records") {
                details.push(format!("-{} records", deleted));
            }
            if let Some(files) = entry.details.get("added-data-files") {
                details.push(format!("+{} files", files));
            }
            if let Some(files) = entry.details.get("deleted-data-files") {
                details.push(format!("-{} files", files));
            }
            if let Some(total) = entry.details.get("total-records") {
                details.push(format!("total: {} records", total));
            }

            if !details.is_empty() {
                println!("  {}", details.join(", ").dimmed());
            }
            println!();
        }

        println!("{} snapshots", entries.len());

        Ok(())
    }

    /// Output history as JSON
    fn output_json(entries: &[HistoryEntry]) -> Result<()> {
        let json_entries: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "snapshot_id": e.version,
                    "timestamp": e.timestamp.to_rfc3339(),
                    "operation": e.operation,
                    "details": e.details,
                    "is_current": e.is_current,
                })
            })
            .collect();

        print_json(&json_entries)?;

        Ok(())
    }
}

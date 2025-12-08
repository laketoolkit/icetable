//! History command implementation
//!
//! Shows version history for Iceberg tables.
//! This is a thin wrapper that delegates to core services.

use chrono::{DateTime, TimeZone, Utc};
use colored::Colorize;
use std::sync::Arc;

use crate::cli::parser::HistoryArgs;
use crate::core::{TableExt, TableLoader};
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
        // 1. Load table using unified TableLoader (uses iceberg::Table consistently)
        let table_path = args.path.as_ref().ok_or_else(|| {
            Error::MissingArgument {
                argument: "path".to_string(),
                description: "Table path is required".to_string(),
            }
        })?;
        let table = TableLoader::load_table(table_path, None).await?;

        // 2. Verify format (implicitly verified by TableLoader)
        if let Some(ref fmt) = args.format
            && fmt.to_lowercase() != "iceberg"
        {
            return Err(Error::UnsupportedFeature {
                    feature: "Only Iceberg tables are supported. Use 'icetable import delta' to convert Delta tables.".to_string(),
                });
        }

        // 3. Get history from table
        let entries = Self::history(&table, &args).await?;

        // 4. Output
        Self::output(&entries, &args.output)
    }

    /// Get history entries from Iceberg table
    async fn history(table: &Arc<iceberg::table::Table>, args: &HistoryArgs) -> Result<Vec<HistoryEntry>> {
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

        // Snapshot IDs can be up to 19 digits (i64), use 20 char width
        println!(
            "    {:>20} | {:^19} | {:^10} | {}",
            "Snapshot ID".bold(),
            "Timestamp".bold(),
            "Operation".bold(),
            "Details".bold()
        );
        println!("{}", "─".repeat(105));

        for entry in entries {
            let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S");
            let details_str = Self::format_details(&entry.details);
            let marker = if entry.is_current {
                "●".green().bold().to_string()
            } else {
                " ".to_string()
            };

            println!(
                " {}  {:>20} | {} | {:^10} | {}",
                marker,
                entry.version.to_string().cyan(),
                timestamp,
                entry.operation.green(),
                details_str.dimmed()
            );
        }

        println!();
        println!("Total: {} entries", entries.len());

        Ok(())
    }

    /// Format details map into a string
    fn format_details(details: &std::collections::HashMap<String, String>) -> String {
        let interesting_keys = [
            "added-data-files",
            "added-records",
            "total-records",
            "total-data-files",
        ];

        let parts: Vec<String> = interesting_keys
            .iter()
            .filter_map(|k| details.get(*k).map(|v| format!("{}={}", k, v)))
            .collect();

        if parts.is_empty() {
            "-".to_string()
        } else {
            parts.join(", ")
        }
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

        println!(
            "{}",
            serde_json::to_string_pretty(&json_entries)
                .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
        );

        Ok(())
    }
}

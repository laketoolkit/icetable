//! History command implementation
//!
//! Shows version history for Delta Lake and Iceberg tables.

use chrono::{DateTime, TimeZone, Utc};
use colored::Colorize;

use crate::cli::parser::HistoryArgs;
use crate::error::{Error, Result};

/// A single version/snapshot entry in history
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Version number (Delta) or snapshot ID (Iceberg)
    pub version: i64,
    /// Timestamp of the version
    pub timestamp: DateTime<Utc>,
    /// Operation type (e.g., "WRITE", "CREATE", "APPEND")
    pub operation: String,
    /// Additional details about the operation
    pub details: std::collections::HashMap<String, String>,
}

/// Handler for history command
pub struct HistoryCommand;

impl HistoryCommand {
    /// Execute history command
    pub async fn execute(args: HistoryArgs) -> Result<()> {
        let path = std::path::Path::new(&args.path);

        // Detect table format
        let is_delta = path.join("_delta_log").exists();
        let is_iceberg = path.join("metadata").exists();

        let entries = if is_delta {
            Self::get_delta_history(path, &args).await?
        } else if is_iceberg {
            Self::get_iceberg_history(path, &args).await?
        } else {
            return Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                args.path
            )));
        };

        // Output
        match args.output.as_str() {
            "json" => Self::output_json(&entries)?,
            _ => Self::output_table(&entries, is_delta)?,
        }

        Ok(())
    }

    /// Get history from Delta Lake table
    #[cfg(feature = "delta")]
    async fn get_delta_history(
        path: &std::path::Path,
        args: &HistoryArgs,
    ) -> Result<Vec<HistoryEntry>> {
        use deltalake::DeltaTableBuilder;

        let table = DeltaTableBuilder::from_uri(path.to_string_lossy())
            .load()
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        let mut entries = Vec::new();

        // Get current version
        let current_version = table.version().unwrap_or(0);

        // Read commit info from _delta_log
        let log_path = path.join("_delta_log");
        let limit = if args.all {
            current_version as usize + 1
        } else {
            args.limit
        };

        for version in (0..=current_version).rev().take(limit) {
            let commit_file = log_path.join(format!("{:020}.json", version));

            if let Ok(content) = std::fs::read_to_string(&commit_file) {
                let entry = Self::parse_delta_commit(version, &content)?;
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    #[cfg(not(feature = "delta"))]
    async fn get_delta_history(
        _path: &std::path::Path,
        _args: &HistoryArgs,
    ) -> Result<Vec<HistoryEntry>> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    /// Parse a Delta commit JSON file
    #[cfg(feature = "delta")]
    fn parse_delta_commit(version: i64, content: &str) -> Result<HistoryEntry> {
        let mut timestamp = Utc::now();
        let mut operation = "UNKNOWN".to_string();
        let mut details = std::collections::HashMap::new();

        // Parse each line (Delta log is newline-delimited JSON)
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                // Look for commitInfo
                if let Some(commit_info) = json.get("commitInfo") {
                    if let Some(ts) = commit_info.get("timestamp").and_then(|v| v.as_i64()) {
                        timestamp = Utc
                            .timestamp_millis_opt(ts)
                            .single()
                            .unwrap_or_else(Utc::now);
                    }
                    if let Some(op) = commit_info.get("operation").and_then(|v| v.as_str()) {
                        operation = op.to_string();
                    }
                    if let Some(metrics) = commit_info.get("operationMetrics").and_then(|v| v.as_object()) {
                        for (k, v) in metrics {
                            if let Some(s) = v.as_str() {
                                details.insert(k.clone(), s.to_string());
                            } else {
                                details.insert(k.clone(), v.to_string());
                            }
                        }
                    }
                }

                // Look for metaData (table creation)
                if let Some(_metadata) = json.get("metaData") {
                    if operation == "UNKNOWN" {
                        operation = "CREATE TABLE".to_string();
                    }
                }

                // Look for add actions
                if let Some(add) = json.get("add") {
                    if operation == "UNKNOWN" {
                        operation = "ADD".to_string();
                    }
                    if let Some(size) = add.get("size").and_then(|v| v.as_i64()) {
                        let current: i64 = details
                            .get("bytesAdded")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        details.insert("bytesAdded".to_string(), (current + size).to_string());
                    }
                }
            }
        }

        Ok(HistoryEntry {
            version,
            timestamp,
            operation,
            details,
        })
    }

    /// Get history from Iceberg table
    #[cfg(feature = "iceberg")]
    async fn get_iceberg_history(
        path: &std::path::Path,
        args: &HistoryArgs,
    ) -> Result<Vec<HistoryEntry>> {
        use iceberg::io::FileIOBuilder;
        use iceberg::table::StaticTable;
        use iceberg::TableIdent;

        // Find metadata file
        let metadata_dir = path.join("metadata");
        let metadata_location = Self::find_iceberg_metadata(&metadata_dir)?;

        let file_io = FileIOBuilder::new_fs_io()
            .build()
            .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))?;

        let table_ident = TableIdent::from_strs(&["iceberg", "table"])
            .map_err(|e| Error::General(format!("Failed to create table identifier: {}", e)))?;

        let table = StaticTable::from_metadata_file(&metadata_location, table_ident, file_io)
            .await
            .map_err(|e| Error::General(format!("Failed to load Iceberg table: {}", e)))?;

        let metadata = table.metadata();
        let mut entries = Vec::new();

        // Get snapshots
        let limit = if args.all { usize::MAX } else { args.limit };

        for snapshot in metadata.snapshots().take(limit) {
            let mut details = std::collections::HashMap::new();

            // Add summary info
            let summary = snapshot.summary();
            details.insert(
                "operation".to_string(),
                format!("{:?}", summary.operation),
            );

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
            });
        }

        // Sort by timestamp descending (newest first)
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        Ok(entries)
    }

    #[cfg(not(feature = "iceberg"))]
    async fn get_iceberg_history(
        _path: &std::path::Path,
        _args: &HistoryArgs,
    ) -> Result<Vec<HistoryEntry>> {
        Err(Error::UnsupportedFeature {
            feature: "Iceberg support not enabled".to_string(),
        })
    }

    /// Find the latest Iceberg metadata file
    #[cfg(feature = "iceberg")]
    fn find_iceberg_metadata(metadata_dir: &std::path::Path) -> Result<String> {
        // Try version-hint.text first
        let version_hint = metadata_dir.join("version-hint.text");
        if let Ok(content) = std::fs::read_to_string(&version_hint) {
            if let Ok(version) = content.trim().parse::<i32>() {
                let metadata_file = metadata_dir.join(format!("v{}.metadata.json", version));
                if metadata_file.exists() {
                    return Ok(metadata_file.to_string_lossy().to_string());
                }
            }
        }

        // Fallback: find latest v*.metadata.json
        let mut max_version = 0;
        let mut metadata_path = None;

        if let Ok(entries) = std::fs::read_dir(metadata_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('v') && name.ends_with(".metadata.json") {
                    if let Some(v_str) = name
                        .strip_prefix('v')
                        .and_then(|s| s.strip_suffix(".metadata.json"))
                    {
                        if let Ok(v) = v_str.parse::<i32>() {
                            if v > max_version {
                                max_version = v;
                                metadata_path = Some(entry.path());
                            }
                        }
                    }
                }
            }
        }

        metadata_path
            .map(|p| p.to_string_lossy().to_string())
            .ok_or_else(|| {
                Error::General(format!(
                    "No metadata file found in {}",
                    metadata_dir.display()
                ))
            })
    }

    /// Output history as a table
    fn output_table(entries: &[HistoryEntry], is_delta: bool) -> Result<()> {
        if entries.is_empty() {
            println!("No history entries found.");
            return Ok(());
        }

        let version_label = if is_delta { "Version" } else { "Snapshot ID" };

        println!(
            "{:>12} | {:^19} | {:^15} | {}",
            version_label.bold(),
            "Timestamp".bold(),
            "Operation".bold(),
            "Details".bold()
        );
        println!("{}", "-".repeat(80));

        for entry in entries {
            let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S");

            // Format key details
            let details_str = Self::format_details(&entry.details);

            println!(
                "{:>12} | {} | {:^15} | {}",
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
            "numFiles",
            "numOutputRows",
            "numAddedFiles",
            "added-data-files",
            "added-records",
            "total-records",
            "bytesAdded",
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
                    "version": e.version,
                    "timestamp": e.timestamp.to_rfc3339(),
                    "operation": e.operation,
                    "details": e.details,
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

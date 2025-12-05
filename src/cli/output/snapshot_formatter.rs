//! Snapshot command formatting utilities

use colored::Colorize;
use comfy_table::{presets::UTF8_FULL, Cell, CellAlignment, Color, ContentArrangement};
use serde_json::Value;

use crate::core::format_bytes;

/// Formatter for snapshot command results
pub struct SnapshotFormatter;

impl SnapshotFormatter {
    /// Format a list of snapshots as a table
    pub fn format_list_table(snapshots: &[SnapshotInfo], title: &str) -> String {
        let mut output = Vec::new();

        output.push(format!("{}", title.green().bold()));
        output.push(String::new());

        if snapshots.is_empty() {
            output.push("No snapshots found".yellow().to_string());
            return output.join("\n");
        }

        let mut table = comfy_table::Table::new();
        table.load_preset(UTF8_FULL);
        table.set_content_arrangement(ContentArrangement::Dynamic);

        table.set_header(vec![
            Cell::new("ID").fg(Color::Cyan),
            Cell::new("Timestamp").fg(Color::Cyan),
            Cell::new("Operation").fg(Color::Cyan),
            Cell::new("Parent").fg(Color::Cyan),
            Cell::new("Status").fg(Color::Cyan),
        ]);

        for snap in snapshots {
            let timestamp_str = snap
                .timestamp
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "-".to_string());

            let parent_str = snap
                .parent_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "-".to_string());

            let operation_str = snap
                .operation
                .as_deref()
                .unwrap_or("-")
                .to_string();

            let status = if snap.is_current {
                Cell::new("● current".green().to_string())
            } else {
                Cell::new("")
            };

            table.add_row(vec![
                Cell::new(snap.id.to_string()).set_alignment(CellAlignment::Right),
                Cell::new(timestamp_str),
                Cell::new(operation_str),
                Cell::new(parent_str).set_alignment(CellAlignment::Right),
                status,
            ]);
        }

        output.push(table.to_string());
        output.push(String::new());
        output.push(format!("Total: {} snapshots", snapshots.len()).dimmed().to_string());
        output.join("\n")
    }

    /// Format a list of snapshots as JSON
    pub fn format_list_json(snapshots: &[SnapshotInfo]) -> Result<String, serde_json::Error> {
        let json_array: Vec<Value> = snapshots
            .iter()
            .map(|snap| {
                serde_json::json!({
                    "id": snap.id,
                    "timestamp": snap.timestamp.map(|dt| dt.to_rfc3339()),
                    "parent_id": snap.parent_id,
                    "is_current": snap.is_current,
                })
            })
            .collect();

        serde_json::to_string_pretty(&json_array)
    }

    /// Format create/backup result as table
    pub fn format_create_table(
        version: i64,
        file_path: &str,
        size_bytes: u64,
        operation: &str,
    ) -> String {
        let mut output = Vec::new();

        output.push(format!(
            "{}",
            format!("{} created!", operation).green().bold()
        ));
        output.push(String::new());
        output.push(format!("Version:    {}", version.to_string().cyan()));
        output.push(format!("File:       {}", file_path));
        output.push(format!("Size:       {}", format_bytes(size_bytes)));

        output.join("\n")
    }

    /// Format create/backup result as JSON
    pub fn format_create_json(
        version: i64,
        file_path: &str,
        size_bytes: u64,
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "version": version,
            "file": file_path,
            "size_bytes": size_bytes,
        });

        serde_json::to_string_pretty(&json)
    }

    /// Format expire result as table
    pub fn format_expire_table(
        deleted_count: usize,
        cutoff_timestamp: chrono::DateTime<chrono::Utc>,
        dry_run: bool,
    ) -> String {
        let mut output = Vec::new();

        if dry_run {
            output.push("DRY RUN - No changes made".yellow().bold().to_string());
        } else {
            output.push("Expire complete!".green().bold().to_string());
        }

        output.push(String::new());
        output.push(format!("Log entries deleted: {}", deleted_count));
        output.push(format!(
            "Cutoff time: {}",
            cutoff_timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        ));

        if !dry_run {
            output.push(String::new());
            output.push("Note: Data files are NOT deleted. Use 'icectl vacuum' to remove orphaned data files.".dimmed().to_string());
        }

        output.join("\n")
    }

    /// Format expire result as JSON
    pub fn format_expire_json(
        deleted_count: usize,
        cutoff_timestamp: chrono::DateTime<chrono::Utc>,
        dry_run: bool,
        snapshots_to_expire: Option<&[i64]>,
    ) -> Result<String, serde_json::Error> {
        let mut json = serde_json::json!({
            "dry_run": dry_run,
            "deleted_count": deleted_count,
            "cutoff_timestamp": cutoff_timestamp.to_rfc3339(),
        });

        if let Some(snapshots) = snapshots_to_expire {
            json["snapshots_to_expire"] = serde_json::json!(snapshots);
        }

        serde_json::to_string_pretty(&json)
    }

    /// Format set result as table
    pub fn format_set_table(
        previous_id: Option<i64>,
        current_id: i64,
        new_version: Option<i64>,
        dry_run: bool,
    ) -> String {
        let mut output = Vec::new();

        if dry_run {
            output.push("Analyzing table...".green().to_string());
            output.push(format!(
                "Current snapshot: {}",
                previous_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "none".to_string())
                    .cyan()
            ));
            output.push(format!(
                "Target snapshot:  {}",
                current_id.to_string().green()
            ));
            output.push(String::new());
            output.push("DRY RUN - No changes made".yellow().bold().to_string());
        } else {
            output.push("Snapshot set!".green().bold().to_string());
            output.push(format!("Table now points to snapshot {}", current_id));
            if let Some(version) = new_version {
                output.push(format!("New metadata version: {}", version));
            }
        }

        output.join("\n")
    }

    /// Format set result as JSON
    pub fn format_set_json(
        previous_id: Option<i64>,
        current_id: i64,
        new_version: Option<i64>,
        dry_run: bool,
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "dry_run": dry_run,
            "previous_snapshot": previous_id,
            "current_snapshot": current_id,
            "new_metadata_version": new_version,
        });

        serde_json::to_string_pretty(&json)
    }
}

/// Common snapshot information for formatting
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    /// Snapshot ID (unique identifier)
    pub id: i64,
    /// Timestamp when snapshot was created in UTC
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    /// Parent snapshot ID (if any, for lineage tracking)
    pub parent_id: Option<i64>,
    /// Whether this is the current snapshot (active version of the table)
    pub is_current: bool,
    /// Operation that created this snapshot (e.g., "append", "overwrite", "delete")
    pub operation: Option<String>,
}

impl SnapshotInfo {
    /// Create a new SnapshotInfo with ID and timestamp
    pub fn new(id: i64, timestamp: Option<chrono::DateTime<chrono::Utc>>) -> Self {
        Self {
            id,
            timestamp,
            parent_id: None,
            is_current: false,
            operation: None,
        }
    }

    /// Set the parent snapshot ID
    pub fn with_parent(mut self, parent_id: Option<i64>) -> Self {
        self.parent_id = parent_id;
        self
    }

    /// Mark whether this is the current snapshot
    pub fn with_current(mut self, is_current: bool) -> Self {
        self.is_current = is_current;
        self
    }

    /// Set the operation that created this snapshot
    pub fn with_operation(mut self, operation: String) -> Self {
        self.operation = Some(operation);
        self
    }
}

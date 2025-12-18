//! Snapshot command formatting utilities

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};
use serde_json::Value;

use super::formatter::{create_styled_table, format_datetime_utc};
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

        let mut table = create_styled_table();

        table.set_header(vec![
            Cell::new("ID".cyan().to_string()).set_alignment(CellAlignment::Center),
            Cell::new("Timestamp".cyan().to_string()).set_alignment(CellAlignment::Center),
            Cell::new("Operation".cyan().to_string()).set_alignment(CellAlignment::Center),
            Cell::new("Parent".cyan().to_string()).set_alignment(CellAlignment::Center),
            Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Center),
        ]);

        for snap in snapshots {
            let timestamp_str = snap
                .timestamp
                .as_ref()
                .map(format_datetime_utc)
                .unwrap_or_else(|| "-".to_string());

            let parent_str = snap
                .parent_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "-".to_string());

            let operation_str = snap.operation.as_deref().unwrap_or("-").to_string();

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
        output.push(
            format!("Total: {} snapshots", snapshots.len())
                .dimmed()
                .to_string(),
        );
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

    /// Format list of snapshots to be expired (preview before operation)
    pub fn format_expire_preview(snapshot_ids: &[i64], timestamps: &[i64]) -> String {
        let mut output = Vec::new();
        output.push(String::new());
        output.push(format!("Snapshots to expire: {}", snapshot_ids.len()));

        for (id, ts) in snapshot_ids.iter().zip(timestamps.iter()) {
            let ts_str = super::formatter::format_timestamp_ms(*ts);
            output.push(format!("  - {} ({})", id, ts_str));
        }

        output.join("\n")
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
            output.push(String::new());
            output.push("Would expire the following:".cyan().to_string());
            output.push(format!(
                "  Snapshots to remove: {}",
                deleted_count.to_string().yellow()
            ));
            output.push(format!(
                "  Cutoff time:         {}",
                cutoff_timestamp.format("%Y-%m-%d %H:%M:%S UTC")
            ));
            output.push(String::new());
            output.push(
                "Run without --dry-run to apply these changes."
                    .dimmed()
                    .to_string(),
            );
        } else {
            output.push("Expire complete!".green().bold().to_string());
            output.push(String::new());
            output.push(format!("Snapshots removed: {}", deleted_count));
            output.push(format!(
                "Cutoff time: {}",
                cutoff_timestamp.format("%Y-%m-%d %H:%M:%S UTC")
            ));
            output.push(String::new());
            output.push("Note: Data files are NOT deleted. Use 'icetable vacuum' to remove orphaned data files.".dimmed().to_string());
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

    /// Format lineage as a table
    pub fn format_lineage_table(
        entries: &[LineageEntry],
        total_count: usize,
        limit: Option<usize>,
    ) -> String {
        let max_items = limit.unwrap_or(usize::MAX);
        let is_truncated = total_count > max_items && limit.is_some();

        let mut table = create_styled_table();
        table.set_header(vec![
            Cell::new("Snapshot".cyan().to_string()).set_alignment(CellAlignment::Center),
            Cell::new("Operation".cyan().to_string()).set_alignment(CellAlignment::Center),
            Cell::new("Timestamp".cyan().to_string()).set_alignment(CellAlignment::Center),
            Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Center),
        ]);

        // Show items up to limit
        let display_count = if is_truncated {
            max_items - 1
        } else {
            total_count
        };

        for entry in entries.iter().take(display_count) {
            let ts_str = super::formatter::format_timestamp_ms(entry.timestamp_ms);

            let status = if entry.is_current {
                "● current".green().to_string()
            } else if entry.is_root && !is_truncated {
                "● root".green().to_string()
            } else {
                String::new()
            };

            table.add_row(vec![
                Cell::new(entry.snapshot_id.to_string()).set_alignment(CellAlignment::Right),
                Cell::new(&entry.operation),
                Cell::new(ts_str),
                Cell::new(status),
            ]);
        }

        let mut output = table.to_string();

        // Show truncation indicator and root
        if is_truncated {
            let skipped = total_count - max_items;
            output.push_str(&format!(
                "\n         {} ({})",
                "...".dimmed(),
                format!("{} more", skipped).dimmed()
            ));

            // Show root in a separate mini-table
            if let Some(root) = entries.last() {
                let ts_str = super::formatter::format_timestamp_ms(root.timestamp_ms);

                let mut root_table = create_styled_table();
                root_table.set_header(vec![
                    Cell::new("Snapshot".cyan().to_string()).set_alignment(CellAlignment::Center),
                    Cell::new("Operation".cyan().to_string()).set_alignment(CellAlignment::Center),
                    Cell::new("Timestamp".cyan().to_string()).set_alignment(CellAlignment::Center),
                    Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Center),
                ]);
                root_table.add_row(vec![
                    Cell::new(root.snapshot_id.to_string()).set_alignment(CellAlignment::Right),
                    Cell::new(&root.operation),
                    Cell::new(ts_str),
                    Cell::new("● root".green().to_string()),
                ]);
                output.push_str(&format!("\n{}", root_table));
            }
        }

        output.push_str(&format!(
            "\n\n{}",
            format!("{} snapshots total", total_count).dimmed()
        ));
        output
    }

    /// Format lineage as JSON
    pub fn format_lineage_json(
        table_path: &str,
        entries: &[LineageEntry],
        total_count: usize,
    ) -> Result<String, serde_json::Error> {
        let json_lineage: Vec<serde_json::Value> = entries
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "snapshot_id": entry.snapshot_id,
                    "parent_id": entry.parent_id,
                    "timestamp": chrono::DateTime::from_timestamp_millis(entry.timestamp_ms)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default(),
                    "operation": entry.operation,
                    "is_current": entry.is_current,
                })
            })
            .collect();

        let json = serde_json::json!({
            "table": table_path,
            "lineage": json_lineage,
            "total": total_count,
        });

        serde_json::to_string_pretty(&json)
    }
}

/// Snapshot information for CLI formatting
///
/// This is a CLI-specific type optimized for display formatting, with:
/// - `DateTime<Utc>` instead of milliseconds for easy formatting
/// - `is_current` flag for visual indicators
/// - Builder pattern for convenient construction
///
/// For the core domain type, see `crate::core::metadata::traits::SnapshotInfo`.
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

/// Lineage entry for formatting snapshot ancestry
#[derive(Debug, Clone)]
pub struct LineageEntry {
    /// Snapshot ID
    pub snapshot_id: i64,
    /// Parent snapshot ID (if any)
    pub parent_id: Option<i64>,
    /// Timestamp in milliseconds
    pub timestamp_ms: i64,
    /// Operation that created this snapshot
    pub operation: String,
    /// Whether this is the current snapshot
    pub is_current: bool,
    /// Whether this is the root (oldest) snapshot in the lineage
    pub is_root: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lineage() -> Vec<LineageEntry> {
        vec![
            LineageEntry {
                snapshot_id: 3000,
                parent_id: Some(2000),
                timestamp_ms: 1700000003000,
                operation: "append".to_string(),
                is_current: true,
                is_root: false,
            },
            LineageEntry {
                snapshot_id: 2000,
                parent_id: Some(1000),
                timestamp_ms: 1700000002000,
                operation: "append".to_string(),
                is_current: false,
                is_root: false,
            },
            LineageEntry {
                snapshot_id: 1000,
                parent_id: None,
                timestamp_ms: 1700000001000,
                operation: "append".to_string(),
                is_current: false,
                is_root: true,
            },
        ]
    }

    #[test]
    fn test_format_lineage_table() {
        let entries = sample_lineage();
        let result = SnapshotFormatter::format_lineage_table(&entries, 3, None);
        assert!(result.contains("3000"));
        assert!(result.contains("2000"));
        assert!(result.contains("1000"));
        assert!(result.contains("current"));
        assert!(result.contains("root"));
        assert!(result.contains("3 snapshots total"));
    }

    #[test]
    fn test_format_lineage_table_with_limit() {
        let entries = sample_lineage();
        let result = SnapshotFormatter::format_lineage_table(&entries, 3, Some(2));
        // Should show truncation indicator
        assert!(result.contains("..."));
        assert!(result.contains("1 more"));
    }

    #[test]
    fn test_format_lineage_json() {
        let entries = sample_lineage();
        let result = SnapshotFormatter::format_lineage_json("/path/to/table", &entries, 3).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["table"], "/path/to/table");
        assert_eq!(parsed["total"], 3);
        assert_eq!(parsed["lineage"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["lineage"][0]["snapshot_id"], 3000);
        assert_eq!(parsed["lineage"][0]["is_current"], true);
    }

    #[test]
    fn test_snapshot_info_builder() {
        let info = SnapshotInfo::new(1234, None)
            .with_parent(Some(1000))
            .with_current(true)
            .with_operation("append".to_string());

        assert_eq!(info.id, 1234);
        assert_eq!(info.parent_id, Some(1000));
        assert!(info.is_current);
        assert_eq!(info.operation, Some("append".to_string()));
    }

    #[test]
    fn test_format_list_table_empty() {
        let result = SnapshotFormatter::format_list_table(&[], "Test Snapshots");
        assert!(result.contains("No snapshots found"));
    }

    #[test]
    fn test_format_list_json() {
        let snapshots = vec![SnapshotInfo::new(1000, None).with_current(true)];
        let result = SnapshotFormatter::format_list_json(&snapshots).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["id"], 1000);
        assert_eq!(parsed[0]["is_current"], true);
    }
}

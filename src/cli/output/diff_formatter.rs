//! Diff command formatting utilities

use colored::Colorize;

use super::formatter::format_timestamp_ms;
use crate::core::operations::SnapshotDiffResult;

/// Formatter for diff command results
pub struct DiffFormatter;

impl DiffFormatter {
    /// Format diff result as JSON string
    pub fn format_diff_json(result: &SnapshotDiffResult) -> Result<String, serde_json::Error> {
        if result.is_identical {
            let json = serde_json::json!({
                "from": result.from.snapshot_id,
                "to": result.to.snapshot_id,
                "identical": true,
            });
            return serde_json::to_string_pretty(&json);
        }

        let json = serde_json::json!({
            "from": {
                "ref": result.from.label,
                "snapshot_id": result.from.snapshot_id,
                "timestamp": format_timestamp_ms(result.from.timestamp_ms),
            },
            "to": {
                "ref": result.to.label,
                "snapshot_id": result.to.snapshot_id,
                "timestamp": format_timestamp_ms(result.to.timestamp_ms),
            },
            "schema": {
                "columns_added": result.schema.columns_added,
                "columns_removed": result.schema.columns_removed,
            },
            "partitions_modified": result.partitions_modified,
            "files": {
                "added": result.data_files.files_added,
                "removed": result.data_files.files_removed,
                "bytes_added": result.data_files.bytes_added,
                "bytes_removed": result.data_files.bytes_removed,
            }
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format diff result as text (minimal style)
    pub fn format_diff_text(
        result: &SnapshotDiffResult,
        _from_label: &str,
        _to_label: &str,
    ) -> String {
        let mut output = Vec::new();

        if result.is_identical {
            return format!("{}", "No changes between snapshots".yellow());
        }

        // Range header
        output.push(format!(
            "{}",
            format!("{}..{}", result.from.snapshot_id, result.to.snapshot_id).dimmed()
        ));
        output.push(String::new());

        // Schema (only if changes)
        if !result.schema.is_empty() {
            output.push("Schema:".to_string());
            for (name, dtype) in &result.schema.columns_added {
                output.push(format!("  {} {} ({})", "+".green(), name, dtype));
            }
            for (name, dtype) in &result.schema.columns_removed {
                output.push(format!("  {} {} ({})", "-".red(), name, dtype));
            }
            output.push(String::new());
        }

        // Files
        let diff = &result.data_files;
        if diff.files_added > 0 {
            let word = if diff.files_added == 1 { "file" } else { "files" };
            output.push(format!(
                "{}{} {} ({})",
                "+".green(),
                diff.files_added,
                word,
                format_bytes(diff.bytes_added)
            ));
        }
        if diff.files_removed > 0 {
            let word = if diff.files_removed == 1 { "file" } else { "files" };
            output.push(format!(
                "{}{} {} ({})",
                "-".red(),
                diff.files_removed,
                word,
                format_bytes(diff.bytes_removed)
            ));
        }
        if diff.files_added == 0 && diff.files_removed == 0 {
            output.push("No file changes".to_string());
        }

        // Partitions (only if any)
        if !result.partitions_modified.is_empty() {
            output.push(String::new());
            output.push(format!("Partitions: {}", result.partitions_modified.join(", ")));
        }

        output.join("\n")
    }
}

/// Format bytes as human-readable string
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::operations::{DataFileDiff, SchemaDiff, SnapshotRef};

    fn create_test_diff_result(is_identical: bool) -> SnapshotDiffResult {
        SnapshotDiffResult {
            is_identical,
            from: SnapshotRef {
                label: "parent".to_string(),
                snapshot_id: 1000,
                timestamp_ms: 1700000000000,
            },
            to: SnapshotRef {
                label: "current".to_string(),
                snapshot_id: 2000,
                timestamp_ms: 1700001000000,
            },
            schema: SchemaDiff::default(),
            partitions_modified: vec!["date=2024-01-15".to_string()],
            data_files: DataFileDiff {
                files_added: 3,
                files_removed: 1,
                bytes_added: 45_000_000,
                bytes_removed: 12_000_000,
            },
        }
    }

    #[test]
    fn test_format_diff_json_identical() {
        let result = create_test_diff_result(true);
        let json = DiffFormatter::format_diff_json(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["identical"], true);
        assert_eq!(parsed["from"], 1000);
        assert_eq!(parsed["to"], 2000);
    }

    #[test]
    fn test_format_diff_json_with_changes() {
        let result = create_test_diff_result(false);
        let json = DiffFormatter::format_diff_json(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["from"]["snapshot_id"], 1000);
        assert_eq!(parsed["to"]["snapshot_id"], 2000);
        assert_eq!(parsed["files"]["added"], 3);
        assert_eq!(parsed["files"]["removed"], 1);
        assert!(parsed["partitions_modified"].is_array());
    }

    #[test]
    fn test_format_diff_text_identical() {
        let result = create_test_diff_result(true);
        let text = DiffFormatter::format_diff_text(&result, "parent", "current");

        assert!(text.contains("No changes"));
    }

    #[test]
    fn test_format_diff_text_with_changes() {
        let result = create_test_diff_result(false);
        let text = DiffFormatter::format_diff_text(&result, "parent", "current");

        // Range header
        assert!(text.contains("1000..2000"));
        // Files (schema not shown when no changes)
        assert!(text.contains("+3 files"));
        assert!(text.contains("-1 file"));
        // Partitions
        assert!(text.contains("Partitions:"));
        assert!(text.contains("date=2024-01-15"));
    }

    #[test]
    fn test_format_diff_text_no_file_changes() {
        let mut result = create_test_diff_result(false);
        result.data_files = DataFileDiff::default();

        let text = DiffFormatter::format_diff_text(&result, "parent", "current");

        assert!(text.contains("No file changes"));
    }

    #[test]
    fn test_format_diff_text_with_schema_changes() {
        let mut result = create_test_diff_result(false);
        result.schema = SchemaDiff {
            columns_added: vec![("new_col".to_string(), "string".to_string())],
            columns_removed: vec![("old_col".to_string(), "int".to_string())],
        };

        let text = DiffFormatter::format_diff_text(&result, "parent", "current");

        assert!(text.contains("new_col"));
        assert!(text.contains("old_col"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(45 * 1024 * 1024), "45.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }
}

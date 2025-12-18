//! History command formatting utilities

use colored::Colorize;

use super::formatter::format_datetime_utc;

/// Formatter for history command results
pub struct HistoryFormatter;

impl HistoryFormatter {
    /// Format history entries as a timeline table
    pub fn format_table(entries: &[HistoryEntryInfo]) -> String {
        if entries.is_empty() {
            return "No history entries found.".to_string();
        }

        let mut lines = Vec::new();

        for entry in entries {
            let marker = if entry.is_current {
                "●".yellow().bold().to_string()
            } else {
                "○".dimmed().to_string()
            };

            let timestamp = format_datetime_utc(&entry.timestamp);

            let op = match entry.operation.as_str() {
                "Append" => "append".green().to_string(),
                "Overwrite" => "overwrite".yellow().to_string(),
                "Delete" => "delete".red().to_string(),
                "Replace" => "replace".cyan().to_string(),
                other => other.to_string(),
            };

            lines.push(format!(
                "{} {} - {} ({})",
                marker,
                entry.version.to_string().cyan().bold(),
                op,
                timestamp.dimmed()
            ));

            // Details line
            if !entry.details.is_empty() {
                lines.push(format!("  {}", entry.details.join(", ").dimmed()));
            }
            lines.push(String::new());
        }

        lines.push(format!("{} snapshots", entries.len()));
        lines.join("\n")
    }

    /// Format history entries as JSON
    pub fn format_json(entries: &[HistoryEntryInfo]) -> Result<String, serde_json::Error> {
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

        serde_json::to_string_pretty(&json_entries)
    }
}

/// History entry information for formatting
#[derive(Debug, Clone)]
pub struct HistoryEntryInfo {
    /// Snapshot version/ID
    pub version: i64,
    /// Timestamp when snapshot was created
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Operation type (Append, Overwrite, Delete, Replace)
    pub operation: String,
    /// Additional details about the snapshot
    pub details: Vec<String>,
    /// Whether this is the current snapshot
    pub is_current: bool,
}

impl HistoryEntryInfo {
    /// Create from core HistoryEntry
    pub fn from_core(entry: &crate::core::operations::HistoryEntry) -> Self {
        Self {
            version: entry.version,
            timestamp: entry.timestamp,
            operation: entry.operation.clone(),
            details: crate::core::operations::HistoryService::format_details(entry),
            is_current: entry.is_current,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<HistoryEntryInfo> {
        vec![
            HistoryEntryInfo {
                version: 3000,
                timestamp: chrono::DateTime::from_timestamp_millis(1700000003000).unwrap(),
                operation: "Append".to_string(),
                details: vec!["10 files added".to_string()],
                is_current: true,
            },
            HistoryEntryInfo {
                version: 2000,
                timestamp: chrono::DateTime::from_timestamp_millis(1700000002000).unwrap(),
                operation: "Overwrite".to_string(),
                details: vec![],
                is_current: false,
            },
        ]
    }

    #[test]
    fn test_format_table_empty() {
        let result = HistoryFormatter::format_table(&[]);
        assert!(result.contains("No history entries"));
    }

    #[test]
    fn test_format_table() {
        let entries = sample_entries();
        let result = HistoryFormatter::format_table(&entries);
        assert!(result.contains("3000"));
        assert!(result.contains("2000"));
        assert!(result.contains("append"));
        assert!(result.contains("overwrite"));
        assert!(result.contains("2 snapshots"));
    }

    #[test]
    fn test_format_table_with_details() {
        let entries = sample_entries();
        let result = HistoryFormatter::format_table(&entries);
        assert!(result.contains("10 files added"));
    }

    #[test]
    fn test_format_json() {
        let entries = sample_entries();
        let result = HistoryFormatter::format_json(&entries).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["snapshot_id"], 3000);
        assert_eq!(parsed[0]["operation"], "Append");
        assert_eq!(parsed[0]["is_current"], true);
    }
}

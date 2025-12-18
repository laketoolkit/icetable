//! Stats command formatting utilities

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::formatter::create_styled_table;
use super::formatter::format_datetime_utc;
use crate::core::operations::{PartitionStats, TableStats};
use crate::core::{format_bytes, format_number};

/// Formatter for stats command results
pub struct StatsFormatter;

impl StatsFormatter {
    /// Format table stats as JSON
    pub fn format_table_stats_json(stats: &TableStats) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "table": stats.table_name,
            "format": stats.format,
            "total_records": stats.total_records,
            "compressed_size_bytes": stats.compressed_size,
            "format_version": stats.format_version,
            "created_at": stats.last_modified.map(|dt| dt.to_rfc3339()),
            "properties": stats.properties,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format table stats as text table
    pub fn format_table_stats_text(stats: &TableStats) -> String {
        let mut output = Vec::new();

        output.push(format!("{}", stats.table_name.cyan().bold()));
        output.push(String::new());

        let mut table = create_styled_table();
        table.set_header(vec![
            Cell::new("Metric".cyan().to_string()).set_alignment(CellAlignment::Left),
            Cell::new("Value".cyan().to_string()).set_alignment(CellAlignment::Right),
        ]);

        if let Some(rows) = stats.total_records {
            table.add_row(vec![
                Cell::new("Total Records").set_alignment(CellAlignment::Left),
                Cell::new(format_number(rows)).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(size) = stats.compressed_size {
            table.add_row(vec![
                Cell::new("Total Size").set_alignment(CellAlignment::Left),
                Cell::new(format_bytes(size)).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(files) = stats.properties.get("total-data-files")
            && let Ok(n) = files.parse::<i64>()
        {
            table.add_row(vec![
                Cell::new("Data Files").set_alignment(CellAlignment::Left),
                Cell::new(format_number(n)).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(ref version) = stats.format_version {
            table.add_row(vec![
                Cell::new("Format Version").set_alignment(CellAlignment::Left),
                Cell::new(version).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(ref dt) = stats.last_modified {
            table.add_row(vec![
                Cell::new("Last Modified").set_alignment(CellAlignment::Left),
                Cell::new(format_datetime_utc(dt)).set_alignment(CellAlignment::Right),
            ]);
        }

        output.push(table.to_string());
        output.join("\n")
    }

    /// Format partition stats as JSON
    pub fn format_partition_stats_json(
        stats: &PartitionStats,
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "table": stats.table_name,
            "format": stats.format,
            "partition_filter": stats.partition_filter,
            "stats": {
                "file_count": stats.file_count,
                "total_size": stats.total_size,
                "avg_file_size": stats.avg_file_size,
                "small_files": stats.small_files,
                "small_files_percent": stats.small_files_percent,
                "recommended_target_size": stats.recommended_target_size,
            }
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format partition stats as text
    pub fn format_partition_stats_text(stats: &PartitionStats) -> String {
        let mut output = Vec::new();

        output.push(String::new());
        output.push(format!("Partition: {}", stats.partition_filter));
        output.push(format!("  Files: {}", stats.file_count));
        output.push(format!("  Total Size: {}", format_bytes(stats.total_size)));
        output.push(format!(
            "  Avg File Size: {}",
            format_bytes(stats.avg_file_size)
        ));
        output.push(format!(
            "  Small Files (<128MB): {} ({:.1}%)",
            stats.small_files, stats.small_files_percent
        ));

        if stats.small_files > 0 && stats.small_files_percent > 50.0 {
            output.push(format!(
                "  {}  Recommend: optimize --target-size {}",
                "⚠".yellow(),
                format_bytes(stats.recommended_target_size)
            ));
        }

        output.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_table_stats() -> TableStats {
        let mut properties = HashMap::new();
        properties.insert("total-data-files".to_string(), "42".to_string());

        TableStats {
            table_name: "test_table".to_string(),
            format: "iceberg".to_string(),
            total_records: Some(100000),
            compressed_size: Some(1024 * 1024 * 500),
            format_version: Some("2".to_string()),
            last_modified: None,
            properties,
        }
    }

    fn create_test_partition_stats() -> PartitionStats {
        PartitionStats {
            table_name: "test_table".to_string(),
            format: "iceberg".to_string(),
            partition_filter: "date=2024-01-01".to_string(),
            file_count: 10,
            total_size: 1024 * 1024 * 100,
            avg_file_size: 1024 * 1024 * 10,
            small_files: 3,
            small_files_percent: 30.0,
            recommended_target_size: 1024 * 1024 * 128,
        }
    }

    #[test]
    fn test_format_table_stats_json() {
        let stats = create_test_table_stats();
        let json = StatsFormatter::format_table_stats_json(&stats).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["table"], "test_table");
        assert_eq!(parsed["format"], "iceberg");
        assert_eq!(parsed["total_records"], 100000);
    }

    #[test]
    fn test_format_table_stats_text() {
        let stats = create_test_table_stats();
        let text = StatsFormatter::format_table_stats_text(&stats);

        assert!(text.contains("test_table"));
        assert!(text.contains("Total Records"));
        assert!(text.contains("Total Size"));
        assert!(text.contains("Data Files"));
        assert!(text.contains("Format Version"));
    }

    #[test]
    fn test_format_partition_stats_json() {
        let stats = create_test_partition_stats();
        let json = StatsFormatter::format_partition_stats_json(&stats).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["table"], "test_table");
        assert_eq!(parsed["partition_filter"], "date=2024-01-01");
        assert_eq!(parsed["stats"]["file_count"], 10);
    }

    #[test]
    fn test_format_partition_stats_text() {
        let stats = create_test_partition_stats();
        let text = StatsFormatter::format_partition_stats_text(&stats);

        assert!(text.contains("Partition: date=2024-01-01"));
        assert!(text.contains("Files: 10"));
        assert!(text.contains("Small Files"));
    }

    #[test]
    fn test_format_partition_stats_text_with_warning() {
        let mut stats = create_test_partition_stats();
        stats.small_files = 8;
        stats.small_files_percent = 80.0;

        let text = StatsFormatter::format_partition_stats_text(&stats);
        assert!(text.contains("Recommend: optimize"));
    }

    #[test]
    fn test_format_partition_stats_text_no_warning() {
        let stats = create_test_partition_stats();
        // Default has 30% small files, should not show warning
        let text = StatsFormatter::format_partition_stats_text(&stats);
        assert!(!text.contains("Recommend: optimize"));
    }

    #[test]
    fn test_format_table_stats_text_empty_properties() {
        let stats = TableStats {
            table_name: "empty_table".to_string(),
            format: "iceberg".to_string(),
            total_records: None,
            compressed_size: None,
            format_version: None,
            last_modified: None,
            properties: HashMap::new(),
        };

        let text = StatsFormatter::format_table_stats_text(&stats);
        assert!(text.contains("empty_table"));
        // Should not contain metrics when they are None
        assert!(!text.contains("Total Records"));
    }
}

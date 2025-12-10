//! Output formatting utilities

use colored::Colorize;
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table, presets};

use super::icons::{SeverityIcon, StatusIcon};

/// Create a styled comfy_table with UTF8_FULL preset and dynamic arrangement
///
/// This is the standard table style used throughout the CLI
pub fn create_styled_table() -> Table {
    let mut table = Table::new();
    table.load_preset(presets::UTF8_FULL);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table
}

/// Create a styled header cell with cyan color and center alignment
///
/// This provides consistent header styling across all CLI tables
pub fn create_header_cell(text: &str) -> Cell {
    Cell::new(text.cyan().to_string()).set_alignment(CellAlignment::Center)
}

/// Create header cells from a slice of strings
///
/// Convenience function for creating multiple header cells at once
pub fn create_header_cells(headers: &[&str]) -> Vec<Cell> {
    headers.iter().map(|h| create_header_cell(h)).collect()
}

/// Format a timestamp from milliseconds to human-readable string with UTC suffix
pub fn format_timestamp_ms(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Format a DateTime<Utc> to human-readable string with UTC suffix
pub fn format_datetime_utc(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Output formatter for different formats
pub struct OutputFormatter;

impl OutputFormatter {
    /// Format output as a table
    pub fn format_table(headers: Vec<String>, rows: Vec<Vec<String>>) -> String {
        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_header(headers);

        for row in rows {
            table.add_row(row);
        }

        table.to_string()
    }

    /// Format output as JSON
    pub fn format_json<T: serde::Serialize>(data: &T) -> crate::error::Result<String> {
        serde_json::to_string_pretty(data).map_err(|e| crate::error::Error::General(e.to_string()))
    }

    /// Format output as YAML
    pub fn format_yaml<T: serde::Serialize>(data: &T) -> crate::error::Result<String> {
        serde_yaml::to_string(data).map_err(|e| crate::error::Error::General(e.to_string()))
    }

    /// Format a success message
    pub fn success(message: &str) -> String {
        format!("{} {}", StatusIcon::Success, message)
    }

    /// Format an error message
    pub fn error(message: &str) -> String {
        format!("{}  {}", SeverityIcon::Error, message)
    }

    /// Format a warning message
    pub fn warning(message: &str) -> String {
        format!("{}  {}", SeverityIcon::Warning, message)
    }

    /// Format an info message
    pub fn info(message: &str) -> String {
        format!("{}  {}", SeverityIcon::Info, message)
    }
}

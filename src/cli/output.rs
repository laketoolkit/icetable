//! Output formatting utilities

use colored::Colorize;
use comfy_table::{presets, Table};

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
        format!("{} {}", "✓".green(), message)
    }

    /// Format an error message
    pub fn error(message: &str) -> String {
        format!("{} {}", "✗".red(), message)
    }

    /// Format a warning message
    pub fn warning(message: &str) -> String {
        format!("{} {}", "⚠".yellow(), message)
    }

    /// Format an info message
    pub fn info(message: &str) -> String {
        format!("{} {}", "ℹ".blue(), message)
    }
}

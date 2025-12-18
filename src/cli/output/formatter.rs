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

/// Format a `DateTime<Utc>` to human-readable string with UTC suffix
pub fn format_datetime_utc(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Trait for types that can be displayed in CLI output
///
/// Implement this trait for result types that need to be displayed
/// in both text and JSON formats. The trait requires `Serialize` for JSON output.
///
/// # Example
///
/// ```ignore
/// use icetable::cli::output::CliOutput;
///
/// struct MyResult {
///     count: u32,
///     message: String,
/// }
///
/// impl CliOutput for MyResult {
///     fn format_text(&self) -> String {
///         format!("Count: {}\nMessage: {}", self.count, self.message)
///     }
/// }
///
/// // In command handler:
/// let result = MyResult { count: 42, message: "done".into() };
/// output_result(&result, output_format)?;
/// ```
pub trait CliOutput: serde::Serialize {
    /// Format the result as human-readable text
    fn format_text(&self) -> String;
}

/// Output a result in the specified format
///
/// This is the standard way to output command results. It handles
/// JSON vs text formatting automatically based on the output format.
pub fn output_result<T: CliOutput>(result: &T, format: &str) -> crate::error::Result<()> {
    match format {
        "json" => {
            let json = serde_json::to_string_pretty(result).map_err(|e| {
                crate::error::Error::Serialization {
                    message: format!("JSON serialization failed: {}", e),
                }
            })?;
            println!("{}", json);
        }
        "yaml" => {
            let yaml = serde_yaml_ng::to_string(result).map_err(|e| {
                crate::error::Error::Serialization {
                    message: format!("YAML serialization failed: {}", e),
                }
            })?;
            print!("{}", yaml);
        }
        _ => {
            println!("{}", result.format_text());
        }
    }
    Ok(())
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
        serde_json::to_string_pretty(data).map_err(|e| crate::error::Error::Serialization {
            message: format!("JSON serialization failed: {}", e),
        })
    }

    /// Format output as YAML
    pub fn format_yaml<T: serde::Serialize>(data: &T) -> crate::error::Result<String> {
        serde_yaml_ng::to_string(data).map_err(|e| crate::error::Error::Serialization {
            message: format!("YAML serialization failed: {}", e),
        })
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

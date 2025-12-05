//! Output formatting utilities

use comfy_table::{Table, presets};
use unicode_width::UnicodeWidthStr;

use crate::utils::{visual_width, wrap_line};

use super::icons::{SeverityIcon, StatusIcon};

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

    /// Create a framed box with title and content lines
    ///
    /// # Arguments
    /// * `title` - Optional title to display centered in the top border
    /// * `lines` - Vector of content lines to display in the box
    /// * `width` - Fixed width of the box (default 100)
    ///
    /// # Returns
    /// A formatted string with the framed content
    pub fn framed_box(title: Option<&str>, lines: Vec<String>, width: Option<usize>) -> String {
        let box_width = width.unwrap_or(100);
        let content_width = box_width - 2; // -2 for left and right borders

        let mut output = Vec::new();

        // Top border with optional centered title
        if let Some(t) = title {
            let title_width = UnicodeWidthStr::width(t);
            let padding_total = content_width.saturating_sub(title_width);
            let padding_left = padding_total / 2;
            let padding_right = padding_total - padding_left;
            output.push(format!(
                "┌{}{}{}┐",
                "─".repeat(padding_left),
                t,
                "─".repeat(padding_right)
            ));
        } else {
            output.push(format!("┌{}┐", "─".repeat(content_width)));
        }

        // Empty line after title
        output.push(format!("│{:width$}│", "", width = content_width));

        // Content lines
        for line in lines {
            if line.is_empty() {
                // Empty line
                output.push(format!("│{:width$}│", "", width = content_width));
            } else {
                let line_width = visual_width(&line);

                if line_width <= content_width {
                    // Line fits, pad it
                    let padding = content_width - line_width;
                    output.push(format!("│{}{:width$}│", line, "", width = padding));
                } else {
                    // Line too long, wrap it
                    let wrapped = wrap_line(&line, content_width);
                    for wrapped_line in wrapped {
                        let wrapped_width = visual_width(&wrapped_line);
                        let padding = content_width.saturating_sub(wrapped_width);
                        output.push(format!("│{}{:width$}│", wrapped_line, "", width = padding));
                    }
                }
            }
        }

        // Empty line before bottom
        output.push(format!("│{:width$}│", "", width = content_width));

        // Bottom border
        output.push(format!("└{}┘", "─".repeat(content_width)));

        output.join("\n")
    }
}

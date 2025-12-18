//! CliOutput implementations for core result types
//!
//! This module provides `CliOutput` implementations for result types from core,
//! enabling them to be used with `output_result()` for standardized formatting.

use colored::Colorize;

use super::formatter::CliOutput;
use crate::core::format_bytes;
use crate::core::metadata::{MaintenanceResult, SnapshotInfo};
use crate::core::operations::validate::ValidateResult;

// ═══════════════════════════════════════════════════════════════════════════════
// ValidateResult
// ═══════════════════════════════════════════════════════════════════════════════

impl CliOutput for ValidateResult {
    fn format_text(&self) -> String {
        let mut lines = Vec::new();

        // Status header
        let status = if self.is_valid {
            format!("{} Valid {}", "✓".green(), self.format_name.cyan())
        } else {
            format!("{} Invalid {}", "✗".red(), self.format_name.cyan())
        };
        lines.push(status);

        // File info if available
        if let Some(rows) = self.num_rows {
            lines.push(format!("  Rows: {}", rows.to_string().white()));
        }
        if let Some(size) = self.file_size {
            lines.push(format!("  Size: {}", format_bytes(size).white()));
        }

        // Mode indicator
        if self.quick_mode {
            lines.push(format!("  Mode: {}", "quick".dimmed()));
        }

        // Errors
        if !self.errors.is_empty() {
            lines.push(String::new());
            lines.push(format!("{}", "Errors:".red().bold()));
            for error in &self.errors {
                lines.push(format!("  {} {}", "•".red(), error));
            }
        }

        // Warnings
        if !self.warnings.is_empty() {
            lines.push(String::new());
            lines.push(format!("{}", "Warnings:".yellow().bold()));
            for warning in &self.warnings {
                lines.push(format!("  {} {}", "•".yellow(), warning));
            }
        }

        // Recommendations
        if !self.recommendations.is_empty() {
            lines.push(String::new());
            lines.push(format!("{}", "Recommendations:".cyan().bold()));
            for rec in &self.recommendations {
                lines.push(format!("  {} {}", "→".cyan(), rec));
            }
        }

        lines.join("\n")
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MaintenanceResult
// ═══════════════════════════════════════════════════════════════════════════════

impl CliOutput for MaintenanceResult {
    fn format_text(&self) -> String {
        let mut lines = Vec::new();

        // Operation header
        lines.push(format!(
            "{} {} completed",
            "✓".green(),
            self.operation.cyan().bold()
        ));

        // Changes summary
        if self.files_added > 0 || self.files_removed > 0 {
            lines.push(String::new());
            lines.push("Changes:".white().bold().to_string());

            if self.files_added > 0 {
                lines.push(format!(
                    "  {} {} files added ({})",
                    "+".green(),
                    self.files_added,
                    format_bytes(self.bytes_added)
                ));
            }
            if self.files_removed > 0 {
                lines.push(format!(
                    "  {} {} files removed ({})",
                    "-".red(),
                    self.files_removed,
                    format_bytes(self.bytes_removed)
                ));
            }

            // Net change
            let delta = self.bytes_delta();
            if delta != 0 {
                let delta_str = if delta > 0 {
                    format!("+{}", format_bytes(delta as u64)).red().to_string()
                } else {
                    format!("-{}", format_bytes((-delta) as u64))
                        .green()
                        .to_string()
                };
                lines.push(format!("  Net: {}", delta_str));
            }
        } else {
            lines.push(format!("  {}", "No changes made".dimmed()));
        }

        // Records affected
        if self.records_affected > 0 {
            lines.push(format!(
                "  Records affected: {}",
                self.records_affected.to_string().white()
            ));
        }

        // Additional details
        if !self.details.is_empty() {
            lines.push(String::new());
            for (key, value) in &self.details {
                lines.push(format!("  {}: {}", key.dimmed(), value));
            }
        }

        lines.join("\n")
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SnapshotInfo (from metadata traits)
// ═══════════════════════════════════════════════════════════════════════════════

impl CliOutput for SnapshotInfo {
    fn format_text(&self) -> String {
        use super::format_timestamp_ms;

        let mut lines = Vec::new();

        lines.push(format!(
            "Snapshot {} ({})",
            self.id.to_string().cyan().bold(),
            self.operation.white()
        ));
        lines.push(format!(
            "  Timestamp: {}",
            format_timestamp_ms(self.timestamp_ms)
        ));

        if let Some(parent) = self.parent_id {
            lines.push(format!("  Parent: {}", parent.to_string().dimmed()));
        }

        if !self.summary.is_empty() {
            lines.push("  Summary:".to_string());
            for (key, value) in &self.summary {
                lines.push(format!("    {}: {}", key.dimmed(), value));
            }
        }

        lines.join("\n")
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Vec<T> wrapper for lists
// ═══════════════════════════════════════════════════════════════════════════════

/// Wrapper for outputting a list of items that implement CliOutput
#[derive(serde::Serialize)]
pub struct ListOutput<T: serde::Serialize> {
    /// The list of items
    pub items: Vec<T>,
    /// Total count (may differ from items.len() if paginated)
    pub total: usize,
}

impl<T: CliOutput + serde::Serialize> CliOutput for ListOutput<T> {
    fn format_text(&self) -> String {
        if self.items.is_empty() {
            return "No items found.".dimmed().to_string();
        }

        let mut lines: Vec<String> = self.items.iter().map(|item| item.format_text()).collect();

        lines.push(String::new());
        lines.push(format!("Total: {} items", self.total).dimmed().to_string());

        lines.join("\n\n")
    }
}

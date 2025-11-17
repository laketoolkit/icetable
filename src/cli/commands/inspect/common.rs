//! Common types and utilities for inspect command

use crate::cli::output::{Box, BoxItem, BoxLayout, BoxRenderer, BoxSection};
use std::path::Path;

/// Verbosity level for inspection output
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VerbosityLevel {
    /// Normal output
    Normal = 0,
    /// Verbose output (-v) - all details
    Verbose = 1,
}

/// Options for physical layout inspection
#[derive(Debug, Clone)]
pub struct PhysicalInspectOptions {
    /// Show schema section
    pub show_schema: bool,
    /// Show physical layout section
    pub show_layout: bool,
    /// Show statistics section
    pub show_stats: bool,
    /// Verbosity level
    pub verbosity: VerbosityLevel,
}

impl PhysicalInspectOptions {
    /// Create options from CLI args
    pub fn from_cli_args(
        schema: bool,
        layout: bool,
        stats: bool,
        verbosity: VerbosityLevel,
    ) -> Self {
        let any_flag = schema || layout || stats;

        Self {
            show_schema: if any_flag { schema } else { true },
            show_layout: if any_flag { layout } else { true },
            show_stats: if any_flag { stats } else { true },
            verbosity,
        }
    }
}

/// Result of physical layout inspection
pub struct PhysicalInspectResult {
    /// File information section
    pub file_info: Vec<BoxItem>,
    /// Schema section (optional)
    pub schema: Option<Vec<BoxItem>>,
    /// Physical layout section (optional)
    pub layout: Option<Vec<BoxItem>>,
    /// Statistics section (optional)
    pub statistics: Option<Vec<BoxItem>>,
    /// Custom title for statistics section (defaults to "Statistics")
    pub stats_title: Option<String>,
}

impl PhysicalInspectResult {
    /// Render the result to string
    pub fn render(&self, title: &str) -> String {
        let layout = BoxLayout::new(100);
        let renderer = BoxRenderer::new(layout);

        let mut container = Box::titled(title);

        // File Information section
        container = container.section(
            BoxSection::titled("File Information")
                .items(self.file_info.clone())
        );

        // Schema section
        if let Some(schema_items) = &self.schema {
            container = container.section(
                BoxSection::titled("Schema")
                    .items(schema_items.clone())
            );
        }

        // Physical Layout section
        if let Some(layout_items) = &self.layout {
            container = container.section(
                BoxSection::titled("Physical Layout")
                    .items(layout_items.clone())
            );
        }

        // Statistics section (or custom title if provided)
        if let Some(stats_items) = &self.statistics {
            let title = self.stats_title.as_deref().unwrap_or("Statistics");
            container = container.section(
                BoxSection::titled(title)
                    .items(stats_items.clone())
            );
        }

        renderer.render(container)
    }
}

/// Helper functions for creating BoxItems

/// Create a key-value item with aligned keys
pub fn kv_item(key: &str, value: impl std::fmt::Display, key_width: usize) -> BoxItem {
    BoxItem::KeyValue {
        key: key.to_string(),
        value: value.to_string(),
        key_width: Some(key_width),
    }
}

/// Create a simple text item
pub fn text_item(text: impl Into<String>) -> BoxItem {
    BoxItem::Text(text.into())
}

/// Format file size in human-readable format
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Format number with thousands separators
pub fn format_number(n: i64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 && *c != '-' {
            result.push(',');
        }
        result.push(*c);
    }

    result
}

/// Format percentage
#[allow(dead_code)]
pub fn format_percentage(value: f64) -> String {
    format!("{:.2}%", value * 100.0)
}

/// Get file name from path
pub fn get_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Format compression ratio
pub fn format_compression_ratio(compressed: u64, uncompressed: u64) -> String {
    if uncompressed == 0 {
        return "N/A".to_string();
    }
    let ratio = compressed as f64 / uncompressed as f64;
    format!("{:.2}x", 1.0 / ratio)
}

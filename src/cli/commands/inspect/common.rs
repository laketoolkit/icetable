//! Common types and utilities for inspect command

use crate::cli::output::{Box, BoxItem, BoxLayout, BoxRenderer, BoxSection};

// Re-export format_bytes from core for convenience
pub use crate::core::format_bytes;

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
    /// Deep scan: check all snapshots for orphan detection
    pub deep_scan: bool,
}

impl PhysicalInspectOptions {
    /// Create options from CLI args
    pub fn from_cli_args(
        schema: bool,
        layout: bool,
        stats: bool,
        verbosity: VerbosityLevel,
        deep_scan: bool,
    ) -> Self {
        let any_flag = schema || layout || stats;

        Self {
            show_schema: if any_flag { schema } else { true },
            show_layout: if any_flag { layout } else { true },
            show_stats: if any_flag { stats } else { true },
            verbosity,
            deep_scan,
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

        // Table Information section
        container = container
            .section(BoxSection::titled("Table Information").items(self.file_info.clone()));

        // Schema section
        if let Some(schema_items) = &self.schema {
            container = container.section(BoxSection::titled("Schema").items(schema_items.clone()));
        }

        // Physical Layout section
        if let Some(layout_items) = &self.layout {
            container = container
                .section(BoxSection::titled("Physical Layout").items(layout_items.clone()));
        }

        // Statistics section (or custom title if provided)
        if let Some(stats_items) = &self.statistics {
            let title = self.stats_title.as_deref().unwrap_or("Statistics");
            container = container.section(BoxSection::titled(title).items(stats_items.clone()));
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

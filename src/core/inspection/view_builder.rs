//! View builder for converting metadata to presentation-ready structure

use super::formatters::*;
use super::traits::*;
use std::collections::HashMap;

/// View section containing items
#[derive(Debug, Clone)]
pub struct ViewSection {
    /// Section title
    pub title: String,
    /// Section items
    pub items: Vec<ViewItem>,
}

/// View item (format-agnostic representation)
#[derive(Debug, Clone)]
pub enum ViewItem {
    /// Key-value pair with optional key width
    KeyValue {
        key: String,
        value: String,
        key_width: Option<usize>,
    },
    /// Plain text
    Text(String),
    /// Empty line
    Empty,
    /// List of items
    List(Vec<String>),
    /// Table with headers and rows
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

impl ViewItem {
    /// Create a key-value item with default key width
    pub fn kv(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::KeyValue {
            key: key.into(),
            value: value.into(),
            key_width: Some(20),
        }
    }

    /// Create a key-value item with custom key width
    pub fn kv_width(key: impl Into<String>, value: impl Into<String>, width: usize) -> Self {
        Self::KeyValue {
            key: key.into(),
            value: value.into(),
            key_width: Some(width),
        }
    }

    /// Create a text item
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Create an empty item
    pub fn empty() -> Self {
        Self::Empty
    }
}

/// Complete inspection view
#[derive(Debug, Clone)]
pub struct InspectionView {
    /// View sections
    pub sections: Vec<ViewSection>,
}

/// Builder for creating inspection views from metadata
pub struct InspectionViewBuilder {
    sections: Vec<ViewSection>,
}

impl InspectionViewBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
        }
    }

    /// Add file information section
    pub fn with_file_info(mut self, info: &FileInfo) -> Self {
        let mut items = vec![
            ViewItem::kv("Path", extract_filename(&info.path)),
            ViewItem::kv("Size", format_bytes(info.file_size)),
            ViewItem::kv("Format Version", &info.format_version),
        ];

        if let Some(created_by) = &info.created_by {
            items.push(ViewItem::kv("Created By", created_by));
        }

        // Add any additional metadata
        for (key, value) in &info.metadata {
            items.push(ViewItem::kv(key, value));
        }

        self.sections.push(ViewSection {
            title: "File Information".to_string(),
            items,
        });
        self
    }

    /// Add schema section
    pub fn with_schema(mut self, schema: &SchemaInfo) -> Self {
        let mut items = vec![
            ViewItem::kv("Columns", format_number(schema.num_columns as i64)),
            ViewItem::empty(),
        ];

        // Add column definitions
        for col in &schema.columns {
            let nullable = if col.nullable { "" } else { "NOT NULL" };
            items.push(ViewItem::text(format!(
                "  {}.  {:<30} {:<20} {}",
                col.index + 1,
                col.name,
                col.column_type,
                nullable
            )));
        }

        self.sections.push(ViewSection {
            title: "Schema".to_string(),
            items,
        });
        self
    }

    /// Add layout section
    pub fn with_layout(self, layout: &LayoutInfo) -> Self {
        match layout {
            LayoutInfo::RowGroupBased(rg) => self.add_rowgroup_layout(rg),
            LayoutInfo::BatchBased(batch) => self.add_batch_layout(batch),
            LayoutInfo::FileBased(files) => self.add_file_layout(files),
            LayoutInfo::Unstructured(uns) => self.add_unstructured_layout(uns),
        }
    }

    /// Add statistics section
    pub fn with_statistics(mut self, stats: &StatisticsInfo) -> Self {
        let mut items = vec![
            ViewItem::kv("Total Rows", format_number(stats.total_rows)),
            ViewItem::kv("Compressed Size", format_bytes(stats.compressed_size)),
            ViewItem::kv("Uncompressed Size", format_bytes(stats.uncompressed_size)),
        ];

        if stats.uncompressed_size > 0 {
            items.push(ViewItem::kv(
                "Compression Ratio",
                format_compression_ratio(stats.compressed_size, stats.uncompressed_size),
            ));
        }

        // Add column statistics if available
        if !stats.column_stats.is_empty() {
            items.push(ViewItem::empty());
            items.push(ViewItem::text("Column Statistics:"));

            for col_stat in &stats.column_stats {
                items.push(ViewItem::empty());
                items.push(ViewItem::text(format!("  {}", col_stat.column_name)));

                if let Some(null_count) = col_stat.null_count {
                    items.push(ViewItem::text(format!(
                        "    Null Count: {}",
                        format_number(null_count)
                    )));
                }

                if let Some(min) = &col_stat.min_value {
                    items.push(ViewItem::text(format!("    Min: {}", min)));
                }

                if let Some(max) = &col_stat.max_value {
                    items.push(ViewItem::text(format!("    Max: {}", max)));
                }

                if let Some(distinct) = col_stat.distinct_count {
                    items.push(ViewItem::text(format!(
                        "    Distinct: {}",
                        format_number(distinct)
                    )));
                }
            }
        }

        self.sections.push(ViewSection {
            title: "Statistics".to_string(),
            items,
        });
        self
    }

    /// Add row group layout (Parquet)
    fn add_rowgroup_layout(mut self, rg: &RowGroupLayout) -> Self {
        let mut items = vec![
            ViewItem::kv("Row Groups", format_number(rg.num_row_groups as i64)),
            ViewItem::empty(),
        ];

        for row_group in &rg.row_groups {
            items.push(ViewItem::text(format!(
                "Row Group {}:",
                format_number(row_group.index as i64)
            )));
            items.push(ViewItem::text(format!(
                "  Rows: {}",
                format_number(row_group.num_rows)
            )));
            items.push(ViewItem::text(format!(
                "  Compressed: {}",
                format_bytes(row_group.total_compressed_size as u64)
            )));
            items.push(ViewItem::text(format!(
                "  Uncompressed: {}",
                format_bytes(row_group.total_uncompressed_size as u64)
            )));

            if !row_group.columns.is_empty() {
                items.push(ViewItem::text("  Columns:"));
                for col in &row_group.columns {
                    items.push(ViewItem::text(format!(
                        "    {} - {} - {} -> {}",
                        col.column_name,
                        col.compression,
                        format_bytes(col.uncompressed_size as u64),
                        format_bytes(col.compressed_size as u64)
                    )));
                }
            }

            items.push(ViewItem::empty());
        }

        self.sections.push(ViewSection {
            title: "Physical Layout".to_string(),
            items,
        });
        self
    }

    /// Add batch layout (Arrow IPC)
    fn add_batch_layout(mut self, batch: &BatchLayout) -> Self {
        let mut items = vec![
            ViewItem::kv("Record Batches", format_number(batch.num_batches as i64)),
            ViewItem::empty(),
        ];

        for b in &batch.batches {
            items.push(ViewItem::text(format!("Batch {}:", b.index)));
            items.push(ViewItem::text(format!(
                "  Rows: {}",
                format_number(b.num_rows as i64)
            )));
            items.push(ViewItem::text(format!(
                "  Metadata: {}",
                format_bytes(b.metadata_length)
            )));
            items.push(ViewItem::text(format!(
                "  Body: {}",
                format_bytes(b.body_length)
            )));
            items.push(ViewItem::empty());
        }

        self.sections.push(ViewSection {
            title: "Physical Layout".to_string(),
            items,
        });
        self
    }

    /// Add file-based layout (Delta/Iceberg)
    fn add_file_layout(mut self, files: &FileBasedLayout) -> Self {
        let mut items = vec![
            ViewItem::kv("Data Files", format_number(files.num_files as i64)),
            ViewItem::kv("Total Size", format_bytes(files.total_size)),
        ];

        if let Some(partitioning) = &files.partitioning {
            items.push(ViewItem::kv("Partitioning", partitioning));
        }

        // Add any additional details
        for (key, value) in &files.details {
            items.push(ViewItem::kv(key, value));
        }

        self.sections.push(ViewSection {
            title: "Physical Layout".to_string(),
            items,
        });
        self
    }

    /// Add unstructured layout (CSV/JSON)
    fn add_unstructured_layout(mut self, uns: &UnstructuredLayout) -> Self {
        let mut items = Vec::new();

        if let Some(records) = uns.estimated_records {
            items.push(ViewItem::kv(
                "Estimated Records",
                format_number(records),
            ));
        }

        if let Some(delimiter) = uns.delimiter {
            items.push(ViewItem::kv("Delimiter", format!("'{}'", delimiter)));
        }

        if let Some(has_header) = uns.has_header {
            items.push(ViewItem::kv(
                "Has Header",
                if has_header { "Yes" } else { "No" },
            ));
        }

        self.sections.push(ViewSection {
            title: "Layout".to_string(),
            items,
        });
        self
    }

    /// Build the final inspection view
    pub fn build(self) -> InspectionView {
        InspectionView {
            sections: self.sections,
        }
    }
}

impl Default for InspectionViewBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert InspectionView to CLI BoxItems for rendering
pub fn view_to_box_items(view: &InspectionView) -> Vec<crate::cli::output::BoxItem> {
    use crate::cli::output::BoxItem;

    let mut items = Vec::new();

    for section in &view.sections {
        // Add section title
        items.push(BoxItem::Text(format!("══════ {} ══════", section.title)));
        items.push(BoxItem::Empty);

        // Add section items
        for item in &section.items {
            match item {
                ViewItem::KeyValue { key, value, key_width } => {
                    items.push(BoxItem::KeyValue {
                        key: key.clone(),
                        value: value.clone(),
                        key_width: *key_width,
                    });
                }
                ViewItem::Text(text) => {
                    items.push(BoxItem::Text(text.clone()));
                }
                ViewItem::Empty => {
                    items.push(BoxItem::Empty);
                }
                ViewItem::List(list_items) => {
                    for list_item in list_items {
                        items.push(BoxItem::Text(format!("  • {}", list_item)));
                    }
                }
                ViewItem::Table { headers, rows } => {
                    // Create table header
                    let header_text = headers.join("  ");
                    items.push(BoxItem::Text(header_text));
                    items.push(BoxItem::Separator);

                    // Create table rows
                    for row in rows {
                        let row_text = row.join("  ");
                        items.push(BoxItem::Text(row_text));
                    }
                }
            }
        }

        items.push(BoxItem::Empty);
    }

    items
}

/// Convert InspectionView to PhysicalInspectResult for CLI rendering
pub fn view_to_inspect_result(view: &InspectionView) -> crate::cli::commands::inspect::common::PhysicalInspectResult {
    use crate::cli::output::BoxItem;

    let mut file_info = Vec::new();
    let mut schema = None;
    let mut layout = None;
    let mut statistics = None;

    for section in &view.sections {
        let mut section_items = Vec::new();

        // Convert section items to BoxItems
        for item in &section.items {
            match item {
                ViewItem::KeyValue { key, value, key_width } => {
                    section_items.push(BoxItem::KeyValue {
                        key: key.clone(),
                        value: value.clone(),
                        key_width: *key_width,
                    });
                }
                ViewItem::Text(text) => {
                    section_items.push(BoxItem::Text(text.clone()));
                }
                ViewItem::Empty => {
                    section_items.push(BoxItem::Empty);
                }
                ViewItem::List(list_items) => {
                    for list_item in list_items {
                        section_items.push(BoxItem::Text(format!("  • {}", list_item)));
                    }
                }
                ViewItem::Table { headers, rows } => {
                    // Create table header
                    let header_text = headers.join("  ");
                    section_items.push(BoxItem::Text(header_text));
                    section_items.push(BoxItem::Separator);

                    // Create table rows
                    for row in rows {
                        let row_text = row.join("  ");
                        section_items.push(BoxItem::Text(row_text));
                    }
                }
            }
        }

        // Assign to appropriate section
        match section.title.as_str() {
            "File Information" => {
                file_info = section_items;
            }
            "Schema" => {
                schema = Some(section_items);
            }
            "Physical Layout" | "Layout" => {
                layout = Some(section_items);
            }
            "Statistics" | "File Contents" => {
                statistics = Some(section_items);
            }
            _ => {
                // Unknown section - add to statistics
                if statistics.is_none() {
                    statistics = Some(Vec::new());
                }
                if let Some(ref mut stats) = statistics {
                    stats.push(BoxItem::Text(format!("══════ {} ══════", section.title)));
                    stats.push(BoxItem::Empty);
                    stats.extend(section_items);
                }
            }
        }
    }

    crate::cli::commands::inspect::common::PhysicalInspectResult {
        file_info,
        schema,
        layout,
        statistics,
        stats_title: None,
    }
}

//! CSV format detection and inspection

use std::path::Path;
use std::sync::Arc;

use crate::cli::output::BoxItem;
use crate::core::storage::{GetOptions, StorageBackend};
use crate::error::{Error, Result};

use super::common::*;

/// Inspect CSV file format
pub async fn inspect_csv_layout(
    path: &Path,
    storage: Arc<dyn StorageBackend>,
    options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    // Read first chunk to detect format
    let path_str = path.to_str().ok_or_else(|| {
        Error::General(format!("Invalid path: {}", path.display()))
    })?;
    let data = storage.get(path_str, &GetOptions::default()).await?;
    let preview = String::from_utf8_lossy(&data[..data.len().min(8192)]);

    // Build file information
    let file_info = build_file_info(path, data.len());

    // Build schema section - basic
    let schema = if options.show_schema {
        Some(build_schema_section(&preview))
    } else {
        None
    };

    // Build layout section - format detection
    let layout = if options.show_layout {
        use super::common::VerbosityLevel;
        Some(build_layout_section(&preview, options.verbosity >= VerbosityLevel::Verbose))
    } else {
        None
    };

    // Build statistics section
    let statistics = if options.show_stats {
        Some(build_statistics_section(&data))
    } else {
        None
    };

    Ok(PhysicalInspectResult {
        file_info,
        schema,
        layout,
        statistics,
        stats_title: None,
    })
}

fn build_file_info(path: &Path, file_size: usize) -> Vec<BoxItem> {
    let mut items = Vec::new();

    items.push(kv_item("Path", get_file_name(path), 20));
    items.push(kv_item("Format", "CSV", 20));
    items.push(kv_item("File Size", format_size(file_size as u64), 20));

    items
}

fn build_schema_section(preview: &str) -> Vec<BoxItem> {
    let mut items = Vec::new();

    // Detect delimiter
    let delimiter = detect_delimiter(preview);

    // Try to extract header
    if let Some(header_line) = preview.lines().next() {
        let fields: Vec<&str> = header_line.split(delimiter).collect();

        items.push(text_item(format!("Detected Fields: {}", fields.len())));
        items.push(BoxItem::Empty);

        for (idx, field) in fields.iter().take(20).enumerate() {
            let field_name = field.trim_matches('"').trim();
            items.push(text_item(format!(
                "  {:<3} {}",
                format!("{}.", idx + 1),
                field_name
            )));
        }

        if fields.len() > 20 {
            items.push(text_item(format!("  ... and {} more fields", fields.len() - 20)));
        }
    }

    items
}

fn build_layout_section(preview: &str, verbose: bool) -> Vec<BoxItem> {
    let mut items = Vec::new();

    // Delimiter detection
    let delimiter = detect_delimiter(preview);
    let delimiter_name = match delimiter {
        ',' => "Comma (,)",
        '\t' => "Tab (\\t)",
        ';' => "Semicolon (;)",
        '|' => "Pipe (|)",
        _ => "Other",
    };

    items.push(text_item("Format Detection:"));
    items.push(BoxItem::Empty);
    items.push(kv_item("Delimiter", delimiter_name, 25));

    // Encoding detection (simplified)
    let encoding = detect_encoding(preview);
    items.push(kv_item("Encoding", encoding, 25));

    // Quote character detection
    let has_quotes = preview.contains('"');
    items.push(kv_item("Quote Char", if has_quotes { "Double quote (\")" } else { "None detected" }, 25));

    // Header detection
    let has_header = detect_header(preview, delimiter);
    items.push(kv_item("Header Row", if has_header { "Detected" } else { "Not detected" }, 25));

    items.push(BoxItem::Empty);

    // Line information
    items.push(text_item("Line Information:"));
    items.push(BoxItem::Empty);

    let lines: Vec<&str> = preview.lines().collect();
    items.push(kv_item("Sample Lines", lines.len().min(100), 25));

    if !lines.is_empty() {
        let first_line_len = lines[0].len();
        items.push(kv_item("First Line Length", first_line_len, 25));
    }

    // Check for consistent column count
    if verbose && lines.len() > 1 {
        let mut col_counts = std::collections::HashMap::new();
        for line in lines.iter().take(20) {
            let count = line.split(delimiter).count();
            *col_counts.entry(count).or_insert(0) += 1;
        }

        items.push(BoxItem::Empty);
        items.push(text_item("Column Count Distribution (first 20 lines):"));
        for (count, frequency) in col_counts.iter() {
            items.push(text_item(format!("  {} columns: {} lines", count, frequency)));
        }
    }

    items
}

fn build_statistics_section(data: &[u8]) -> Vec<BoxItem> {
    let mut items = Vec::new();

    items.push(kv_item("File Size", format_size(data.len() as u64), 30));

    // Count lines (approximate)
    let content = String::from_utf8_lossy(data);
    let line_count = content.lines().count();
    items.push(kv_item("Total Lines", format_number(line_count as i64), 30));

    // Estimate rows (subtract header if present)
    let delimiter = detect_delimiter(&content);
    let has_header = detect_header(&content, delimiter);
    let estimated_rows = if has_header && line_count > 0 {
        line_count - 1
    } else {
        line_count
    };

    items.push(kv_item("Estimated Rows", format_number(estimated_rows as i64), 30));

    items.push(BoxItem::Empty);

    // Average line size
    if line_count > 0 {
        let avg_line_size = data.len() / line_count;
        items.push(kv_item(
            "Avg Line Size",
            format_size(avg_line_size as u64),
            30,
        ));
    }

    items
}

fn detect_delimiter(content: &str) -> char {
    // Count occurrences of common delimiters in first few lines
    let sample: String = content.lines().take(5).collect::<Vec<_>>().join("\n");

    let comma_count = sample.matches(',').count();
    let tab_count = sample.matches('\t').count();
    let semicolon_count = sample.matches(';').count();
    let pipe_count = sample.matches('|').count();

    // Return the most common delimiter
    let max = comma_count.max(tab_count).max(semicolon_count).max(pipe_count);

    if max == comma_count {
        ','
    } else if max == tab_count {
        '\t'
    } else if max == semicolon_count {
        ';'
    } else if max == pipe_count {
        '|'
    } else {
        ',' // default
    }
}

fn detect_encoding(content: &str) -> &'static str {
    // Simplified encoding detection
    if content.chars().all(|c| c.is_ascii()) {
        "ASCII"
    } else {
        "UTF-8"
    }
}

fn detect_header(content: &str, delimiter: char) -> bool {
    let mut lines = content.lines();

    if let (Some(first), Some(second)) = (lines.next(), lines.next()) {
        let first_fields: Vec<&str> = first.split(delimiter).collect();
        let second_fields: Vec<&str> = second.split(delimiter).collect();

        // If field counts differ significantly, likely no header
        if first_fields.len() != second_fields.len() {
            return false;
        }

        // Check if first line fields are mostly non-numeric (likely header)
        let first_numeric_count = first_fields
            .iter()
            .filter(|f| f.trim().parse::<f64>().is_ok())
            .count();

        let second_numeric_count = second_fields
            .iter()
            .filter(|f| f.trim().parse::<f64>().is_ok())
            .count();

        // If first line has significantly fewer numeric values, it's likely a header
        first_numeric_count < second_numeric_count || first_numeric_count == 0
    } else {
        false
    }
}

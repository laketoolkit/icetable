//! JSON format detection and inspection

use std::path::Path;
use std::sync::Arc;

use serde_json::Value;

use crate::cli::output::BoxItem;
use crate::core::storage::{GetOptions, StorageBackend};
use crate::error::{Error, Result};

use super::common::*;

/// JSON format type
#[derive(Debug, Clone, Copy)]
enum JsonFormat {
    /// Newline-delimited JSON (one object per line)
    NdJson,
    /// JSON array of objects
    Array,
    /// Single JSON object
    Object,
}

/// Inspect JSON file format
pub async fn inspect_json_layout(
    path: &Path,
    storage: Arc<dyn StorageBackend>,
    options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    // Read the file
    let path_str = path.to_str().ok_or_else(|| {
        Error::General(format!("Invalid path: {}", path.display()))
    })?;
    let data = storage.get(path_str, &GetOptions::default()).await?;
    let content = String::from_utf8_lossy(&data);

    // Detect format
    let format = detect_json_format(&content)?;

    // Build file information
    let file_info = build_file_info(path, data.len(), format);

    // Build schema section
    let schema = if options.show_schema {
        Some(build_schema_section(&content, format)?)
    } else {
        None
    };

    // Build layout section
    let layout = if options.show_layout {
        Some(build_layout_section(&content, format, options.verbose)?)
    } else {
        None
    };

    // Build statistics section
    let statistics = if options.show_stats {
        Some(build_statistics_section(&content, format)?)
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

fn build_file_info(path: &Path, file_size: usize, format: JsonFormat) -> Vec<BoxItem> {
    let mut items = Vec::new();

    items.push(kv_item("Path", get_file_name(path), 20));
    items.push(kv_item("Format", "JSON", 20));

    let format_name = match format {
        JsonFormat::NdJson => "Newline-Delimited JSON",
        JsonFormat::Array => "JSON Array",
        JsonFormat::Object => "Single JSON Object",
    };
    items.push(kv_item("JSON Type", format_name, 20));

    items.push(kv_item("File Size", format_size(file_size as u64), 20));

    items
}

fn build_schema_section(content: &str, format: JsonFormat) -> Result<Vec<BoxItem>> {
    let mut items = Vec::new();

    match format {
        JsonFormat::NdJson => {
            // Parse first object to infer schema
            if let Some(first_line) = content.lines().next() {
                if let Ok(obj) = serde_json::from_str::<Value>(first_line) {
                    items.push(text_item("Inferred Schema (from first record):"));
                    items.push(BoxItem::Empty);
                    add_schema_fields(&mut items, &obj, 0);
                }
            }
        }
        JsonFormat::Array => {
            // Parse array and get first element
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(content) {
                if let Some(first_obj) = arr.first() {
                    items.push(text_item("Inferred Schema (from first record):"));
                    items.push(BoxItem::Empty);
                    add_schema_fields(&mut items, first_obj, 0);
                }
            }
        }
        JsonFormat::Object => {
            if let Ok(obj) = serde_json::from_str::<Value>(content) {
                items.push(text_item("Object Structure:"));
                items.push(BoxItem::Empty);
                add_schema_fields(&mut items, &obj, 0);
            }
        }
    }

    Ok(items)
}

fn build_layout_section(
    content: &str,
    format: JsonFormat,
    verbose: bool,
) -> Result<Vec<BoxItem>> {
    let mut items = Vec::new();

    items.push(text_item("Format Information:"));
    items.push(BoxItem::Empty);

    match format {
        JsonFormat::NdJson => {
            items.push(kv_item("Format Type", "Newline-Delimited JSON", 25));
            items.push(kv_item("Records", "One per line", 25));

            let line_count = content.lines().count();
            items.push(kv_item("Total Lines", format_number(line_count as i64), 25));

            if verbose {
                items.push(BoxItem::Empty);
                items.push(text_item("Sample Line Sizes:"));
                items.push(BoxItem::Empty);

                for (idx, line) in content.lines().take(5).enumerate() {
                    items.push(text_item(format!(
                        "  Line {}: {} bytes",
                        idx + 1,
                        line.len()
                    )));
                }
            }
        }
        JsonFormat::Array => {
            items.push(kv_item("Format Type", "JSON Array", 25));

            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(content) {
                items.push(kv_item("Array Length", format_number(arr.len() as i64), 25));

                if verbose && !arr.is_empty() {
                    items.push(BoxItem::Empty);
                    items.push(text_item("Array Elements:"));
                    items.push(BoxItem::Empty);

                    for (idx, elem) in arr.iter().take(5).enumerate() {
                        let elem_type = match elem {
                            Value::Object(_) => "Object",
                            Value::Array(_) => "Array",
                            Value::String(_) => "String",
                            Value::Number(_) => "Number",
                            Value::Bool(_) => "Boolean",
                            Value::Null => "Null",
                        };
                        items.push(text_item(format!("  Element {}: {}", idx + 1, elem_type)));
                    }
                }
            }
        }
        JsonFormat::Object => {
            items.push(kv_item("Format Type", "Single Object", 25));

            if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(content) {
                items.push(kv_item("Top-Level Keys", obj.len(), 25));
            }
        }
    }

    items.push(BoxItem::Empty);

    // Nesting information
    items.push(text_item("Structure Analysis:"));
    items.push(BoxItem::Empty);

    let max_depth = calculate_max_depth(content, format)?;
    items.push(kv_item("Max Nesting Depth", max_depth, 25));

    Ok(items)
}

fn build_statistics_section(content: &str, format: JsonFormat) -> Result<Vec<BoxItem>> {
    let mut items = Vec::new();

    items.push(kv_item("File Size", format_size(content.len() as u64), 30));

    match format {
        JsonFormat::NdJson => {
            let line_count = content.lines().count();
            items.push(kv_item("Total Records", format_number(line_count as i64), 30));

            if line_count > 0 {
                let avg_size = content.len() / line_count;
                items.push(kv_item("Avg Record Size", format_size(avg_size as u64), 30));
            }
        }
        JsonFormat::Array => {
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(content) {
                items.push(kv_item("Total Records", format_number(arr.len() as i64), 30));

                if !arr.is_empty() {
                    let avg_size = content.len() / arr.len();
                    items.push(kv_item(
                        "Avg Record Size (approx)",
                        format_size(avg_size as u64),
                        30,
                    ));
                }
            }
        }
        JsonFormat::Object => {
            if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(content) {
                items.push(kv_item("Top-Level Keys", obj.len(), 30));
            }
        }
    }

    Ok(items)
}

fn detect_json_format(content: &str) -> Result<JsonFormat> {
    let trimmed = content.trim();

    // Try to parse as NDJSON (first line should be valid JSON object)
    if let Some(first_line) = content.lines().next() {
        if let Ok(Value::Object(_)) = serde_json::from_str::<Value>(first_line.trim()) {
            // Check if multiple lines exist
            if content.lines().count() > 1 {
                return Ok(JsonFormat::NdJson);
            }
        }
    }

    // Try to parse as JSON array
    if trimmed.starts_with('[') {
        if let Ok(Value::Array(_)) = serde_json::from_str::<Value>(content) {
            return Ok(JsonFormat::Array);
        }
    }

    // Try to parse as single JSON object
    if trimmed.starts_with('{') {
        if let Ok(Value::Object(_)) = serde_json::from_str::<Value>(content) {
            return Ok(JsonFormat::Object);
        }
    }

    Err(Error::General("Could not detect JSON format".to_string()))
}

fn add_schema_fields(items: &mut Vec<BoxItem>, value: &Value, indent: usize) {
    let prefix = "  ".repeat(indent);

    match value {
        Value::Object(obj) => {
            for (key, val) in obj.iter().take(20) {
                let type_str = get_value_type(val);
                items.push(text_item(format!("{}{}: {}", prefix, key, type_str)));

                // Recursively show nested objects (limited depth)
                if indent < 2 && matches!(val, Value::Object(_)) {
                    add_schema_fields(items, val, indent + 1);
                }
            }

            if obj.len() > 20 {
                items.push(text_item(format!("{}... and {} more fields", prefix, obj.len() - 20)));
            }
        }
        _ => {
            items.push(text_item(format!("{}{}", prefix, get_value_type(value))));
        }
    }
}

fn get_value_type(value: &Value) -> String {
    match value {
        Value::Object(obj) => format!("Object ({} keys)", obj.len()),
        Value::Array(arr) => format!("Array ({} elements)", arr.len()),
        Value::String(_) => "String".to_string(),
        Value::Number(_) => "Number".to_string(),
        Value::Bool(_) => "Boolean".to_string(),
        Value::Null => "Null".to_string(),
    }
}

fn calculate_max_depth(content: &str, format: JsonFormat) -> Result<usize> {
    match format {
        JsonFormat::NdJson => {
            if let Some(first_line) = content.lines().next() {
                if let Ok(obj) = serde_json::from_str::<Value>(first_line) {
                    return Ok(get_depth(&obj));
                }
            }
        }
        JsonFormat::Array => {
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(content) {
                if let Some(first) = arr.first() {
                    return Ok(get_depth(first));
                }
            }
        }
        JsonFormat::Object => {
            if let Ok(obj) = serde_json::from_str::<Value>(content) {
                return Ok(get_depth(&obj));
            }
        }
    }

    Ok(0)
}

fn get_depth(value: &Value) -> usize {
    match value {
        Value::Object(obj) => {
            if obj.is_empty() {
                1
            } else {
                1 + obj.values().map(get_depth).max().unwrap_or(0)
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                1
            } else {
                1 + arr.iter().map(get_depth).max().unwrap_or(0)
            }
        }
        _ => 0,
    }
}

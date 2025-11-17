//! Parquet physical layout inspection

use std::path::Path;
use std::sync::Arc;

use colored::Colorize;
use datafusion::parquet::file::metadata::FileMetaData;
use datafusion::parquet::file::reader::{FileReader, SerializedFileReader};

use crate::cli::output::BoxItem;
use crate::core::storage::{GetOptions, StorageBackend};
use crate::error::{Error, Result};

use super::common::*;

/// Wrap text to fit within a maximum width, breaking at word boundaries
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if text.len() <= max_width {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();

    // Split by whitespace OR commas to handle comma-separated lists
    let parts: Vec<&str> = if text.contains(',') && !text.contains(' ') {
        // If it's a comma-separated list without spaces, split by comma
        text.split(',').collect()
    } else {
        // Otherwise, split by whitespace
        text.split_whitespace().collect()
    };

    for (i, word) in parts.iter().enumerate() {
        let separator = if text.contains(',') && !text.contains(' ') {
            // For comma-separated lists, keep the comma
            if i < parts.len() - 1 { "," } else { "" }
        } else {
            // For space-separated text, use space
            if i == 0 { "" } else { " " }
        };

        let word_with_sep = if separator.is_empty() {
            word.to_string()
        } else {
            format!("{}{}", word, separator)
        };

        // If adding this word would exceed the width
        if !current_line.is_empty() && current_line.len() + word_with_sep.len() > max_width {
            lines.push(current_line);
            current_line = word_with_sep;
        } else {
            current_line.push_str(&word_with_sep);
        }
    }

    // Add the last line if not empty
    if !current_line.is_empty() {
        lines.push(current_line);
    }

    // If no lines were created (e.g., single very long word), just split it
    if lines.is_empty() {
        let mut pos = 0;
        while pos < text.len() {
            let end = (pos + max_width).min(text.len());
            lines.push(text[pos..end].to_string());
            pos = end;
        }
    }

    lines
}

/// Inspect Parquet file physical layout
pub async fn inspect_parquet_layout(
    path: &Path,
    storage: Arc<dyn StorageBackend>,
    options: &PhysicalInspectOptions,
) -> Result<PhysicalInspectResult> {
    // Read the file from storage
    let path_str = path.to_str().ok_or_else(|| {
        Error::General(format!("Invalid path: {}", path.display()))
    })?;
    let data = storage.get(path_str, &GetOptions::default()).await?;
    let file_size = data.len() as u64;

    // Create a Parquet reader - Bytes implements ChunkReader
    let reader = SerializedFileReader::new(data.clone())
        .map_err(|e| Error::General(format!("Failed to read Parquet file: {}", e)))?;

    let metadata = reader.metadata().file_metadata();
    let row_groups = reader.metadata().row_groups();

    // Build file information
    let file_info = build_file_info(path, metadata, row_groups.len(), file_size);

    // Build schema section if requested
    let schema = if options.show_schema {
        Some(build_schema_section(metadata))
    } else {
        None
    };

    // Build physical layout section if requested
    let layout = if options.show_layout {
        Some(build_layout_section(metadata, row_groups, file_size, &data, options))
    } else {
        None
    };

    // Build statistics section if requested
    let statistics = if options.show_stats {
        Some(build_statistics_section(metadata, row_groups, options))
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

fn build_file_info(
    path: &Path,
    metadata: &FileMetaData,
    num_row_groups: usize,
    file_size: u64,
) -> Vec<BoxItem> {
    let mut items = Vec::new();

    items.push(kv_item("Path", get_file_name(path), 20));
    items.push(kv_item("Format", "Apache Parquet", 20));
    items.push(kv_item("Version", metadata.version(), 20));

    if let Some(created_by) = metadata.created_by() {
        // Truncate if too long
        let created_by = if created_by.len() > 50 {
            format!("{}...", &created_by[..47])
        } else {
            created_by.to_string()
        };
        items.push(kv_item("Created by", created_by, 20));
    }

    items.push(kv_item("File Size", format_size(file_size), 20));
    items.push(kv_item("Rows", format_number(metadata.num_rows()), 20));
    items.push(kv_item("Row Groups", num_row_groups, 20));
    items.push(kv_item("Columns", metadata.schema_descr().num_columns(), 20));

    items
}

fn build_schema_section(metadata: &FileMetaData) -> Vec<BoxItem> {
    let mut items = Vec::new();

    let schema = metadata.schema_descr();
    items.push(text_item(format!("Columns: {}", schema.num_columns())));
    items.push(BoxItem::Empty);

    // List columns with types
    for (idx, col) in schema.columns().iter().enumerate() {
        let col_type = format!("{:?}", col.physical_type());
        let mut logical_type = col
            .logical_type()
            .map(|lt| format!(" ({})", format!("{:?}", lt)))
            .unwrap_or_default();

        // Truncate logical_type if too long to fit in box (max ~40 chars for logical type)
        if logical_type.len() > 40 {
            logical_type.truncate(37);
            logical_type.push_str("...)");
        }

        items.push(text_item(format!(
            "  {:<3} {:<30} {}{}",
            format!("{}.", idx + 1),
            col.name(),
            col_type,
            logical_type
        )));
    }

    items
}

fn build_layout_section(
    metadata: &FileMetaData,
    row_groups: &[datafusion::parquet::file::metadata::RowGroupMetaData],
    file_size: u64,
    data: &bytes::Bytes,
    options: &PhysicalInspectOptions,
) -> Vec<BoxItem> {
    use super::common::VerbosityLevel;
    let mut items = Vec::new();

    // File Structure breakdown
    items.push(text_item(format!("═══ {} ═══", "File Structure".bold())));
    items.push(BoxItem::Empty);

    // Parquet format: [4-byte magic "PAR1"][Row Groups][FileMetadata][4-byte length][4-byte magic]
    let header_size = 4u64; // Magic number at start

    // Footer: read footer length from end of file
    let footer_len_bytes = &data[data.len() - 8..data.len() - 4];
    let footer_metadata_len = u32::from_le_bytes([
        footer_len_bytes[0],
        footer_len_bytes[1],
        footer_len_bytes[2],
        footer_len_bytes[3],
    ]) as u64;

    // Footer = metadata + 4-byte length + 4-byte magic
    let footer_size = footer_metadata_len + 8;

    // Body = everything between header and footer (all row groups)
    let body_size = file_size - header_size - footer_size;

    items.push(kv_item("Header", format_size(header_size), 25));
    items.push(kv_item("Body (Row Groups)", format_size(body_size), 25));
    items.push(kv_item("Footer", format_size(footer_size), 25));
    items.push(BoxItem::Empty);

    // Footer Contents
    items.push(text_item(format!("───  {} ───", "Footer Contents".bold())));
    items.push(BoxItem::Empty);
    items.push(kv_item("  Version", metadata.version(), 25));
    items.push(kv_item(
        "  Schema",
        format!("{} fields", metadata.schema_descr().num_columns()),
        25,
    ));
    items.push(kv_item("  Row Groups", row_groups.len(), 25));

    // Show key-value metadata if present
    if let Some(kv_metadata) = metadata.key_value_metadata() {
        items.push(kv_item("  Key-Value Metadata", format!("{} entries", kv_metadata.len()), 25));

        if options.verbosity >= VerbosityLevel::Verbose && !kv_metadata.is_empty() {
            items.push(BoxItem::Empty);
            items.push(text_item("  Metadata Entries:"));

            // Show first 5 entries
            for kv in kv_metadata.iter().take(5) {
                let value = kv.value.as_ref().map(|v| {
                    // Special handling for common metadata keys
                    match kv.key.as_str() {
                        "ARROW:schema" => {
                            format!("<Arrow Schema> ({} bytes, base64 encoded)", v.len())
                        },
                        "pandas" => {
                            // Try to show readable pandas metadata
                            if v.starts_with("{") || v.starts_with("[") {
                                if v.len() > 100 {
                                    format!("<JSON metadata> ({} bytes)", v.len())
                                } else {
                                    v.clone()
                                }
                            } else {
                                format!("<Pandas metadata> ({} bytes)", v.len())
                            }
                        },
                        _ => {
                            // For other keys, check if it looks like base64
                            if v.len() > 50 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=') {
                                format!("<base64 data> ({} bytes)", v.len())
                            } else {
                                v.clone()
                            }
                        }
                    }
                }).unwrap_or_else(|| "null".to_string());

                // Word wrap long values
                let prefix = format!("    {}: ", kv.key);
                let max_width = 94; // Box width (100) - margins (6)
                let wrapped_lines = wrap_text(&value, max_width - prefix.len());

                // First line with the key
                items.push(text_item(format!("{}{}", prefix, wrapped_lines[0])));

                // Continuation lines with indentation matching the value start
                for line in &wrapped_lines[1..] {
                    let indent = " ".repeat(prefix.len());
                    items.push(text_item(format!("{}{}", indent, line)));
                }
            }

            if kv_metadata.len() > 5 {
                items.push(text_item(format!("    [{} more entries]", kv_metadata.len() - 5)));
            }
        }
    }

    items.push(BoxItem::Empty);

    // Row group distribution
    items.push(text_item(format!("═══ {} ═══", "Row Groups".bold())));
    items.push(BoxItem::Empty);

    if row_groups.is_empty() {
        items.push(text_item("  No row groups found"));
        return items;
    }

    // Calculate row distribution
    let total_rows = metadata.num_rows();
    let avg_rows_per_group = total_rows / row_groups.len() as i64;

    items.push(kv_item("Total Rows", format_number(total_rows), 25));
    items.push(kv_item("Avg Rows/Group", format_number(avg_rows_per_group), 25));

    // Row group size distribution
    let total_bytes: i64 = row_groups.iter().map(|rg| rg.total_byte_size()).sum();
    let avg_bytes_per_group = total_bytes / row_groups.len() as i64;

    items.push(kv_item(
        "Total Compressed Size",
        format_size(total_bytes as u64),
        25,
    ));
    items.push(kv_item(
        "Avg Size/Group",
        format_size(avg_bytes_per_group as u64),
        25,
    ));

    items.push(BoxItem::Empty);

    // Column chunk information
    items.push(text_item(format!("═══ {} ═══", "Column Encoding & Compression".bold())));
    items.push(BoxItem::Empty);

    if let Some(first_rg) = row_groups.first() {
        let num_cols = first_rg.columns().len();
        for (idx, col_chunk) in first_rg.columns().iter().enumerate() {
            let col_meta = col_chunk.column_descr();
            let compression = format!("{:?}", col_chunk.compression());
            let encodings = col_chunk
                .encodings()
                .iter()
                .map(|e| format!("{:?}", e))
                .collect::<Vec<_>>()
                .join(", ");

            items.push(text_item(format!("• {}", col_meta.name().bold())));
            items.push(kv_item("  Compression", compression, 20));
            items.push(kv_item("  Encodings", encodings, 20));

            if options.verbosity >= VerbosityLevel::Verbose {
                items.push(kv_item(
                    "  Compressed Size",
                    format_size(col_chunk.compressed_size() as u64),
                    20,
                ));
                items.push(kv_item(
                    "  Uncompressed Size",
                    format_size(col_chunk.uncompressed_size() as u64),
                    20,
                ));
            }

            if idx < num_cols - 1 {
                // Add spacing between columns
                items.push(BoxItem::Empty);
            }
        }
    }

    // Verbose: show each row group
    if options.verbosity >= VerbosityLevel::Verbose && row_groups.len() <= 10 {
        items.push(BoxItem::Empty);
        items.push(text_item(format!("─── {} ───", "Row Group Details".bold())));
        items.push(BoxItem::Empty);

        for (idx, rg) in row_groups.iter().enumerate() {
            items.push(text_item(format!("• {}", format!("Row Group {}", idx).bold())));
            items.push(kv_item(
                "  Offset",
                format_number(rg.file_offset().unwrap_or(0)),
                20,
            ));
            items.push(kv_item(
                "  Length",
                format_size(rg.total_byte_size() as u64),
                20,
            ));
            items.push(kv_item(
                "  Rows",
                format_number(rg.num_rows()),
                20,
            ));
            items.push(kv_item(
                "  Columns",
                rg.num_columns(),
                20,
            ));

            // Add separator between row groups
            if idx < row_groups.len() - 1 {
                items.push(BoxItem::Empty);
                items.push(text_item("· · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · · ·"));
            }
            items.push(BoxItem::Empty);
        }
    }

    items
}

fn build_statistics_section(
    metadata: &FileMetaData,
    row_groups: &[datafusion::parquet::file::metadata::RowGroupMetaData],
    options: &PhysicalInspectOptions,
) -> Vec<BoxItem> {
    let mut items = Vec::new();

    // Overall statistics
    items.push(text_item(format!("─── {} ───", "Overall Compression".bold())));
    items.push(BoxItem::Empty);

    let total_compressed: i64 = row_groups.iter().map(|rg| rg.total_byte_size()).sum();

    // Estimate uncompressed size from column chunks
    let total_uncompressed: i64 = row_groups
        .iter()
        .flat_map(|rg| rg.columns())
        .map(|col| col.uncompressed_size())
        .sum();

    items.push(kv_item(
        "Compressed Size",
        format_size(total_compressed as u64),
        30,
    ));
    items.push(kv_item(
        "Uncompressed Size",
        format_size(total_uncompressed as u64),
        30,
    ));

    if total_uncompressed > 0 {
        let ratio = format_compression_ratio(
            total_compressed as u64,
            total_uncompressed as u64,
        );
        items.push(kv_item("Compression Ratio", ratio, 30));
    }

    items.push(BoxItem::Empty);

    // Per-column statistics
    items.push(text_item(format!("═══ {} ═══", "Per-Column Statistics".bold())));
    items.push(BoxItem::Empty);

    let schema = metadata.schema_descr();
    let num_cols = schema.columns().len();
    for (col_idx, col_desc) in schema.columns().iter().enumerate() {
        let col_name = col_desc.name();

        // Aggregate statistics across all row groups
        let mut total_col_compressed: i64 = 0;
        let mut total_col_uncompressed: i64 = 0;
        let mut total_null_count: i64 = 0;
        let mut has_stats = false;
        let mut min_value: Option<String> = None;
        let mut max_value: Option<String> = None;
        let mut distinct_count: Option<i64> = None;

        for rg in row_groups {
            if let Some(col_chunk) = rg.columns().get(col_idx) {
                total_col_compressed += col_chunk.compressed_size();
                total_col_uncompressed += col_chunk.uncompressed_size();

                if let Some(stats) = col_chunk.statistics() {
                    if let Some(null_count) = stats.null_count_opt() {
                        total_null_count += null_count as i64;
                        has_stats = true;
                    }

                    // Extract min/max values (only from first row group for simplicity)
                    if min_value.is_none() && stats.has_min_max_set() {
                        let physical_type = col_desc.physical_type();
                        min_value = Some(format_stat_value(stats.min_bytes(), physical_type));
                        max_value = Some(format_stat_value(stats.max_bytes(), physical_type));
                    }

                    // Get distinct count if available
                    if let Some(dc) = stats.distinct_count_opt() {
                        distinct_count = Some(dc as i64);
                    }
                }
            }
        }

        items.push(text_item(format!("• {}", col_name.bold())));
        items.push(kv_item(
            "  Compressed",
            format_size(total_col_compressed as u64),
            25,
        ));
        items.push(kv_item(
            "  Uncompressed",
            format_size(total_col_uncompressed as u64),
            25,
        ));

        if total_col_uncompressed > 0 {
            let ratio = format_compression_ratio(
                total_col_compressed as u64,
                total_col_uncompressed as u64,
            );
            items.push(kv_item("  Ratio", ratio, 25));
        }

        if has_stats {
            let null_pct = (total_null_count as f64 / metadata.num_rows() as f64) * 100.0;
            items.push(kv_item(
                "  Nulls",
                format!("{} ({:.2}%)", format_number(total_null_count), null_pct),
                25,
            ));
        }

        // Show Min/Max and Distinct count in verbose mode
        use super::common::VerbosityLevel;
        if options.verbosity >= VerbosityLevel::Verbose {
            if let Some(min) = &min_value {
                let min_display = if min.len() > 50 {
                    format!("{}...", &min[..47])
                } else {
                    min.clone()
                };
                items.push(kv_item("  Min", min_display, 25));
            }

            if let Some(max) = &max_value {
                let max_display = if max.len() > 50 {
                    format!("{}...", &max[..47])
                } else {
                    max.clone()
                };
                items.push(kv_item("  Max", max_display, 25));
            }

            if let Some(dc) = distinct_count {
                items.push(kv_item("  Distinct", format_number(dc), 25));
            }
        }

        if col_idx < num_cols - 1 {
            items.push(BoxItem::Empty);
        }
    }

    items
}

/// Format a statistic value based on its physical type
fn format_stat_value(bytes: &[u8], physical_type: datafusion::parquet::basic::Type) -> String {
    use datafusion::parquet::basic::Type;

    match physical_type {
        Type::INT32 => {
            if bytes.len() >= 4 {
                let value = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                value.to_string()
            } else {
                format!("<invalid i32: {} bytes>", bytes.len())
            }
        }
        Type::INT64 => {
            if bytes.len() >= 8 {
                let value = i64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                    bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                value.to_string()
            } else {
                format!("<invalid i64: {} bytes>", bytes.len())
            }
        }
        Type::FLOAT => {
            if bytes.len() >= 4 {
                let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                format!("{:.2}", value)
            } else {
                format!("<invalid f32: {} bytes>", bytes.len())
            }
        }
        Type::DOUBLE => {
            if bytes.len() >= 8 {
                let value = f64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                    bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                format!("{:.2}", value)
            } else {
                format!("<invalid f64: {} bytes>", bytes.len())
            }
        }
        Type::BYTE_ARRAY => {
            // Try to decode as UTF-8 string
            match String::from_utf8(bytes.to_vec()) {
                Ok(s) => {
                    if s.len() > 30 {
                        format!("\"{}...\"", &s[..27])
                    } else {
                        format!("\"{}\"", s)
                    }
                }
                Err(_) => {
                    // Not valid UTF-8, show as hex
                    if bytes.len() > 16 {
                        format!("<binary: {} bytes>", bytes.len())
                    } else {
                        format!("<hex: {}>", bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>())
                    }
                }
            }
        }
        Type::BOOLEAN => {
            if !bytes.is_empty() {
                if bytes[0] != 0 { "true" } else { "false" }.to_string()
            } else {
                "<invalid bool>".to_string()
            }
        }
        Type::FIXED_LEN_BYTE_ARRAY => {
            if bytes.len() > 16 {
                format!("<fixed binary: {} bytes>", bytes.len())
            } else {
                format!("<hex: {}>", bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>())
            }
        }
        Type::INT96 => {
            format!("<int96: {} bytes>", bytes.len())
        }
    }
}

//! Parquet physical layout inspection

use std::path::Path;
use std::sync::Arc;

use datafusion::parquet::file::metadata::FileMetaData;
use datafusion::parquet::file::reader::{FileReader, SerializedFileReader};

use crate::cli::output::BoxItem;
use crate::core::storage::{GetOptions, StorageBackend};
use crate::error::{Error, Result};

use super::common::*;

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
        Some(build_layout_section(metadata, row_groups, file_size, &data, options.verbose))
    } else {
        None
    };

    // Build statistics section if requested
    let statistics = if options.show_stats {
        Some(build_statistics_section(metadata, row_groups))
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
        let logical_type = col
            .logical_type()
            .map(|lt| format!(" ({})", format!("{:?}", lt)))
            .unwrap_or_default();

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
    verbose: bool,
) -> Vec<BoxItem> {
    let mut items = Vec::new();

    // File Structure breakdown
    items.push(text_item("File Structure:"));
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
    items.push(text_item("Footer Contents:"));
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

        if verbose && !kv_metadata.is_empty() {
            items.push(BoxItem::Empty);
            items.push(text_item("  Metadata Entries:"));

            // Show first 5 entries
            for kv in kv_metadata.iter().take(5) {
                let value = kv.value.as_ref().map(|v| {
                    if v.len() > 50 {
                        format!("{}...", &v[..47])
                    } else {
                        v.clone()
                    }
                }).unwrap_or_else(|| "null".to_string());

                items.push(text_item(format!("    {}: {}", kv.key, value)));
            }

            if kv_metadata.len() > 5 {
                items.push(text_item(format!("    [{} more entries]", kv_metadata.len() - 5)));
            }
        }
    }

    items.push(BoxItem::Empty);

    // Row group distribution
    items.push(text_item(format!("Row Groups: {}", row_groups.len())));
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
    items.push(text_item("Column Encoding & Compression:"));
    items.push(BoxItem::Empty);

    if let Some(first_rg) = row_groups.first() {
        for col_chunk in first_rg.columns() {
            let col_meta = col_chunk.column_descr();
            let compression = format!("{:?}", col_chunk.compression());
            let encodings = col_chunk
                .encodings()
                .iter()
                .map(|e| format!("{:?}", e))
                .collect::<Vec<_>>()
                .join(", ");

            items.push(text_item(format!("  {}", col_meta.name())));
            items.push(kv_item("    Compression", compression, 20));
            items.push(kv_item("    Encodings", encodings, 20));

            if verbose {
                items.push(kv_item(
                    "    Compressed Size",
                    format_size(col_chunk.compressed_size() as u64),
                    20,
                ));
                items.push(kv_item(
                    "    Uncompressed Size",
                    format_size(col_chunk.uncompressed_size() as u64),
                    20,
                ));
            }
        }
    }

    // Verbose: show each row group
    if verbose && row_groups.len() <= 10 {
        items.push(BoxItem::Empty);
        items.push(text_item("Row Group Details:"));
        items.push(BoxItem::Empty);

        for (idx, rg) in row_groups.iter().enumerate() {
            items.push(text_item(format!("  Row Group {}:", idx)));
            items.push(kv_item(
                "    Rows",
                format_number(rg.num_rows()),
                20,
            ));
            items.push(kv_item(
                "    Size",
                format_size(rg.total_byte_size() as u64),
                20,
            ));
            items.push(kv_item(
                "    Columns",
                rg.num_columns(),
                20,
            ));
        }
    }

    items
}

fn build_statistics_section(
    metadata: &FileMetaData,
    row_groups: &[datafusion::parquet::file::metadata::RowGroupMetaData],
) -> Vec<BoxItem> {
    let mut items = Vec::new();

    // Overall statistics
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
    items.push(text_item("Per-Column Statistics:"));
    items.push(BoxItem::Empty);

    let schema = metadata.schema_descr();
    for (col_idx, col_desc) in schema.columns().iter().enumerate() {
        let col_name = col_desc.name();

        // Aggregate statistics across all row groups
        let mut total_col_compressed: i64 = 0;
        let mut total_col_uncompressed: i64 = 0;
        let mut total_null_count: i64 = 0;
        let mut has_stats = false;

        for rg in row_groups {
            if let Some(col_chunk) = rg.columns().get(col_idx) {
                total_col_compressed += col_chunk.compressed_size();
                total_col_uncompressed += col_chunk.uncompressed_size();

                if let Some(stats) = col_chunk.statistics() {
                    if let Some(null_count) = stats.null_count_opt() {
                        total_null_count += null_count as i64;
                        has_stats = true;
                    }
                }
            }
        }

        items.push(text_item(format!("  {}", col_name)));
        items.push(kv_item(
            "    Compressed",
            format_size(total_col_compressed as u64),
            25,
        ));
        items.push(kv_item(
            "    Uncompressed",
            format_size(total_col_uncompressed as u64),
            25,
        ));

        if total_col_uncompressed > 0 {
            let ratio = format_compression_ratio(
                total_col_compressed as u64,
                total_col_uncompressed as u64,
            );
            items.push(kv_item("    Ratio", ratio, 25));
        }

        if has_stats {
            let null_pct = (total_null_count as f64 / metadata.num_rows() as f64) * 100.0;
            items.push(kv_item(
                "    Nulls",
                format!("{} ({:.2}%)", format_number(total_null_count), null_pct),
                25,
            ));
        }
    }

    items
}

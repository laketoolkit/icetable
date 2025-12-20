//! Inspection result formatting utilities

use arrow::array::Array;
use arrow::datatypes::{DataType, Schema};
use arrow::record_batch::RecordBatch;
use colored::Colorize;
use comfy_table::{Cell, CellAlignment, Color};

use super::format_timestamp_ms;
use super::formatter::create_styled_table;
use super::{Box, BoxItem, BoxLayout, BoxRenderer, BoxSection};
use crate::core::formats::{ColumnStats, FileMetadata};
use crate::core::operations::inspect::{
    IcebergInspectOptions, IcebergInspectResult, InspectResult,
};
use crate::core::{format_bytes, format_number};

/// Formatter for inspection results
pub struct InspectionFormatter;

impl InspectionFormatter {
    /// Format an inspection result
    pub fn format_inspect_result(
        result: &InspectResult,
        options: &crate::core::operations::inspect::InspectOptions,
    ) -> String {
        let mut output = Vec::new();

        // File information box
        let mut file_content = vec![format!("Format: {}", result.format_name)];

        // Add basic info from metadata if available
        if let Some(metadata) = &result.metadata {
            if let Some(rows) = metadata.num_rows {
                file_content.push(format!("Rows:   {}", format_number(rows)));
            }
            if let Some(compressed) = metadata.compressed_size {
                if let Some(uncompressed) = metadata.uncompressed_size {
                    file_content.push(format!(
                        "Size:   {} (compressed), {} (uncompressed)",
                        format_bytes(compressed),
                        format_bytes(uncompressed)
                    ));
                } else {
                    file_content.push(format!("Size:   {}", format_bytes(compressed)));
                }
            }
        }

        let file_box = crate::utils::create_box_frame(
            Some(&"FILE INFORMATION".bold().to_string()),
            file_content,
            Some(72),
        );
        output.push(file_box);
        output.push(String::new());

        // Metadata (if requested and available)
        if options.show_metadata
            && let Some(metadata) = &result.metadata
        {
            let metadata_content = Self::format_metadata_content(metadata);
            if !metadata_content.is_empty() {
                let metadata_box = crate::utils::create_box_frame(
                    Some(&"METADATA".bold().to_string()),
                    metadata_content,
                    Some(72),
                );
                output.push(metadata_box);
                output.push(String::new());
            }
        }

        // Schema (if requested)
        if options.show_schema {
            let schema_content = Self::format_schema_content(&result.schema);
            let schema_box = crate::utils::create_box_frame(
                Some(&"SCHEMA".bold().to_string()),
                schema_content,
                Some(72),
            );
            output.push(schema_box);
            output.push(String::new());
        }

        // Statistics (if requested and available)
        if options.show_stats
            && let Some(stats) = &result.statistics
        {
            output.push(format!("{}", "STATISTICS".bold()));
            output.push(Self::format_statistics(stats));
            output.push(String::new());
        }

        // Sample Data (if requested and available)
        if options.show_data
            && let Some(batch) = &result.sample_data
        {
            output.push("DATA PREVIEW".bold().to_string());
            output.push(Self::format_record_batch(batch));
        }

        output.join("\n")
    }

    /// Format schema content as vector of lines
    fn format_schema_content(schema: &Schema) -> Vec<String> {
        let mut output = Vec::new();

        for (i, field) in schema.fields().iter().enumerate() {
            let is_last = i == schema.fields().len() - 1;
            let prefix = if is_last { "└" } else { "├" };

            let nullable_marker = if field.is_nullable() {
                " (nullable)"
            } else {
                ""
            };
            let type_str = Self::format_data_type(field.data_type());

            output.push(format!(
                "{} {}: {}{}",
                prefix.bright_black(),
                field.name().white().bold(),
                type_str.cyan(),
                nullable_marker.dimmed()
            ));
        }

        output
    }

    /// Format DataType to readable string
    fn format_data_type(dtype: &DataType) -> String {
        match dtype {
            DataType::Int8 => "int8".to_string(),
            DataType::Int16 => "int16".to_string(),
            DataType::Int32 => "int32".to_string(),
            DataType::Int64 => "int64".to_string(),
            DataType::UInt8 => "uint8".to_string(),
            DataType::UInt16 => "uint16".to_string(),
            DataType::UInt32 => "uint32".to_string(),
            DataType::UInt64 => "uint64".to_string(),
            DataType::Float16 => "float16".to_string(),
            DataType::Float32 => "float32".to_string(),
            DataType::Float64 => "float64".to_string(),
            DataType::Utf8 => "string".to_string(),
            DataType::LargeUtf8 => "large_string".to_string(),
            DataType::Binary => "binary".to_string(),
            DataType::LargeBinary => "large_binary".to_string(),
            DataType::Boolean => "bool".to_string(),
            DataType::Date32 => "date32".to_string(),
            DataType::Date64 => "date64".to_string(),
            DataType::Timestamp(unit, tz) => {
                let tz_str = tz.as_ref().map(|t| format!(" ({})", t)).unwrap_or_default();
                format!("timestamp({:?}){}", unit, tz_str)
            }
            DataType::List(field) => format!("list<{}>", Self::format_data_type(field.data_type())),
            DataType::LargeList(field) => {
                format!("large_list<{}>", Self::format_data_type(field.data_type()))
            }
            DataType::Struct(fields) => {
                let field_strs: Vec<_> = fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name(), Self::format_data_type(f.data_type())))
                    .collect();
                format!("struct<{}>", field_strs.join(", "))
            }
            DataType::Decimal128(p, s) => format!("decimal({}, {})", p, s),
            _ => format!("{:?}", dtype),
        }
    }

    /// Format metadata content as vector of lines (without row/size info shown in FILE INFORMATION)
    fn format_metadata_content(metadata: &FileMetadata) -> Vec<String> {
        let mut output = Vec::new();

        // Compression info with ratio
        if let Some(compression) = &metadata.compression {
            if let (Some(compressed), Some(uncompressed)) =
                (metadata.compressed_size, metadata.uncompressed_size)
            {
                let ratio = (compressed as f64 / uncompressed as f64) * 100.0;
                output.push(format!("Compression:    {} ({:.1}%)", compression, ratio));
            } else {
                output.push(format!("Compression:    {}", compression));
            }
        }

        if let Some(version) = &metadata.format_version {
            output.push(format!("Format Version: {}", version));
        }

        // Custom metadata
        for (key, value) in &metadata.metadata {
            output.push(format!("{}: {}", key, value));
        }

        output
    }

    /// Format column statistics as a table
    pub fn format_statistics(stats: &[ColumnStats]) -> String {
        let mut table = create_styled_table();

        table.set_header(vec![
            "Column".to_string(),
            "Null Count".to_string(),
            "Distinct".to_string(),
            "Min".to_string(),
            "Max".to_string(),
            "Mean".to_string(),
            "Std Dev".to_string(),
        ]);

        for stat in stats {
            let null_count = match stat.null_count {
                Some(count) => count.to_string(),
                None => "N/A".to_string(),
            };

            let distinct_count = match stat.distinct_count {
                Some(count) => count.to_string(),
                None => "N/A".to_string(),
            };

            let min_value = stat.min_value.clone().unwrap_or_else(|| "N/A".to_string());
            let max_value = stat.max_value.clone().unwrap_or_else(|| "N/A".to_string());
            let mean_value = match stat.mean {
                Some(val) => format!("{:.2}", val),
                None => "N/A".to_string(),
            };
            let std_dev_value = match stat.std_dev {
                Some(val) => format!("{:.2}", val),
                None => "N/A".to_string(),
            };

            table.add_row(vec![
                stat.name.clone(),
                null_count,
                distinct_count,
                min_value,
                max_value,
                mean_value,
                std_dev_value,
            ]);
        }

        table.to_string()
    }

    /// Format an Arrow RecordBatch as a table
    pub fn format_record_batch(batch: &RecordBatch) -> String {
        let mut table = create_styled_table();

        // Create header
        let schema = batch.schema();
        let mut headers = Vec::new();
        for field in schema.fields() {
            headers.push(field.name().to_string());
        }
        table.set_header(headers);

        // Add rows (limit to first 10 rows for preview)
        let max_rows = 10;
        let num_rows = batch.num_rows().min(max_rows);

        for row_idx in 0..num_rows {
            let mut row_cells = Vec::new();
            for col_idx in 0..batch.num_columns() {
                let column = batch.column(col_idx);
                let cell = match column.data_type() {
                    DataType::Utf8 | DataType::LargeUtf8 => {
                        if let Some(array) =
                            column.as_any().downcast_ref::<arrow::array::StringArray>()
                        {
                            if array.is_null(row_idx) {
                                Cell::new("NULL").fg(Color::Red)
                            } else {
                                Cell::new(array.value(row_idx))
                            }
                        } else {
                            Cell::new("<string>")
                        }
                    }
                    DataType::Int8 | DataType::Int16 | DataType::Int32 | DataType::Int64 => {
                        // Use array_value_to_string for generic int handling
                        if column.is_null(row_idx) {
                            Cell::new("NULL").fg(Color::Red)
                        } else {
                            Cell::new(
                                arrow::util::display::array_value_to_string(column, row_idx)
                                    .unwrap_or_else(|_| "<int>".to_string()),
                            )
                        }
                    }
                    DataType::UInt8 | DataType::UInt16 | DataType::UInt32 | DataType::UInt64 => {
                        if column.is_null(row_idx) {
                            Cell::new("NULL").fg(Color::Red)
                        } else {
                            Cell::new(
                                arrow::util::display::array_value_to_string(column, row_idx)
                                    .unwrap_or_else(|_| "<uint>".to_string()),
                            )
                        }
                    }
                    DataType::Float32 | DataType::Float64 => {
                        if column.is_null(row_idx) {
                            Cell::new("NULL").fg(Color::Red)
                        } else {
                            Cell::new(
                                arrow::util::display::array_value_to_string(column, row_idx)
                                    .unwrap_or_else(|_| "<float>".to_string()),
                            )
                        }
                    }
                    DataType::Boolean => {
                        if let Some(array) =
                            column.as_any().downcast_ref::<arrow::array::BooleanArray>()
                        {
                            if array.is_null(row_idx) {
                                Cell::new("NULL").fg(Color::Red)
                            } else {
                                Cell::new(if array.value(row_idx) {
                                    "true"
                                } else {
                                    "false"
                                })
                            }
                        } else {
                            Cell::new("<bool>")
                        }
                    }
                    DataType::Timestamp(_, _) => {
                        if column.is_null(row_idx) {
                            Cell::new("NULL").fg(Color::Red)
                        } else {
                            Cell::new(
                                arrow::util::display::array_value_to_string(column, row_idx)
                                    .unwrap_or_else(|_| "<timestamp>".to_string()),
                            )
                        }
                    }
                    _ => Cell::new(format!("{:?}", column.data_type())),
                };
                row_cells.push(cell);
            }
            table.add_row(row_cells);
        }

        // Add truncation notice if needed
        if batch.num_rows() > max_rows {
            table.add_row(vec![
                Cell::new(format!(
                    "... (showing {} of {} rows)",
                    num_rows,
                    batch.num_rows()
                ))
                .set_alignment(CellAlignment::Center),
            ]);
        }

        table.to_string()
    }

    /// Format Iceberg table inspection result for display
    pub fn format_iceberg_table(
        result: &IcebergInspectResult,
        options: &IcebergInspectOptions,
    ) -> String {
        let layout = BoxLayout::new(100);
        let renderer = BoxRenderer::new(layout);
        let mut container = Box::titled("Iceberg Table Inspection");

        // ═══════════════════════════════════════════════════════════════════════
        // TABLE INFORMATION
        // ═══════════════════════════════════════════════════════════════════════
        let key_width = 18;
        let mut table_info = vec![
            BoxItem::kv_aligned(
                "Format",
                format!("Iceberg v{}", result.format_version),
                key_width,
            ),
            BoxItem::kv_aligned("Location", &result.location, key_width),
            BoxItem::kv_aligned("Table UUID", &result.table_uuid, key_width),
            BoxItem::kv_aligned(
                "Current Snapshot",
                result
                    .current_snapshot_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "None".to_string()),
                key_width,
            ),
            BoxItem::kv_aligned(
                "Snapshot Count",
                result.snapshot_count.to_string(),
                key_width,
            ),
            BoxItem::kv_aligned(
                "Last Updated",
                format_timestamp_ms(result.last_updated_ms),
                key_width,
            ),
        ];

        // Add sequence number in verbose mode
        if options.verbose {
            table_info.push(BoxItem::kv_aligned(
                "Last Sequence",
                result.last_sequence_number.to_string(),
                key_width,
            ));
        }

        container = container.section(BoxSection::titled("Table Information").items(table_info));

        // ═══════════════════════════════════════════════════════════════════════
        // CURRENT STATE (records, files, delete files, size)
        // ═══════════════════════════════════════════════════════════════════════
        let state = &result.current_state;
        let mut state_items = Vec::new();

        // Total Records
        state_items.push(BoxItem::kv_aligned(
            "Total Records",
            state
                .total_records
                .map(format_number)
                .unwrap_or_else(|| "-".to_string()),
            16,
        ));

        // Data Files
        state_items.push(BoxItem::kv_aligned(
            "Data Files",
            state
                .total_data_files
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            16,
        ));

        // Delete Files - always show, highlight if > 0
        let delete_files_str = match state.total_delete_files {
            Some(v) if v > 0 => format!("{} {}", v, "(compaction recommended)".yellow()),
            Some(v) => v.to_string(),
            None => "0".to_string(),
        };
        state_items.push(BoxItem::kv_aligned("Delete Files", delete_files_str, 16));

        // Total Size
        state_items.push(BoxItem::kv_aligned(
            "Total Size",
            state
                .total_files_size
                .map(|v| format_bytes(v as u64))
                .unwrap_or_else(|| "-".to_string()),
            16,
        ));

        container = container.section(BoxSection::titled("Current State").items(state_items));

        // ═══════════════════════════════════════════════════════════════════════
        // SCHEMA
        // ═══════════════════════════════════════════════════════════════════════
        let mut schema_items = vec![
            BoxItem::kv_aligned("Schema ID", result.schema_id.to_string(), 12),
            BoxItem::kv_aligned("Columns", result.fields.len().to_string(), 12),
        ];

        // Show identifier fields if any
        if !result.identifier_field_ids.is_empty() {
            let id_names: Vec<&str> = result
                .fields
                .iter()
                .filter(|f| f.is_identifier)
                .map(|f| f.name.as_str())
                .collect();
            schema_items.push(BoxItem::kv_aligned("Identifier", id_names.join(", "), 12));
        }

        // Add subsection header for fields
        schema_items.push(BoxItem::Empty);
        schema_items.push(BoxItem::text(format!(
            "{} Fields {}",
            "──".white(),
            "─".repeat(80).white()
        )));

        // Calculate max field name width for alignment
        let max_name_width = result
            .fields
            .iter()
            .map(|f| f.name.len())
            .max()
            .unwrap_or(0);

        for field in &result.fields {
            let nullable_str = if field.required { "" } else { " (nullable)" };
            let id_marker = if field.is_identifier { " [ID]" } else { "" };

            if options.verbose {
                // Verbose: show field ID after name
                let name_with_id = format!("{} ({})", field.name, field.field_id);
                let max_verbose_width = max_name_width + 6; // account for " (XX)"
                schema_items.push(BoxItem::text(format!(
                    "  {:<width$}  {}{}{}",
                    name_with_id.white().bold(),
                    field.field_type.cyan(),
                    nullable_str.dimmed(),
                    id_marker.yellow(),
                    width = max_verbose_width
                )));

                // Show doc string if present
                if let Some(ref doc) = field.doc {
                    schema_items.push(BoxItem::text(format!(
                        "    {} {}",
                        "doc:".dimmed(),
                        doc.dimmed()
                    )));
                }
            } else {
                // Normal mode: simpler format
                schema_items.push(BoxItem::text(format!(
                    "  {:<width$}  {}{}{}",
                    field.name.white().bold(),
                    field.field_type.cyan(),
                    nullable_str.dimmed(),
                    id_marker.yellow(),
                    width = max_name_width
                )));
            }
        }

        container = container.section(BoxSection::titled("Schema").items(schema_items));

        // ═══════════════════════════════════════════════════════════════════════
        // PARTITION & SORT
        // ═══════════════════════════════════════════════════════════════════════
        let mut partition_items = Vec::new();

        if result.partition_fields.is_empty() {
            partition_items.push(BoxItem::kv_aligned("Partitioning", "Unpartitioned", 14));
        } else {
            partition_items.push(BoxItem::kv_aligned(
                "Partition Spec",
                format!("ID {}", result.partition_spec_id),
                14,
            ));
            for (i, field) in result.partition_fields.iter().enumerate() {
                let is_last = i == result.partition_fields.len() - 1;
                let prefix = if is_last { "└" } else { "├" };
                partition_items.push(BoxItem::text(format!(
                    "{} {}: {}",
                    prefix.bright_black(),
                    field.name.white().bold(),
                    field.transform.cyan()
                )));
            }
        }

        if result.sort_fields.is_empty() {
            partition_items.push(BoxItem::kv_aligned("Sort Order", "Unsorted", 14));
        } else {
            partition_items.push(BoxItem::kv_aligned(
                "Sort Order",
                format!("ID {}", result.sort_order_id),
                14,
            ));
            for (i, sf) in result.sort_fields.iter().enumerate() {
                let is_last = i == result.sort_fields.len() - 1;
                let prefix = if is_last { "└" } else { "├" };
                partition_items.push(BoxItem::text(format!(
                    "{} field {}: {} {}",
                    prefix.bright_black(),
                    sf.source_id,
                    sf.direction.cyan(),
                    sf.null_order.dimmed()
                )));
            }
        }

        container =
            container.section(BoxSection::titled("Partition & Sort").items(partition_items));

        // ═══════════════════════════════════════════════════════════════════════
        // PROPERTIES
        // ═══════════════════════════════════════════════════════════════════════
        if !result.properties.is_empty() {
            let mut prop_items = Vec::new();

            if options.verbose {
                // Show ALL properties sorted alphabetically
                let mut sorted_props: Vec<_> = result.properties.iter().collect();
                sorted_props.sort_by_key(|(k, _)| *k);
                for (key, value) in sorted_props {
                    prop_items.push(BoxItem::text(format!("{} = {}", key.cyan(), value)));
                }
            } else {
                // Show only the most important properties
                let key_properties = [
                    "write.format.default",
                    "write.parquet.compression-codec",
                    "write.target-file-size-bytes",
                    "write.delete.mode",
                    "write.update.mode",
                    "write.merge.mode",
                ];

                for key in &key_properties {
                    if let Some(value) = result.properties.get(*key) {
                        prop_items.push(BoxItem::text(format!("{} = {}", key.cyan(), value)));
                    }
                }
            }

            if !prop_items.is_empty() {
                container = container.section(BoxSection::titled("Properties").items(prop_items));
            }
        }

        // ═══════════════════════════════════════════════════════════════════════
        // REFS (branches and tags) - verbose mode only
        // ═══════════════════════════════════════════════════════════════════════
        if options.verbose && !result.refs.is_empty() {
            let mut ref_items = Vec::new();
            for r in &result.refs {
                let type_str = if r.ref_type == "branch" {
                    "branch".green()
                } else {
                    "tag".blue()
                };
                ref_items.push(BoxItem::text(format!(
                    "{} ({}) → snapshot {}",
                    r.name.white().bold(),
                    type_str,
                    r.snapshot_id
                )));
            }
            container = container.section(BoxSection::titled("Refs").items(ref_items));
        }

        // ═══════════════════════════════════════════════════════════════════════
        // METADATA (verbose mode only)
        // ═══════════════════════════════════════════════════════════════════════
        if options.verbose {
            let mut meta_items = vec![
                BoxItem::kv_aligned("Schema Versions", result.schemas_count.to_string(), 20),
                BoxItem::kv_aligned(
                    "Partition Specs",
                    result.partition_specs_count.to_string(),
                    20,
                ),
                BoxItem::kv_aligned("Sort Orders", result.sort_orders_count.to_string(), 20),
            ];

            if !result.metadata_log.is_empty() {
                meta_items.push(BoxItem::kv_aligned(
                    "Metadata Files",
                    result.metadata_log.len().to_string(),
                    20,
                ));
                // Show current metadata file location
                if let Some(latest) = result.metadata_log.last() {
                    meta_items.push(BoxItem::kv_aligned(
                        "Current Metadata",
                        &latest.metadata_file,
                        20,
                    ));
                }
            }

            container = container.section(BoxSection::titled("Metadata").items(meta_items));
        }

        renderer.render(container)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::datatypes::{Field, TimeUnit};
    use std::sync::Arc;

    #[test]
    fn test_format_data_type_primitives() {
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::Int8),
            "int8"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::Int16),
            "int16"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::Int32),
            "int32"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::Int64),
            "int64"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::UInt8),
            "uint8"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::UInt16),
            "uint16"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::UInt32),
            "uint32"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::UInt64),
            "uint64"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::Float32),
            "float32"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::Float64),
            "float64"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::Utf8),
            "string"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::Boolean),
            "bool"
        );
    }

    #[test]
    fn test_format_data_type_dates() {
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::Date32),
            "date32"
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&DataType::Date64),
            "date64"
        );
    }

    #[test]
    fn test_format_data_type_timestamp() {
        let ts = DataType::Timestamp(TimeUnit::Microsecond, None);
        assert_eq!(
            InspectionFormatter::format_data_type(&ts),
            "timestamp(Microsecond)"
        );

        let ts_with_tz = DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into()));
        assert_eq!(
            InspectionFormatter::format_data_type(&ts_with_tz),
            "timestamp(Millisecond) (UTC)"
        );
    }

    #[test]
    fn test_format_data_type_list() {
        let list = DataType::List(Arc::new(Field::new("item", DataType::Int32, true)));
        assert_eq!(InspectionFormatter::format_data_type(&list), "list<int32>");
    }

    #[test]
    fn test_format_data_type_struct() {
        let struct_type = DataType::Struct(
            vec![
                Field::new("a", DataType::Int32, false),
                Field::new("b", DataType::Utf8, true),
            ]
            .into(),
        );
        assert_eq!(
            InspectionFormatter::format_data_type(&struct_type),
            "struct<a: int32, b: string>"
        );
    }

    #[test]
    fn test_format_data_type_decimal() {
        let decimal = DataType::Decimal128(10, 2);
        assert_eq!(
            InspectionFormatter::format_data_type(&decimal),
            "decimal(10, 2)"
        );
    }

    #[test]
    fn test_format_schema_empty() {
        let schema = Schema::empty();
        let result = InspectionFormatter::format_schema_content(&schema);
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_schema_with_fields() {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, true),
        ]);
        let result = InspectionFormatter::format_schema_content(&schema);
        let joined = result.join("\n");
        assert!(joined.contains("id"));
        assert!(joined.contains("name"));
        assert!(joined.contains("int64"));
        assert!(joined.contains("string"));
    }

    #[test]
    fn test_format_metadata_basic() {
        let metadata = FileMetadata {
            num_rows: Some(1000),
            compressed_size: Some(5000),
            uncompressed_size: Some(10000),
            compression: Some("snappy".to_string()),
            format_version: Some("2".to_string()),
            created_at: None,
            metadata: std::collections::HashMap::new(),
        };
        let result = InspectionFormatter::format_metadata_content(&metadata);
        let joined = result.join("\n");
        assert!(joined.contains("snappy"));
        assert!(joined.contains("50.0%")); // compression ratio
    }

    #[test]
    fn test_format_metadata_minimal() {
        let metadata = FileMetadata {
            num_rows: None,
            compressed_size: None,
            uncompressed_size: None,
            compression: None,
            format_version: None,
            created_at: None,
            metadata: std::collections::HashMap::new(),
        };
        let result = InspectionFormatter::format_metadata_content(&metadata);
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_statistics_basic() {
        let stats = vec![
            ColumnStats {
                name: "id".to_string(),
                null_count: Some(0),
                distinct_count: Some(100),
                min_value: Some("1".to_string()),
                max_value: Some("100".to_string()),
                mean: None,
                std_dev: None,
            },
            ColumnStats {
                name: "value".to_string(),
                null_count: Some(5),
                distinct_count: None,
                min_value: Some("0.5".to_string()),
                max_value: Some("99.5".to_string()),
                mean: Some(50.0),
                std_dev: Some(25.0),
            },
        ];
        let result = InspectionFormatter::format_statistics(&stats);
        assert!(result.contains("id"));
        assert!(result.contains("value"));
        assert!(result.contains("100"));
        assert!(result.contains("50.00"));
        assert!(result.contains("25.00"));
    }

    #[test]
    fn test_format_statistics_with_nulls() {
        let stats = vec![ColumnStats {
            name: "col".to_string(),
            null_count: None,
            distinct_count: None,
            min_value: None,
            max_value: None,
            mean: None,
            std_dev: None,
        }];
        let result = InspectionFormatter::format_statistics(&stats);
        assert!(result.contains("col"));
        assert!(result.contains("N/A"));
    }

    #[test]
    fn test_format_record_batch_empty() {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, true),
        ]);
        let batch = RecordBatch::new_empty(Arc::new(schema));
        let result = InspectionFormatter::format_record_batch(&batch);
        assert!(result.contains("id"));
        assert!(result.contains("name"));
    }

    #[test]
    fn test_format_record_batch_with_data() {
        use arrow::array::{Int64Array, StringArray};

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, true),
        ]));

        let ids = Int64Array::from(vec![1, 2, 3]);
        let names = StringArray::from(vec![Some("Alice"), Some("Bob"), None]);

        let batch = RecordBatch::try_new(schema, vec![Arc::new(ids), Arc::new(names)]).unwrap();

        let result = InspectionFormatter::format_record_batch(&batch);
        assert!(result.contains("Alice"));
        assert!(result.contains("Bob"));
        assert!(result.contains("NULL"));
    }

    #[test]
    fn test_format_record_batch_truncation() {
        use arrow::array::Int64Array;

        let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));

        // Create batch with more than 10 rows
        let ids: Vec<i64> = (0..20).collect();
        let id_array = Int64Array::from(ids);

        let batch = RecordBatch::try_new(schema, vec![Arc::new(id_array)]).unwrap();

        let result = InspectionFormatter::format_record_batch(&batch);
        assert!(result.contains("showing 10 of 20 rows"));
    }

    #[test]
    fn test_format_schema_content_single_field() {
        let schema = Schema::new(vec![Field::new("only_field", DataType::Int32, false)]);
        let content = InspectionFormatter::format_schema_content(&schema);
        assert_eq!(content.len(), 1);
        assert!(content[0].contains("└")); // Last field marker
        assert!(content[0].contains("only_field"));
    }

    #[test]
    fn test_format_schema_content_multiple_fields() {
        let schema = Schema::new(vec![
            Field::new("first", DataType::Int32, false),
            Field::new("middle", DataType::Utf8, true),
            Field::new("last", DataType::Boolean, false),
        ]);
        let content = InspectionFormatter::format_schema_content(&schema);
        assert_eq!(content.len(), 3);
        assert!(content[0].contains("├")); // Non-last marker
        assert!(content[1].contains("├"));
        assert!(content[2].contains("└")); // Last marker
    }

    #[test]
    fn test_format_metadata_content_with_compression() {
        let metadata = FileMetadata {
            num_rows: Some(1000),
            compressed_size: Some(1000),
            uncompressed_size: Some(5000),
            compression: Some("zstd".to_string()),
            format_version: Some("3".to_string()),
            created_at: None,
            metadata: std::collections::HashMap::new(),
        };
        let content = InspectionFormatter::format_metadata_content(&metadata);
        assert!(content.iter().any(|l| l.contains("zstd")));
        assert!(content.iter().any(|l| l.contains("20.0%"))); // 1000/5000 = 20%
        assert!(content.iter().any(|l| l.contains("3")));
    }
}

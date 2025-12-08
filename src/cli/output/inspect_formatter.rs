//! Inspection result formatting utilities

use arrow::array::Array;
use arrow::datatypes::{DataType, Schema};
use arrow::record_batch::RecordBatch;
use colored::Colorize;
use comfy_table::{Cell, CellAlignment, Color, Table, presets};

use crate::core::format_bytes;
use crate::core::formats::{ColumnStats, FileMetadata};
use crate::core::inspection::formatters::format_number;
use crate::core::operations::inspect::InspectResult;

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

    /// Format schema with tree structure (legacy)
    pub fn format_schema(schema: &Schema) -> String {
        Self::format_schema_content(schema)
            .iter()
            .map(|line| format!("  {}", line))
            .collect::<Vec<_>>()
            .join("\n")
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

    /// Format metadata (legacy)
    pub fn format_metadata(metadata: &FileMetadata) -> String {
        let mut output = Vec::new();

        if let Some(rows) = metadata.num_rows {
            output.push(format!("  Rows: {}", rows.to_string().bright_white()));
        }

        if let Some(compressed) = metadata.compressed_size {
            output.push(format!(
                "  Compressed Size: {}",
                format_bytes(compressed).bright_white()
            ));
        }

        if let Some(uncompressed) = metadata.uncompressed_size {
            output.push(format!(
                "  Uncompressed Size: {}",
                format_bytes(uncompressed).bright_white()
            ));

            if let Some(compressed) = metadata.compressed_size {
                let ratio = (compressed as f64 / uncompressed as f64) * 100.0;
                output.push(format!(
                    "  Compression Ratio: {}",
                    format!("{:.1}%", ratio).bright_white()
                ));
            }
        }

        if let Some(compression) = &metadata.compression {
            output.push(format!("  Compression: {}", compression.bright_white()));
        }

        if let Some(version) = &metadata.format_version {
            output.push(format!("  Format Version: {}", version.bright_white()));
        }

        for (key, value) in &metadata.metadata {
            output.push(format!("  {}: {}", key, value.bright_white()));
        }

        output.join("\n")
    }

    /// Format column statistics as a table
    pub fn format_statistics(stats: &[ColumnStats]) -> String {
        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);

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
        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);

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
}

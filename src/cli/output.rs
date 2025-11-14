//! Output formatting utilities

use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use colored::Colorize;
use comfy_table::{Attribute, Cell, CellAlignment, Color, ContentArrangement, Table, presets, modifiers::UTF8_ROUND_CORNERS};

use crate::core::formats::{ColumnStats, FileMetadata};
use crate::core::operations::inspect::InspectResult;

/// Output formatter for different formats
pub struct OutputFormatter;

impl OutputFormatter {
    /// Format output as a table
    pub fn format_table(headers: Vec<String>, rows: Vec<Vec<String>>) -> String {
        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_header(headers);

        for row in rows {
            table.add_row(row);
        }

        table.to_string()
    }

    /// Format output as JSON
    pub fn format_json<T: serde::Serialize>(data: &T) -> crate::error::Result<String> {
        serde_json::to_string_pretty(data).map_err(|e| crate::error::Error::General(e.to_string()))
    }

    /// Format output as YAML
    pub fn format_yaml<T: serde::Serialize>(data: &T) -> crate::error::Result<String> {
        serde_yaml::to_string(data).map_err(|e| crate::error::Error::General(e.to_string()))
    }

    /// Format a success message
    pub fn success(message: &str) -> String {
        format!("{} {}", "✓".green(), message)
    }

    /// Format an error message
    pub fn error(message: &str) -> String {
        format!("{} {}", "✗".red(), message)
    }

    /// Format a warning message
    pub fn warning(message: &str) -> String {
        format!("{} {}", "⚠".yellow(), message)
    }

    /// Format an info message
    pub fn info(message: &str) -> String {
        format!("{} {}", "ℹ".blue(), message)
    }

    /// Format an InspectResult for display
    pub fn format_inspect_result(result: &InspectResult) -> String {
        let mut output = Vec::new();

        // Header
        output.push(format!("\n{}: {}\n", "Format".bold(), result.format_name));

        // Metadata
        if let Some(metadata) = &result.metadata {
            output.push(format!("{}", "Metadata".bold()));
            output.push(Self::format_metadata(metadata));
            output.push(String::new());
        }

        // Schema
        output.push(format!("{}", "Schema".bold()));
        output.push(Self::format_schema(&result.schema));
        output.push(String::new());

        // Statistics
        if let Some(stats) = &result.statistics {
            output.push(format!("{}", "Statistics".bold()));
            output.push(Self::format_statistics(stats));
            output.push(String::new());
        }

        // Sample Data
        if let Some(batch) = &result.sample_data {
            output.push(format!("{}", "Data".bold()));
            output.push(Self::format_record_batch(batch));
        }

        output.join("\n")
    }

    /// Format schema with tree structure
    pub fn format_schema(schema: &Schema) -> String {
        let mut output = Vec::new();

        for (i, field) in schema.fields().iter().enumerate() {
            let is_last = i == schema.fields().len() - 1;
            let prefix = if is_last { "└" } else { "├" };

            let nullable_marker = if field.is_nullable() { " (nullable)" } else { "" };
            let type_str = Self::format_data_type(field.data_type());

            output.push(format!(
                "  {} {}: {}{}",
                prefix.bright_black(),
                field.name().white(),
                type_str.yellow(),
                nullable_marker.bright_black()
            ));
        }

        output.join("\n")
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

    /// Format metadata
    pub fn format_metadata(metadata: &FileMetadata) -> String {
        let mut output = Vec::new();

        if let Some(rows) = metadata.num_rows {
            output.push(format!("  Rows: {}", rows.to_string().bright_white()));
        }

        if let Some(compressed) = metadata.compressed_size {
            output.push(format!(
                "  Compressed Size: {}",
                Self::format_bytes(compressed).bright_white()
            ));
        }

        if let Some(uncompressed) = metadata.uncompressed_size {
            output.push(format!(
                "  Uncompressed Size: {}",
                Self::format_bytes(uncompressed).bright_white()
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

    /// Format bytes to human-readable size
    fn format_bytes(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{} {}", size as u64, UNITS[unit_index])
        } else {
            format!("{:.2} {}", size, UNITS[unit_index])
        }
    }

    /// Format column statistics
    pub fn format_statistics(stats: &[ColumnStats]) -> String {
        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);

        // Set headers with formatting
        let headers = vec!["Column", "Null Count", "Min", "Max"];
        table.set_header(headers.iter().map(|h| {
            Cell::new(h)
                .fg(Color::White)
                .add_attribute(Attribute::Bold)
                .set_alignment(CellAlignment::Center)
        }));

        for stat in stats {
            let null_count = stat
                .null_count
                .map(|n| n.to_string())
                .unwrap_or_else(|| "N/A".to_string());
            let min_val = stat.min_value.as_deref().unwrap_or("N/A");
            let max_val = stat.max_value.as_deref().unwrap_or("N/A");

            table.add_row(vec![
                Cell::new(&stat.name),
                Cell::new(null_count),
                Cell::new(min_val),
                Cell::new(max_val),
            ]);
        }

        table.to_string()
    }

    /// Format RecordBatch as table
    pub fn format_record_batch(batch: &RecordBatch) -> String {
        if batch.num_rows() == 0 {
            return "No data available".to_string();
        }

        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);

        // Set headers with white color and bold
        let headers: Vec<_> = batch
            .schema()
            .fields()
            .iter()
            .map(|f| {
                Cell::new(f.name())
                    .fg(Color::White)
                    .add_attribute(Attribute::Bold)
                    .set_alignment(CellAlignment::Center)
            })
            .collect();
        table.set_header(headers);

        // Add rows
        for row_idx in 0..batch.num_rows() {
            let mut row_data = Vec::new();

            for col in batch.columns() {
                let value = arrow::util::display::array_value_to_string(col, row_idx)
                    .unwrap_or_else(|_| "Error".to_string());
                row_data.push(value);
            }

            table.add_row(row_data);
        }

        format!("{}\n({} rows)", table, batch.num_rows())
    }
}

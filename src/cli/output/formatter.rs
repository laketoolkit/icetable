//! Output formatting utilities

use arrow::datatypes::{DataType, Schema};
use arrow::record_batch::RecordBatch;
use colored::Colorize;
use comfy_table::{Attribute, Cell, CellAlignment, Color, Table, presets};
use unicode_width::UnicodeWidthStr;

use crate::core::formats::{ColumnStats, FileMetadata};
use crate::core::operations::inspect::InspectResult;


/// Status icons for overall status (valid/invalid/warning)
#[derive(Debug, Clone, Copy)]
pub enum StatusIcon {
    /// Success/Valid (green tick)
    Success,
    /// Warning (yellow warning sign)
    Warning,
    /// Error/Failed (red cross)
    Error,
}

impl StatusIcon {
    /// Get the colored icon as a string
    pub fn as_str(&self) -> String {
        match self {
            StatusIcon::Success => "✓".green().to_string(),
            StatusIcon::Warning => "⚠".yellow().to_string(),
            StatusIcon::Error => "✗".red().to_string(),
        }
    }

    /// Get just the icon without color
    pub fn icon(&self) -> &'static str {
        match self {
            StatusIcon::Success => "✓",
            StatusIcon::Warning => "⚠",
            StatusIcon::Error => "✗",
        }
    }
}

impl std::fmt::Display for StatusIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Severity icons for individual messages (same shape, different colors)
#[derive(Debug, Clone, Copy)]
pub enum SeverityIcon {
    /// Error message (red)
    Error,
    /// Warning message (yellow)
    Warning,
    /// Info message (cyan)
    Info,
}

impl SeverityIcon {
    /// Get the colored icon as a string
    pub fn as_str(&self) -> String {
        match self {
            SeverityIcon::Error => "🛈".red().to_string(),
            SeverityIcon::Warning => "🛈".yellow().to_string(),
            SeverityIcon::Info => "🛈".cyan().to_string(),
        }
    }

    /// Get just the icon without color
    pub fn icon(&self) -> &'static str {
        "🛈"
    }
}

impl std::fmt::Display for SeverityIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Strip ANSI escape codes from a string
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1B' {
            // ESC character - start of escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we hit a letter (the command character)
                while let Some(&next_ch) = chars.peek() {
                    chars.next();
                    if next_ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Get the visual width of a string (after stripping ANSI codes)
fn visual_width(s: &str) -> usize {
    let stripped = strip_ansi_codes(s);
    UnicodeWidthStr::width(stripped.as_str())
}

/// Wrap a line into multiple lines respecting visual width
/// For lines with colored icons, preserve the first line and only wrap the content
fn wrap_line(line: &str, max_width: usize) -> Vec<String> {
    let stripped = strip_ansi_codes(line);
    let width = UnicodeWidthStr::width(stripped.as_str());

    if width <= max_width {
        return vec![line.to_string()];
    }

    // Detect if this line has a colored icon (contains ANSI codes + emoji)
    let has_colored_icon = line.contains("\x1B[") && line.contains("🛈");

    // Detect leading whitespace/indentation
    let leading_spaces = stripped.chars().take_while(|c| c.is_whitespace()).count();
    let continuation_indent = "     "; // 5 spaces for continuation lines

    if has_colored_icon {
        // Special handling for colored icon lines
        // Split at the dash after the icon to separate rule name from message
        if let Some(dash_pos) = stripped.find(" - ") {
            let before_dash = &stripped[..dash_pos + 3]; // Include " - "
            let after_dash = &stripped[dash_pos + 3..];

            let before_width = UnicodeWidthStr::width(before_dash);

            if before_width <= max_width {
                // First line: preserve original up to dash (with colors)
                let original_before = if let Some(orig_dash) = line.find(" - ") {
                    &line[..orig_dash + 3]
                } else {
                    line
                };

                let mut result = vec![original_before.to_string()];

                // Wrap the rest
                let words: Vec<&str> = after_dash.split_whitespace().collect();
                let mut current_line = String::new();
                let mut current_width = 0;

                for word in words {
                    let word_width = UnicodeWidthStr::width(word);
                    let space_needed = if current_line.is_empty() { 0 } else { 1 };
                    let available_width = max_width.saturating_sub(continuation_indent.len());

                    if current_width + space_needed + word_width <= available_width {
                        if !current_line.is_empty() {
                            current_line.push(' ');
                            current_width += 1;
                        }
                        current_line.push_str(word);
                        current_width += word_width;
                    } else {
                        if !current_line.is_empty() {
                            result.push(format!("{}{}", continuation_indent, current_line));
                        }
                        current_line = word.to_string();
                        current_width = word_width;
                    }
                }

                if !current_line.is_empty() {
                    result.push(format!("{}{}", continuation_indent, current_line));
                }

                return result;
            }
        }
    }

    // Standard wrapping for non-icon lines
    let words: Vec<&str> = stripped.split_whitespace().collect();
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;
    let mut is_first_line = true;

    for word in words {
        let word_width = UnicodeWidthStr::width(word);
        let space_needed = if current_line.is_empty() { 0 } else { 1 };
        let line_indent = if is_first_line {
            0
        } else {
            continuation_indent.len()
        };
        let available_width = max_width.saturating_sub(line_indent);

        if current_width + space_needed + word_width <= available_width {
            if !current_line.is_empty() {
                current_line.push(' ');
                current_width += 1;
            }
            current_line.push_str(word);
            current_width += word_width;
        } else {
            if !current_line.is_empty() {
                let final_line = if is_first_line {
                    format!(
                        "{}{}",
                        " ".repeat(leading_spaces),
                        current_line.trim_start()
                    )
                } else {
                    format!("{}{}", continuation_indent, current_line)
                };
                lines.push(final_line);
                is_first_line = false;
            }
            current_line = word.to_string();
            current_width = word_width;
        }
    }

    if !current_line.is_empty() {
        let final_line = if is_first_line {
            format!(
                "{}{}",
                " ".repeat(leading_spaces),
                current_line.trim_start()
            )
        } else {
            format!("{}{}", continuation_indent, current_line)
        };
        lines.push(final_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

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
        format!("{} {}", StatusIcon::Success, message)
    }

    /// Format an error message
    pub fn error(message: &str) -> String {
        format!("{}  {}", SeverityIcon::Error, message)
    }

    /// Format a warning message
    pub fn warning(message: &str) -> String {
        format!("{}  {}", SeverityIcon::Warning, message)
    }

    /// Format an info message
    pub fn info(message: &str) -> String {
        format!("{}  {}", SeverityIcon::Info, message)
    }

    /// Create a framed box with title and content lines
    ///
    /// # Arguments
    /// * `title` - Optional title to display centered in the top border
    /// * `lines` - Vector of content lines to display in the box
    /// * `width` - Fixed width of the box (default 80)
    ///
    /// # Returns
    /// A formatted string with the framed content
    pub fn framed_box(title: Option<&str>, lines: Vec<String>, width: Option<usize>) -> String {
        let box_width = width.unwrap_or(80);
        let content_width = box_width - 2; // -2 for left and right borders

        let mut output = Vec::new();

        // Top border with optional centered title
        if let Some(t) = title {
            let title_width = UnicodeWidthStr::width(t);
            let padding_total = content_width.saturating_sub(title_width);
            let padding_left = padding_total / 2;
            let padding_right = padding_total - padding_left;
            output.push(format!(
                "┌{}{}{}┐",
                "─".repeat(padding_left),
                t,
                "─".repeat(padding_right)
            ));
        } else {
            output.push(format!("┌{}┐", "─".repeat(content_width)));
        }

        // Empty line after title
        output.push(format!("│{:width$}│", "", width = content_width));

        // Content lines
        for line in lines {
            if line.is_empty() {
                // Empty line
                output.push(format!("│{:width$}│", "", width = content_width));
            } else {
                let line_width = visual_width(&line);

                if line_width <= content_width {
                    // Line fits, pad it
                    let padding = content_width - line_width;
                    output.push(format!("│{}{:width$}│", line, "", width = padding));
                } else {
                    // Line too long, wrap it
                    let wrapped = wrap_line(&line, content_width);
                    for wrapped_line in wrapped {
                        let wrapped_width = visual_width(&wrapped_line);
                        let padding = content_width.saturating_sub(wrapped_width);
                        output.push(format!("│{}{:width$}│", wrapped_line, "", width = padding));
                    }
                }
            }
        }

        // Empty line before bottom
        output.push(format!("│{:width$}│", "", width = content_width));

        // Bottom border
        output.push(format!("└{}┘", "─".repeat(content_width)));

        output.join("\n")
    }

    /// Format an InspectResult for display
    pub fn format_inspect_result(
        result: &InspectResult,
        options: &crate::core::operations::inspect::InspectOptions,
    ) -> String {
        let mut output = Vec::new();

        // Header
        output.push(format!("\n{}: {}\n", "Format".bold(), result.format_name));

        // Metadata (if requested and available)
        if options.show_metadata {
            if let Some(metadata) = &result.metadata {
                output.push(format!("{}", "Metadata".bold()));
                output.push(Self::format_metadata(metadata));
                output.push(String::new());
            }
        }

        // Schema (if requested)
        if options.show_schema {
            output.push(format!("{}", "Schema".bold()));
            output.push(Self::format_schema(&result.schema));
            output.push(String::new());
        }

        // Statistics (if requested and available)
        if options.show_stats {
            if let Some(stats) = &result.statistics {
                output.push(format!("{}", "Statistics".bold()));
                output.push(Self::format_statistics(stats));
                output.push(String::new());
            }
        }

        // Sample Data (if requested and available)
        if options.show_data {
            if let Some(batch) = &result.sample_data {
                output.push(format!("{}", "Data".bold()));
                output.push(Self::format_record_batch(batch));
            }
        }

        output.join("\n")
    }

    /// Format schema with tree structure
    pub fn format_schema(schema: &Schema) -> String {
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
                "  {} {}: {}{}",
                prefix.bright_black(),
                field.name().white().bold(),
                type_str.cyan(),
                nullable_marker.dimmed()
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

            for (_col_idx, col) in batch.columns().iter().enumerate() {
                // Check if value is null
                if col.is_null(row_idx) {
                    // Create a red colored "null" cell for visibility
                    let cell = Cell::new("null").fg(Color::Red);
                    row_data.push(cell);
                } else {
                    let value = arrow::util::display::array_value_to_string(col, row_idx)
                        .unwrap_or_else(|_| "Error".to_string());
                    row_data.push(Cell::new(value));
                }
            }

            table.add_row(row_data);
        }

        format!("{}\n({} rows)", table, batch.num_rows())
    }

    /// Format stats result for display
    pub fn format_stats_result(
        result: &crate::core::operations::stats::StatsResult,
        _options: &crate::core::operations::stats::StatsOptions,
    ) -> String {
        let mut output = Vec::new();

        output.push(format!(
            "\n{}: {}\n",
            "Total Rows".bold(),
            result.total_rows.to_string().bright_white()
        ));

        for col_stat in &result.column_stats {
            output.push(format!(
                "{}",
                format!("Column: {}", col_stat.name).bold().underline()
            ));
            output.push(format!(
                "  Null Count: {}",
                col_stat.null_count.to_string().bright_white()
            ));
            output.push(format!(
                "  Non-Null Count: {}",
                col_stat.non_null_count.to_string().bright_white()
            ));

            // Numeric stats
            if let Some(ref num_stats) = col_stat.numeric_stats {
                output.push(format!("  {}", "Numeric Statistics:".cyan()));

                if let Some(min) = num_stats.min {
                    output.push(format!("    Min: {}", format!("{:.6}", min).bright_white()));
                }
                if let Some(max) = num_stats.max {
                    output.push(format!("    Max: {}", format!("{:.6}", max).bright_white()));
                }
                if let Some(mean) = num_stats.mean {
                    output.push(format!(
                        "    Mean: {}",
                        format!("{:.6}", mean).bright_white()
                    ));
                }
                if let Some(median) = num_stats.median {
                    output.push(format!(
                        "    Median: {}",
                        format!("{:.6}", median).bright_white()
                    ));
                }
                if let Some(std_dev) = num_stats.std_dev {
                    output.push(format!(
                        "    Std Dev: {}",
                        format!("{:.6}", std_dev).bright_white()
                    ));
                }

                if let Some(ref percentiles) = num_stats.percentiles {
                    output.push(format!("    {}", "Percentiles:".cyan()));
                    let mut sorted_percentiles: Vec<_> = percentiles.iter().collect();
                    sorted_percentiles.sort_by_key(|(k, _)| *k);
                    for (name, value) in sorted_percentiles {
                        output.push(format!(
                            "      {}: {}",
                            name,
                            format!("{:.6}", value).bright_white()
                        ));
                    }
                }
            }

            // String stats
            if let Some(ref str_stats) = col_stat.string_stats {
                output.push(format!("  {}", "String Statistics:".cyan()));

                if let Some(min_len) = str_stats.min_length {
                    output.push(format!(
                        "    Min Length: {}",
                        min_len.to_string().bright_white()
                    ));
                }
                if let Some(max_len) = str_stats.max_length {
                    output.push(format!(
                        "    Max Length: {}",
                        max_len.to_string().bright_white()
                    ));
                }
                if let Some(avg_len) = str_stats.avg_length {
                    output.push(format!(
                        "    Avg Length: {}",
                        format!("{:.2}", avg_len).bright_white()
                    ));
                }
                if let Some(distinct) = str_stats.distinct_count {
                    output.push(format!(
                        "    Distinct Values: {}",
                        distinct.to_string().bright_white()
                    ));
                }

                if let Some(ref most_common) = str_stats.most_common {
                    output.push(format!("    {}", "Most Common Values:".cyan()));
                    for (value, count) in most_common {
                        let display_value = if value.len() > 50 {
                            format!("{}...", &value[..47])
                        } else {
                            value.clone()
                        };
                        output.push(format!("      \"{}\" ({})", display_value, count));
                    }
                }
            }

            // Boolean stats
            if let Some(ref bool_stats) = col_stat.boolean_stats {
                output.push(format!("  {}", "Boolean Statistics:".cyan()));
                output.push(format!(
                    "    True Count: {}",
                    bool_stats.true_count.to_string().bright_white()
                ));
                output.push(format!(
                    "    False Count: {}",
                    bool_stats.false_count.to_string().bright_white()
                ));
                output.push(format!(
                    "    True %: {}",
                    format!("{:.2}%", bool_stats.true_percentage).bright_white()
                ));
            }

            // Temporal stats
            if let Some(ref temp_stats) = col_stat.temporal_stats {
                output.push(format!("  {}", "Temporal Statistics:".cyan()));
                if let Some(ref min) = temp_stats.min {
                    output.push(format!("    Min: {}", min.bright_white()));
                }
                if let Some(ref max) = temp_stats.max {
                    output.push(format!("    Max: {}", max.bright_white()));
                }
            }

            output.push(String::new()); // Empty line between columns
        }

        output.join("\n")
    }

    /// Format structural diff result for display
    pub fn format_diff_result(result: &crate::core::operations::diff::DiffResult) -> String {
        let mut output = Vec::new();

        // Header box with file paths
        let left_name = std::path::Path::new(&result.left_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&result.left_path);
        let right_name = std::path::Path::new(&result.right_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&result.right_path);

        // Detect format from file extension
        let format_name = if left_name.ends_with(".parquet") || right_name.ends_with(".parquet") {
            "Parquet Diff"
        } else if left_name.ends_with(".arrow") || right_name.ends_with(".arrow") {
            "Arrow Diff"
        } else if left_name.ends_with(".csv") || right_name.ends_with(".csv") {
            "CSV Diff"
        } else if left_name.ends_with(".json") || right_name.ends_with(".json") {
            "JSON Diff"
        } else {
            "Table Diff"
        };

        let file_paths = format!("{} {} {}", left_name, "→".cyan().bold(), right_name);

        // Center the file paths - need to use visual width (without ANSI codes)
        let box_width: usize = 72;
        // Calculate visual width: left_name + " " + "→" + " " + right_name
        let visual_len = left_name.len() + 1 + 1 + 1 + right_name.len();
        let padding_needed = box_width.saturating_sub(visual_len);
        let left_pad = padding_needed / 2;
        let right_pad = padding_needed - left_pad;
        let centered_paths = format!("{}{}{}",
            " ".repeat(left_pad),
            file_paths,
            " ".repeat(right_pad)
        );

        // Use the box_frame utility
        let header_box = crate::utils::create_box_frame(
            Some(format_name),
            vec![centered_paths],
            Some(box_width)
        );

        output.push(String::new());
        output.push(header_box);

        // ROWS section with emoji
        output.push(Self::format_rows_diff(&result.metadata_diff));
        output.push(String::new());

        // SCHEMA section with emoji
        output.push(Self::format_schema_diff_visual(&result.schema_diff));
        output.push(String::new());

        // METADATA section with emoji (always show)
        output.push(Self::format_metadata_diff_visual(&result.metadata_diff));

        // COLUMN STATISTICS (if verbose mode)
        if !result.column_stats_diff.is_empty() {
            output.push(String::new());
            output.push(Self::format_column_stats_diff(&result.column_stats_diff));
        }

        output.join("\n")
    }

    /// Check if there are metadata changes worth displaying
    fn has_metadata_changes(meta_diff: &crate::core::operations::diff::MetadataDiff) -> bool {
        meta_diff.compression.as_ref().map_or(false, |(l, r)| l != r)
            || !meta_diff.custom_metadata.is_empty()
            || meta_diff.format_version.as_ref().map_or(false, |(l, r)| l != r)
    }

    /// Format ROWS section with emoji
    fn format_rows_diff(meta_diff: &crate::core::operations::diff::MetadataDiff) -> String {
        let mut output = Vec::new();

        output.push(format!("{} {}", "📊", "ROWS".bold()));

        if let Some((left, right)) = meta_diff.num_rows {
            let delta = right - left;

            output.push(format!("  Total:  {} {} {}",
                Self::format_number(left),
                "→".cyan().bold(),
                Self::format_number(right)
            ));

            if delta != 0 {
                output.push(String::new());
                if delta > 0 {
                    output.push(format!("  Added:     {}", Self::format_number(delta).green()));
                    output.push(format!("  Removed:   {}", "0".red()));
                } else {
                    output.push(format!("  Added:     {}", "0".green()));
                    output.push(format!("  Removed:   {}", Self::format_number(delta.abs()).red()));
                }
            }
        }

        output.join("\n")
    }

    /// Format metadata differences (old style, kept for backward compatibility)
    fn format_metadata_diff(meta_diff: &crate::core::operations::diff::MetadataDiff) -> String {
        Self::format_metadata_diff_visual(meta_diff)
    }

    /// Format a number with thousands separator
    fn format_number(n: i64) -> String {
        let s = n.to_string();
        let mut result = String::new();
        let mut count = 0;

        for c in s.chars().rev() {
            if count > 0 && count % 3 == 0 {
                result.push(',');
            }
            result.push(c);
            count += 1;
        }

        result.chars().rev().collect()
    }

    /// Format schema differences with visual style
    fn format_schema_diff_visual(schema_diff: &crate::core::operations::diff::SchemaDiff) -> String {
        let mut output = Vec::new();

        output.push(format!("{} {}", "📋", "SCHEMA".bold()));

        // Show if schemas are identical
        if schema_diff.is_identical() {
            output.push(String::new());
            output.push(format!("  {} Schemas are identical", "✓".green()));
            return output.join("\n");
        }

        output.push(String::new());

        // Columns added
        if !schema_diff.columns_added.is_empty() {
            output.push(format!("  Added Columns ({})", schema_diff.columns_added.len()));
            for col in &schema_diff.columns_added {
                let nullable_str = if col.nullable { "nullable" } else { "non-null" };
                output.push(format!("    {} {:<20} {:<12} {}",
                    "+".green(),
                    col.name,
                    col.data_type,
                    nullable_str.dimmed()
                ));
            }
            output.push(String::new());
        }

        // Columns removed
        if !schema_diff.columns_removed.is_empty() {
            output.push(format!("  Removed Columns ({})", schema_diff.columns_removed.len()));
            for col in &schema_diff.columns_removed {
                let nullable_str = if col.nullable { "nullable" } else { "non-null" };
                output.push(format!("    {} {:<20} {}",
                    "-".red(),
                    col.name,
                    col.data_type.dimmed()
                ));
            }
            output.push(String::new());
        }

        // Columns modified
        if !schema_diff.columns_modified.is_empty() {
            output.push(format!("  Modified Columns ({})", schema_diff.columns_modified.len()));
            for col in &schema_diff.columns_modified {
                if let Some((old_type, new_type)) = &col.type_change {
                    let change_str = format!("{} {} {}", old_type, "→".cyan().bold(), new_type);
                    output.push(format!("    {} {:<20} {}",
                        "~".yellow(),
                        col.name,
                        change_str
                    ));
                }
                if let Some((old_nullable, new_nullable)) = col.nullability_change {
                    let null_change = if new_nullable {
                        "non-null → nullable"
                    } else {
                        "nullable → non-null"
                    };
                    output.push(format!("    {} {:<20} {}",
                        "~".yellow(),
                        col.name,
                        null_change
                    ));
                }
            }
        }

        output.join("\n")
    }

    /// Format metadata differences with visual style
    fn format_metadata_diff_visual(meta_diff: &crate::core::operations::diff::MetadataDiff) -> String {
        let mut output = Vec::new();

        output.push(format!("{} {}", "📦", "METADATA".bold()));
        output.push(String::new());

        output.push(format!("  {}", "File Properties".dimmed()));

        // Rows (always show if available)
        if let Some((left, right)) = meta_diff.num_rows {
            if left != right {
                output.push(format!("    Rows:         {} {} {}",
                    Self::format_number(left),
                    "→".cyan().bold(),
                    Self::format_number(right)
                ));
            }
        }

        // Sizes (always show if available)
        if let Some((left, right)) = meta_diff.compressed_size {
            if left != right {
                output.push(format!("    Size:         {} {} {}",
                    Self::format_bytes(left),
                    "→".cyan().bold(),
                    Self::format_bytes(right)
                ));
            } else {
                output.push(format!("    Size:         {}",
                    Self::format_bytes(left)
                ));
            }
        }

        // Compression (always show if available)
        if let Some((left, right)) = &meta_diff.compression {
            if left != right {
                output.push(format!("    Compression:  {} {} {}",
                    left.to_uppercase(),
                    "→".cyan().bold(),
                    right.to_uppercase()
                ));
            } else if !left.is_empty() {
                output.push(format!("    Compression:  {}",
                    left.to_uppercase()
                ));
            }
        }

        // Format version (always show if available)
        if let Some((left, right)) = &meta_diff.format_version {
            if left != right {
                output.push(format!("    Version:      {} {} {}", left, "→".cyan().bold(), right));
            } else {
                output.push(format!("    Version:      {}", left));
            }
        }

        // Custom metadata changes
        if !meta_diff.custom_metadata.is_empty() {
            let total_changes = meta_diff.custom_metadata.added.len()
                + meta_diff.custom_metadata.removed.len()
                + meta_diff.custom_metadata.modified.len();

            output.push(String::new());
            output.push(format!("  Custom Metadata ({} changes)", total_changes));

            for (key, value) in &meta_diff.custom_metadata.added {
                output.push(format!("    {} {} = {}",
                    "+".green(),
                    key,
                    value
                ));
            }

            for (key, value) in &meta_diff.custom_metadata.removed {
                output.push(format!("    {} {} = {}",
                    "-".red(),
                    key,
                    value
                ));
            }

            for (key, (old_val, new_val)) in &meta_diff.custom_metadata.modified {
                output.push(format!("    {} {}: {} → {}",
                    "~".yellow(),
                    key,
                    old_val,
                    new_val
                ));
            }
        }

        output.join("\n")
    }

    /// Format schema differences (old style, kept for backward compatibility)
    fn format_schema_diff(schema_diff: &crate::core::operations::diff::SchemaDiff) -> String {
        Self::format_schema_diff_visual(schema_diff)
    }

    /// Format column statistics differences (verbose mode)
    fn format_column_stats_diff(
        stats_diff: &[crate::core::operations::diff::ColumnStatsDiff],
    ) -> String {
        let mut output = Vec::new();

        output.push(format!("{}", "COLUMN STATISTICS".bold().underline()));

        for stat in stats_diff {
            output.push(String::new());
            output.push(format!("  {}", stat.name.bold()));

            let mut has_diff = false;

            // Null count
            if let Some((left, right)) = stat.null_count {
                if left != right {
                    has_diff = true;
                    let change_str = format!("{} → {}", Self::format_number(left), Self::format_number(right)).yellow();
                    output.push(format!("    Null count: {}", change_str));
                }
            }

            // Distinct count
            if let Some((left, right)) = stat.distinct_count_approx {
                if left != right {
                    has_diff = true;
                    let change_str = format!("{} → {}", Self::format_number(left), Self::format_number(right)).yellow();
                    output.push(format!("    Distinct count (approx): {}", change_str));
                }
            }

            // Min value
            if let Some((left, right)) = &stat.min_value {
                if left != right {
                    has_diff = true;
                    let change_str = format!("{} → {}", left, right).yellow();
                    output.push(format!("    Min: {}", change_str));
                }
            }

            // Max value
            if let Some((left, right)) = &stat.max_value {
                if left != right {
                    has_diff = true;
                    let change_str = format!("{} → {}", left, right).yellow();
                    output.push(format!("    Max: {}", change_str));
                }
            }

            // Mean
            if let Some((left, right)) = stat.mean {
                if (right - left).abs() > 0.0001 {
                    has_diff = true;
                    let diff = right - left;
                    let diff_marker = if diff > 0.0 {
                        format!("(+{:.4})", diff).green()
                    } else {
                        format!("({:.4})", diff).red()
                    };
                    let change_str = format!("{:.4} → {:.4} {}",
                        left,
                        right,
                        diff_marker
                    ).yellow();
                    output.push(format!("    Mean: {}", change_str));
                }
            }

            if !has_diff {
                output.push(format!("    {} No differences", "•".dimmed()));
            }
        }

        output.join("\n")
    }
}

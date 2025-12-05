//! Diff result formatting utilities

use colored::Colorize;


use crate::core::format_bytes;
use crate::core::inspection::formatters::format_number;
use crate::core::operations::diff::{ColumnStatsDiff, DiffResult, MetadataDiff, SchemaDiff};

/// Formatter for diff results
pub struct DiffFormatter;

impl DiffFormatter {
    /// Format structural diff result for display
    pub fn format_diff_result(result: &DiffResult) -> String {
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
        let centered_paths = format!(
            "{}{}{}",
            " ".repeat(left_pad),
            file_paths,
            " ".repeat(right_pad)
        );

        // Use the box_frame utility
        let header_box = crate::utils::create_box_frame(
            Some(format_name),
            vec![centered_paths],
            Some(box_width),
        );

        output.push(String::new());
        output.push(header_box);
        output.push(String::new());

        // ROWS section - wrapped in box
        let rows_content = Self::format_rows_diff_content(&result.metadata_diff);
        let rows_box = crate::utils::create_box_frame(
            Some(&"ROWS".bold().to_string()),
            rows_content,
            Some(72),
        );
        output.push(rows_box);
        output.push(String::new());

        // SCHEMA section - wrapped in box
        let schema_content = Self::format_schema_diff_content(&result.schema_diff);
        let schema_box = crate::utils::create_box_frame(
            Some(&"SCHEMA".bold().to_string()),
            schema_content,
            Some(72),
        );
        output.push(schema_box);
        output.push(String::new());

        // METADATA section - wrapped in box
        let metadata_content = Self::format_metadata_diff_content(&result.metadata_diff);
        let metadata_box = crate::utils::create_box_frame(
            Some(&"METADATA".bold().to_string()),
            metadata_content,
            Some(72),
        );
        output.push(metadata_box);

        // COLUMN STATISTICS (if verbose mode) - wrapped in box
        if !result.column_stats_diff.is_empty() {
            output.push(String::new());
            let stats_content = Self::format_column_stats_content(&result.column_stats_diff);
            let stats_box = crate::utils::create_box_frame(
                Some(&"COLUMN STATISTICS".bold().to_string()),
                stats_content,
                Some(72),
            );
            output.push(stats_box);
        }

        output.join("\n")
    }

    /// Format ROWS section content (returns lines for box)
    fn format_rows_diff_content(meta_diff: &MetadataDiff) -> Vec<String> {
        let mut output = Vec::new();

        if let Some((left, right)) = meta_diff.num_rows {
            let delta = right - left;

            output.push(format!(
                "Total:  {} {} {}",
                format_number(left),
                "→".cyan().bold(),
                format_number(right)
            ));

            if delta != 0 {
                output.push(String::new());
                if delta > 0 {
                    output.push(format!("Added:     {}", format_number(delta).green()));
                    output.push(format!("Removed:   {}", "0".red()));
                } else {
                    output.push(format!("Added:     {}", "0".green()));
                    output.push(format!("Removed:   {}", format_number(delta.abs()).red()));
                }
            }
        }

        output
    }

    /// Format schema diff content (returns lines for box)
    fn format_schema_diff_content(schema_diff: &SchemaDiff) -> Vec<String> {
        let mut output = Vec::new();

        // Show if schemas are identical
        if schema_diff.is_identical() {
            output.push(format!("{} Schemas are identical", "✓".green()));
            return output;
        }

        // Columns added
        if !schema_diff.columns_added.is_empty() {
            output.push(format!(
                "Added Columns ({})",
                schema_diff.columns_added.len()
            ));
            for col in &schema_diff.columns_added {
                let nullable_str = if col.nullable { "nullable" } else { "non-null" };
                output.push(format!(
                    "  {} {:<20} {:<12} {}",
                    "+".green(),
                    col.name,
                    col.data_type,
                    nullable_str.dimmed()
                ));
            }
            if !schema_diff.columns_removed.is_empty() || !schema_diff.columns_modified.is_empty() {
                output.push(String::new());
            }
        }

        // Columns removed
        if !schema_diff.columns_removed.is_empty() {
            output.push(format!(
                "Removed Columns ({})",
                schema_diff.columns_removed.len()
            ));
            for col in &schema_diff.columns_removed {
                output.push(format!(
                    "  {} {:<20} {}",
                    "-".red(),
                    col.name,
                    col.data_type.dimmed()
                ));
            }
            if !schema_diff.columns_modified.is_empty() {
                output.push(String::new());
            }
        }

        // Columns modified
        if !schema_diff.columns_modified.is_empty() {
            output.push(format!(
                "Modified Columns ({})",
                schema_diff.columns_modified.len()
            ));
            for col in &schema_diff.columns_modified {
                if let Some((old_type, new_type)) = &col.type_change {
                    let change_str = format!("{} {} {}", old_type, "→".cyan().bold(), new_type);
                    output.push(format!(
                        "  {} {:<20} {}",
                        "~".yellow(),
                        col.name,
                        change_str
                    ));
                }
                if let Some((_old_nullable, new_nullable)) = col.nullability_change {
                    let null_change = if new_nullable {
                        "non-null → nullable"
                    } else {
                        "nullable → non-null"
                    };
                    output.push(format!(
                        "  {} {:<20} {}",
                        "~".yellow(),
                        col.name,
                        null_change
                    ));
                }
            }
        }

        output
    }

    /// Format metadata diff content (returns lines for box)
    fn format_metadata_diff_content(meta_diff: &MetadataDiff) -> Vec<String> {
        let mut output = Vec::new();

        output.push(format!("{}", "File Properties".dimmed()));

        // Rows (always show if available)
        if let Some((left, right)) = meta_diff.num_rows
            && left != right
        {
            output.push(format!(
                "  Rows:         {} {} {}",
                format_number(left),
                "→".cyan().bold(),
                format_number(right)
            ));
        }

        // Sizes (always show if available)
        if let Some((left, right)) = meta_diff.compressed_size {
            if left != right {
                output.push(format!(
                    "  Size:         {} {} {}",
                    format_bytes(left),
                    "→".cyan().bold(),
                    format_bytes(right)
                ));
            } else {
                output.push(format!("  Size:         {}", format_bytes(left)));
            }
        }

        // Compression (always show if available)
        if let Some((left, right)) = &meta_diff.compression {
            if left != right {
                output.push(format!(
                    "  Compression:  {} {} {}",
                    left.to_uppercase(),
                    "→".cyan().bold(),
                    right.to_uppercase()
                ));
            } else if !left.is_empty() {
                output.push(format!("  Compression:  {}", left.to_uppercase()));
            }
        }

        // Format version (always show if available)
        if let Some((left, right)) = &meta_diff.format_version {
            if left != right {
                output.push(format!(
                    "  Version:      {} {} {}",
                    left,
                    "→".cyan().bold(),
                    right
                ));
            } else {
                output.push(format!("  Version:      {}", left));
            }
        }

        // Custom metadata changes
        if !meta_diff.custom_metadata.is_empty() {
            let total_changes = meta_diff.custom_metadata.added.len()
                + meta_diff.custom_metadata.removed.len()
                + meta_diff.custom_metadata.modified.len();

            output.push(String::new());
            output.push(format!("Custom Metadata ({} changes)", total_changes));

            for (key, value) in &meta_diff.custom_metadata.added {
                output.push(format!("  {} {} = {}", "+".green(), key, value));
            }

            for (key, value) in &meta_diff.custom_metadata.removed {
                output.push(format!("  {} {} = {}", "-".red(), key, value));
            }

            for (key, (old_val, new_val)) in &meta_diff.custom_metadata.modified {
                output.push(format!(
                    "  {} {}: {} → {}",
                    "~".yellow(),
                    key,
                    old_val,
                    new_val
                ));
            }
        }

        output
    }

    /// Format column stats content (returns lines for box)
    fn format_column_stats_content(stats_diff: &[ColumnStatsDiff]) -> Vec<String> {
        let mut output = Vec::new();

        for stat in stats_diff {
            if !output.is_empty() {
                output.push(String::new());
            }
            output.push(format!("{}", stat.name.bold()));

            let mut has_diff = false;

            // Null count
            if let Some((left, right)) = stat.null_count
                && left != right
            {
                has_diff = true;
                let change_str = format!(
                    "{} {} {}",
                    format_number(left),
                    "→".cyan().bold(),
                    format_number(right)
                )
                .yellow();
                output.push(format!("  Null count: {}", change_str));
            }

            // Distinct count
            if let Some((left, right)) = stat.distinct_count_approx
                && left != right
            {
                has_diff = true;
                let change_str = format!(
                    "{} {} {}",
                    format_number(left),
                    "→".cyan().bold(),
                    format_number(right)
                )
                .yellow();
                output.push(format!("  Distinct count (approx): {}", change_str));
            }

            // Min value
            if let Some((left, right)) = &stat.min_value
                && left != right
            {
                has_diff = true;
                let change_str = format!("{} {} {}", left, "→".cyan().bold(), right).yellow();
                output.push(format!("  Min: {}", change_str));
            }

            // Max value
            if let Some((left, right)) = &stat.max_value
                && left != right
            {
                has_diff = true;
                let change_str = format!("{} {} {}", left, "→".cyan().bold(), right).yellow();
                output.push(format!("  Max: {}", change_str));
            }

            // Mean
            if let Some((left, right)) = stat.mean
                && (right - left).abs() > 0.0001
            {
                has_diff = true;
                let diff = right - left;
                let diff_marker = if diff > 0.0 {
                    format!("(+{:.4})", diff).green()
                } else {
                    format!("({:.4})", diff).red()
                };
                let change_str = format!(
                    "{:.4} {} {:.4} {}",
                    left,
                    "→".cyan().bold(),
                    right,
                    diff_marker
                )
                .yellow();
                output.push(format!("  Mean: {}", change_str));
            }

            if !has_diff {
                output.push(format!("  {} No differences", "•".dimmed()));
            }
        }

        output
    }
}

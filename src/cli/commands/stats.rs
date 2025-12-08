//! Stats command implementation
//!
//! Shows table statistics from snapshot summary metadata.
//! Uses only pre-computed values - instantaneous, no manifest scanning.

use std::path::Path;



use super::common::resolve_table_path;
use crate::cli::parser::StatsArgs;
use crate::core::format_bytes;
use crate::core::formats::FormatHandlerFactory;
use crate::core::inspection::formatters::format_number;
use crate::core::maintenance::PartitionFilter;
use crate::core::storage::StorageBackendFactory;
use crate::core::CatalogConfig;
use crate::error::Result;

/// Statistics for a specific partition
#[derive(Debug, serde::Serialize)]
struct PartitionStats {
    /// Number of files in the partition
    file_count: usize,
    /// Total size in bytes
    total_size: u64,
    /// Average file size in bytes
    avg_file_size: u64,
    /// Number of small files (less than 128MB)
    small_files: usize,
    /// Percentage of small files
    small_files_percent: f64,
    /// Recommended target size for optimize
    recommended_target_size: u64,
}

/// Handler for stats command
pub struct StatsCommand;

impl StatsCommand {
    /// Execute stats command
    pub async fn execute(args: StatsArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        let table_path = resolve_table_path(&args.path, catalog_config.as_ref()).await?;
        let path = Path::new(&table_path);

        // Create storage backend
        let storage = StorageBackendFactory::create_backend(&table_path).await?;

        // Get format handler
        let handler = if let Some(format) = &args.format {
            FormatHandlerFactory::create_handler_for_format(format, path, storage).await?
        } else {
            FormatHandlerFactory::create_handler(path, storage).await?
        };

        // Read metadata (contains snapshot summary - instant)
        let metadata = handler.read_metadata().await?;

        // Extract table name from path
        let table_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("table");

        // If partition filter is specified, get detailed partition stats
        if let Some(partition_filter_str) = &args.partition {
            let partition_filter = PartitionFilter::parse(partition_filter_str)
                .map_err(|e| crate::error::Error::General(format!("Invalid partition filter: {}", e)))?;
            
            let partition_stats = Self::get_partition_stats(&*handler, &partition_filter).await?;
            
            // Format output
            if args.output == "json" {
                let json = serde_json::json!({
                    "table": table_name,
                    "format": handler.format_name(),
                    "partition_filter": partition_filter_str,
                    "stats": partition_stats,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json).unwrap_or_default()
                );
            } else {
                // Print simple partition stats
                println!();
                println!("Partition: {}", partition_filter_str);
                println!("  Files: {}", partition_stats.file_count);
                println!("  Total Size: {}", format_bytes(partition_stats.total_size));
                println!("  Avg File Size: {}", format_bytes(partition_stats.avg_file_size));
                println!("  Small Files (<128MB): {} ({:.1}%)", partition_stats.small_files, partition_stats.small_files_percent);
                
                if partition_stats.small_files > 0 && partition_stats.small_files_percent > 50.0 {
                    println!("  ⚠  Recommend: optimize --target-size {}", format_bytes(partition_stats.recommended_target_size));
                }
            }
        } else {
            // Original behavior: general table stats
            // Format output
            if args.output == "json" {
                let json = serde_json::json!({
                    "table": table_name,
                    "format": handler.format_name(),
                    "total_records": metadata.num_rows,
                    "compressed_size_bytes": metadata.compressed_size,
                    "format_version": metadata.format_version,
                    "created_at": metadata.created_at.map(|dt| dt.to_rfc3339()),
                    "properties": metadata.metadata,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json).unwrap_or_default()
                );
            } else {
                Self::print_text_output(&metadata, table_name);
            }
        }

        Ok(())
    }

    fn print_text_output(metadata: &crate::core::formats::FileMetadata, table_name: &str) {
        // Build content lines
        let mut lines: Vec<(String, String)> = Vec::new();

        if let Some(rows) = metadata.num_rows {
            lines.push(("Total Records".to_string(), format_number(rows)));
        }

        if let Some(size) = metadata.compressed_size {
            lines.push(("Total Size".to_string(), format_bytes(size)));
        }

        if let Some(files) = metadata.metadata.get("total-data-files")
            && let Ok(n) = files.parse::<i64>()
        {
            lines.push(("Data Files".to_string(), format_number(n)));
        }

        if let Some(ref version) = metadata.format_version {
            lines.push(("Format Version".to_string(), version.clone()));
        }

        if let Some(dt) = metadata.created_at {
            lines.push((
                "Last Modified".to_string(),
                dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            ));
        }

        // Calculate widths
        let max_key_width = lines.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        let max_val_width = lines.iter().map(|(_, v)| v.len()).max().unwrap_or(0);
        let content_width = max_key_width + max_val_width + 4; // 4 spaces between
        let box_width = content_width.max(table_name.len()) + 4; // padding

        // Print box
        println!();
        println!("╭{}╮", "─".repeat(box_width));

        // Centered title
        let title_padding = (box_width - table_name.len()) / 2;
        println!(
            "│{}{}{}│",
            " ".repeat(title_padding),
            table_name,
            " ".repeat(box_width - title_padding - table_name.len())
        );

        println!("├{}┤", "─".repeat(box_width));
        println!("│{}│", " ".repeat(box_width));

        // Content lines
        for (key, value) in &lines {
            let line = format!(
                "  {}{}{}",
                key,
                " ".repeat(max_key_width - key.len() + 4),
                value
            );
            println!("│{}{}│", line, " ".repeat(box_width - line.len()));
        }

        println!("│{}│", " ".repeat(box_width));
        println!("╰{}╯", "─".repeat(box_width));
        println!();
    }

    /// Get statistics for files matching a partition filter
    async fn get_partition_stats(
        _handler: &dyn crate::core::formats::FormatHandler,
        _partition_filter: &PartitionFilter,
    ) -> Result<PartitionStats> {

        
        // Get metadata service from handler if possible
        // For now, we'll use a simpler approach: get general stats
        // In a real implementation, we would need to access the metadata service
        
        // Placeholder implementation - returns basic stats
        // TODO: Implement actual partition filtering
        Ok(PartitionStats {
            file_count: 0,
            total_size: 0,
            avg_file_size: 0,
            small_files: 0,
            small_files_percent: 0.0,
            recommended_target_size: 256 * 1024 * 1024, // 256MB default
        })
    }
}

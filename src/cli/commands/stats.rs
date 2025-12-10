//! Stats command implementation
//!
//! Shows table statistics from snapshot summary metadata.
//! Uses only pre-computed values - instantaneous, no manifest scanning.

use std::path::Path;

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::common::{create_table, extract_table_name, print_json, resolve_table_path};
use crate::cli::parser::StatsArgs;
use crate::core::analysis::get_partition_stats;
use crate::core::CatalogConfig;
use crate::core::format_bytes;
use crate::core::formats::FormatHandlerFactory;
use crate::core::inspection::formatters::format_number;
use crate::core::maintenance::PartitionFilter;
use crate::core::storage::create_object_store;
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for stats command
pub struct StatsCommand;

impl StatsCommand {
    /// Execute stats command
    pub async fn execute(args: StatsArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 64 * 1024 * 1024; // 64MB for stats
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args, catalog_config)).await
    }

    async fn execute_inner(args: StatsArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        let table_path = resolve_table_path(&args.path, catalog_config.as_ref()).await?;
        let path = Path::new(&table_path);

        // Create storage backend
        let storage = create_object_store(&table_path).await?;

        // Get format handler
        let handler = if let Some(format) = &args.format {
            FormatHandlerFactory::create_handler_for_format(format, path, storage).await?
        } else {
            FormatHandlerFactory::create_handler(path, storage).await?
        };

        // Read metadata (contains snapshot summary - instant)
        let metadata = handler.read_metadata().await?;

        // Extract table name from path
        let table_name = extract_table_name(&table_path);

        // If partition filter is specified, get detailed partition stats
        if let Some(partition_filter_str) = &args.partition {
            let partition_filter = PartitionFilter::parse(partition_filter_str).map_err(|e| {
                crate::error::Error::General(format!("Invalid partition filter: {}", e))
            })?;

            let partition_stats = get_partition_stats(&table_path, &partition_filter).await?;

            // Format output
            if args.output == "json" {
                let json = serde_json::json!({
                    "table": table_name,
                    "format": handler.format_name(),
                    "partition_filter": partition_filter_str,
                    "stats": partition_stats,
                });
                print_json(&json)?;
            } else {
                // Print simple partition stats
                println!();
                println!("Partition: {}", partition_filter_str);
                println!("  Files: {}", partition_stats.file_count);
                println!("  Total Size: {}", format_bytes(partition_stats.total_size));
                println!(
                    "  Avg File Size: {}",
                    format_bytes(partition_stats.avg_file_size)
                );
                println!(
                    "  Small Files (<128MB): {} ({:.1}%)",
                    partition_stats.small_files, partition_stats.small_files_percent
                );

                if partition_stats.small_files > 0 && partition_stats.small_files_percent > 50.0 {
                    println!(
                        "  ⚠  Recommend: optimize --target-size {}",
                        format_bytes(partition_stats.recommended_target_size)
                    );
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
                print_json(&json)?;
            } else {
                Self::print_text_output(&metadata, table_name);
            }
        }

        Ok(())
    }

    fn print_text_output(metadata: &crate::core::formats::FileMetadata, table_name: &str) {
        // Print table title
        println!("{}", table_name.cyan().bold());
        println!();

        // Build table with stats
        let mut table = create_table();

        table.set_header(vec![
            Cell::new("Metric".cyan().to_string()).set_alignment(CellAlignment::Left),
            Cell::new("Value".cyan().to_string()).set_alignment(CellAlignment::Right),
        ]);

        if let Some(rows) = metadata.num_rows {
            table.add_row(vec![
                Cell::new("Total Records").set_alignment(CellAlignment::Left),
                Cell::new(format_number(rows)).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(size) = metadata.compressed_size {
            table.add_row(vec![
                Cell::new("Total Size").set_alignment(CellAlignment::Left),
                Cell::new(format_bytes(size)).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(files) = metadata.metadata.get("total-data-files")
            && let Ok(n) = files.parse::<i64>()
        {
            table.add_row(vec![
                Cell::new("Data Files").set_alignment(CellAlignment::Left),
                Cell::new(format_number(n)).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(ref version) = metadata.format_version {
            table.add_row(vec![
                Cell::new("Format Version").set_alignment(CellAlignment::Left),
                Cell::new(version).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(dt) = metadata.created_at {
            table.add_row(vec![
                Cell::new("Last Modified").set_alignment(CellAlignment::Left),
                Cell::new(dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()).set_alignment(CellAlignment::Right),
            ]);
        }

        println!("{}", table);
    }
}

//! Stats command implementation
//!
//! Shows table statistics from snapshot summary metadata.
//! Thin wrapper that delegates to StatsService in core.

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::common::{print_json, resolve_table_path};
use crate::cli::output::{create_styled_table, format_datetime_utc};
use crate::cli::parser::StatsArgs;
use crate::core::{format_bytes, format_number};
use crate::core::operations::{PartitionStats, StatsConfig, StatsResult, StatsService, TableStats};
use crate::core::CatalogConfig;
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
        // 1. Resolve table path
        let table_path = resolve_table_path(&args.path, catalog_config.as_ref()).await?;

        // 2. Build config and delegate to service
        let config = StatsConfig {
            partition: args.partition.clone(),
        };

        let result = StatsService::get_stats(&table_path, &config).await?;

        // 3. Output
        Self::output(&result, &args)
    }

    /// Output stats in the requested format
    fn output(result: &StatsResult, args: &StatsArgs) -> Result<()> {
        match result {
            StatsResult::Table(stats) => {
                if args.output == "json" {
                    Self::output_table_json(stats)
                } else {
                    Self::output_table_text(stats)
                }
            }
            StatsResult::Partition(stats) => {
                if args.output == "json" {
                    Self::output_partition_json(stats)
                } else {
                    Self::output_partition_text(stats)
                }
            }
        }
    }

    /// Output table stats as JSON
    fn output_table_json(stats: &TableStats) -> Result<()> {
        let json = serde_json::json!({
            "table": stats.table_name,
            "format": stats.format,
            "total_records": stats.total_records,
            "compressed_size_bytes": stats.compressed_size,
            "format_version": stats.format_version,
            "created_at": stats.last_modified.map(|dt| dt.to_rfc3339()),
            "properties": stats.properties,
        });
        print_json(&json)
    }

    /// Output table stats as text
    fn output_table_text(stats: &TableStats) -> Result<()> {
        println!("{}", stats.table_name.cyan().bold());
        println!();

        let mut table = create_styled_table();
        table.set_header(vec![
            Cell::new("Metric".cyan().to_string()).set_alignment(CellAlignment::Left),
            Cell::new("Value".cyan().to_string()).set_alignment(CellAlignment::Right),
        ]);

        if let Some(rows) = stats.total_records {
            table.add_row(vec![
                Cell::new("Total Records").set_alignment(CellAlignment::Left),
                Cell::new(format_number(rows)).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(size) = stats.compressed_size {
            table.add_row(vec![
                Cell::new("Total Size").set_alignment(CellAlignment::Left),
                Cell::new(format_bytes(size)).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(files) = stats.properties.get("total-data-files")
            && let Ok(n) = files.parse::<i64>()
        {
            table.add_row(vec![
                Cell::new("Data Files").set_alignment(CellAlignment::Left),
                Cell::new(format_number(n)).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(ref version) = stats.format_version {
            table.add_row(vec![
                Cell::new("Format Version").set_alignment(CellAlignment::Left),
                Cell::new(version).set_alignment(CellAlignment::Right),
            ]);
        }

        if let Some(ref dt) = stats.last_modified {
            table.add_row(vec![
                Cell::new("Last Modified").set_alignment(CellAlignment::Left),
                Cell::new(format_datetime_utc(dt)).set_alignment(CellAlignment::Right),
            ]);
        }

        println!("{}", table);
        Ok(())
    }

    /// Output partition stats as JSON
    fn output_partition_json(stats: &PartitionStats) -> Result<()> {
        let json = serde_json::json!({
            "table": stats.table_name,
            "format": stats.format,
            "partition_filter": stats.partition_filter,
            "stats": {
                "file_count": stats.file_count,
                "total_size": stats.total_size,
                "avg_file_size": stats.avg_file_size,
                "small_files": stats.small_files,
                "small_files_percent": stats.small_files_percent,
                "recommended_target_size": stats.recommended_target_size,
            }
        });
        print_json(&json)
    }

    /// Output partition stats as text
    fn output_partition_text(stats: &PartitionStats) -> Result<()> {
        println!();
        println!("Partition: {}", stats.partition_filter);
        println!("  Files: {}", stats.file_count);
        println!("  Total Size: {}", format_bytes(stats.total_size));
        println!("  Avg File Size: {}", format_bytes(stats.avg_file_size));
        println!(
            "  Small Files (<128MB): {} ({:.1}%)",
            stats.small_files, stats.small_files_percent
        );

        if stats.small_files > 0 && stats.small_files_percent > 50.0 {
            println!(
                "  {}  Recommend: optimize --target-size {}",
                "⚠".yellow(),
                format_bytes(stats.recommended_target_size)
            );
        }

        Ok(())
    }
}

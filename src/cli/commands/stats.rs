//! Stats command implementation
//!
//! Shows table statistics from snapshot summary metadata.
//! Thin wrapper that delegates to StatsService in core.

use super::common::resolve_table_from_context;
use crate::cli::output::StatsFormatter;
use crate::cli::parser::{CatalogContext, StatsArgs};
use crate::core::operations::{StatsConfig, StatsResult, StatsService};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for stats command
pub struct StatsCommand;

impl StatsCommand {
    /// Execute stats command
    pub async fn execute(args: StatsArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_MEDIUM_OPS;
        with_resource_limits(MEMORY_MEDIUM_OPS, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: StatsArgs, ctx: &CatalogContext) -> Result<()> {
        // 1. Resolve table (supports catalog resolution)
        let resolution = resolve_table_from_context(ctx).await?;
        let table_path = resolution.location();

        // 2. Build config and delegate to service
        let config = StatsConfig {
            partition: args.partition.clone(),
        };

        let result = StatsService::get_stats(&table_path, &config).await?;

        // 3. Output
        Self::output(&result, &args)
    }

    /// Output stats in the requested format using StatsFormatter
    fn output(result: &StatsResult, args: &StatsArgs) -> Result<()> {
        match result {
            StatsResult::Table(stats) => {
                if args.output == "json" {
                    let json_str = StatsFormatter::format_table_stats_json(stats).map_err(|e| {
                        Error::Serialization {
                            message: e.to_string(),
                        }
                    })?;
                    println!("{}", json_str);
                } else {
                    println!("{}", StatsFormatter::format_table_stats_text(stats));
                }
            }
            StatsResult::Partition(stats) => {
                if args.output == "json" {
                    let json_str =
                        StatsFormatter::format_partition_stats_json(stats).map_err(|e| {
                            Error::Serialization {
                                message: e.to_string(),
                            }
                        })?;
                    println!("{}", json_str);
                } else {
                    println!("{}", StatsFormatter::format_partition_stats_text(stats));
                }
            }
        }
        Ok(())
    }
}

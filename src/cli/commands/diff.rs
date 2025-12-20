//! Diff command implementation
//!
//! Compares snapshots, branches, or tags within a table.
//! Thin wrapper that delegates to DiffService in core.

use super::common::resolve_table_from_context;
use crate::cli::output::DiffFormatter;
use crate::cli::parser::{CatalogContext, DiffArgs};
use crate::core::operations::{DiffConfig, DiffService, SnapshotDiffResult};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for diff command
pub struct DiffCommand;

impl DiffCommand {
    /// Execute diff command
    pub async fn execute(args: DiffArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_HEAVY_OPS;
        with_resource_limits(MEMORY_HEAVY_OPS, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: DiffArgs, ctx: &CatalogContext) -> Result<()> {
        // 1. Resolve table (supports catalog resolution) and create metadata service
        let resolution = resolve_table_from_context(ctx).await?;
        let service = resolution.to_readonly_service().await?;

        // 2. Build config and delegate to service
        let config = DiffConfig {
            from: args.from.clone(),
            to: args.to.clone(),
        };

        let result = DiffService::compare_snapshots(&service, &config).await?;

        // 3. Output
        Self::output(&result, &args)
    }

    /// Output diff result in the requested format using DiffFormatter
    fn output(result: &SnapshotDiffResult, args: &DiffArgs) -> Result<()> {
        if args.output == "json" {
            let json_str =
                DiffFormatter::format_diff_json(result).map_err(|e| Error::Serialization {
                    message: e.to_string(),
                })?;
            println!("{}", json_str);
        } else {
            let from_label = args.from.as_deref().unwrap_or("parent");
            let to_label = args.to.as_deref().unwrap_or("current");
            println!(
                "{}",
                DiffFormatter::format_diff_text(result, from_label, to_label)
            );
        }
        Ok(())
    }
}

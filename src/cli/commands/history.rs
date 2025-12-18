//! History command implementation
//!
//! Shows version history for Iceberg tables.
//! Thin wrapper that delegates to HistoryService in core.

use super::common::resolve_table_from_context;
use crate::cli::output::{HistoryEntryInfo, HistoryFormatter};
use crate::cli::parser::{CatalogContext, HistoryArgs};
use crate::core::operations::{HistoryConfig, HistoryService};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for history command
pub struct HistoryCommand;

impl HistoryCommand {
    /// Execute history command
    pub async fn execute(args: HistoryArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_MEDIUM_OPS;
        with_resource_limits(MEMORY_MEDIUM_OPS, Self::execute_inner(args, ctx)).await
    }

    async fn execute_inner(args: HistoryArgs, ctx: &CatalogContext) -> Result<()> {
        // 1. Resolve table (supports catalog resolution)
        let resolution = resolve_table_from_context(ctx).await?;

        // 2. Get table using factory method - handles catalog vs path context automatically
        let table = resolution.to_table().await?;

        // 3. Build config and delegate to service
        let config = HistoryConfig {
            limit: Some(args.limit),
            all: args.all,
        };

        let entries = HistoryService::get_history(&table, &config)?;

        // 4. Convert to formatter types
        let entry_infos: Vec<HistoryEntryInfo> =
            entries.iter().map(HistoryEntryInfo::from_core).collect();

        // 5. Output using formatter
        if args.output == "json" {
            let json_str =
                HistoryFormatter::format_json(&entry_infos).map_err(|e| Error::Serialization {
                    message: e.to_string(),
                })?;
            println!("{}", json_str);
        } else {
            println!("{}", HistoryFormatter::format_table(&entry_infos));
        }

        Ok(())
    }
}

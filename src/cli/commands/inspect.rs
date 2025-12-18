//! Inspect command implementation
//!
//! Shows detailed information about table structure, metadata, and statistics.
//! Uses IcebergTableInspector from core::operations for the actual inspection logic.

use super::common::{print_json, resolve_table_from_context};
use crate::cli::output::InspectionFormatter;
use crate::cli::parser::{CatalogContext, InspectArgs};
use crate::core::operations::inspect::{IcebergInspectOptions, IcebergTableInspector};
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for inspect command
pub struct InspectCommand;

impl InspectCommand {
    /// Execute inspect command
    pub async fn execute(args: InspectArgs, ctx: &CatalogContext) -> Result<()> {
        use super::constants::MEMORY_HEAVY_OPS;
        with_resource_limits(MEMORY_HEAVY_OPS, Self::inspect_inner(args, ctx)).await
    }

    async fn inspect_inner(args: InspectArgs, ctx: &CatalogContext) -> Result<()> {
        // Resolve table - get catalog table directly when using catalog
        let resolution = resolve_table_from_context(ctx).await?;

        // Get table using factory method - handles catalog vs path context automatically
        let table = resolution.to_table().await?;

        // Build inspection options from CLI args
        let options = IcebergInspectOptions::from_cli(args.verbose);

        // Execute inspection using the core operation
        let result = IcebergTableInspector::inspect(&table, &options)?;

        // Format and display result based on output format
        if args.output == "json" {
            print_json(&result)?;
        } else {
            let output = InspectionFormatter::format_iceberg_table(&result, &options);
            println!("{}", output);
        }

        Ok(())
    }
}

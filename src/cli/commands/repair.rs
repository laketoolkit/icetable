//! Repair command implementation
//!
//! Thin wrapper that delegates to RepairService for Iceberg tables.

use super::common::{TableResolution, confirm_destructive, resolve_table_from_context};
use crate::cli::output::RepairFormatter;
use crate::cli::parser::{CatalogContext, RepairArgs};
use crate::core::CatalogConfig;
use crate::core::maintenance::{MaintenanceConfig, RepairService};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Repair options specifying what actions to take
#[derive(Debug, Clone, Copy)]
pub struct RepairOptions {
    /// Whether to add orphan files to metadata
    pub add_orphans: bool,
    /// Whether to remove missing file references
    pub remove_missing: bool,
}

/// Handler for repair command
pub struct RepairCommand;

impl RepairCommand {
    /// Execute repair command
    pub async fn execute(args: RepairArgs, ctx: &CatalogContext) -> Result<()> {
        let resolution = resolve_table_from_context(ctx).await?;
        let table_path = resolution.location();

        use super::constants::MEMORY_INTENSIVE_OPS;
        with_resource_limits(
            MEMORY_INTENSIVE_OPS,
            Self::repair_inner(table_path, args, &resolution, ctx.catalog_config.as_ref()),
        )
        .await
    }

    async fn repair_inner(
        table_path: String,
        args: RepairArgs,
        resolution: &TableResolution,
        cli_catalog: Option<&crate::core::CatalogConfig>,
    ) -> Result<()> {
        // Validate at least one repair option is specified
        if !args.all && !args.prune && !args.add_orphans {
            return Err(Error::MissingArgument {
                argument: "repair option".to_string(),
                description: "Must specify at least one of: --all, --prune, or --add-orphans".to_string(),
            });
        }

        // Determine repair options
        let options = RepairOptions {
            add_orphans: args.add_orphans || args.all,
            remove_missing: args.prune || args.all,
        };

        // Create service configuration
        let config = MaintenanceConfig {
            dry_run: args.dry_run,
            ..Default::default()
        };

        let service = RepairService::with_config(config);

        Self::repair_iceberg(
            &args,
            &service,
            options,
            &table_path,
            resolution,
            cli_catalog,
        )
        .await
    }

    /// Repair Iceberg table
    async fn repair_iceberg(
        args: &RepairArgs,
        service: &RepairService,
        options: RepairOptions,
        table_path: &str,
        resolution: &TableResolution,
        cli_catalog: Option<&CatalogConfig>,
    ) -> Result<()> {
        println!(
            "{}",
            RepairFormatter::format_analysis_header(table_path, args.dry_run)
        );

        // Create metadata service using factory method - handles catalog vs path context automatically
        let metadata_service = resolution.to_writable_service(cli_catalog, None).await?;

        // First analyze to show what will be done
        let analysis = service.analyze(&metadata_service).await?;
        println!("{}", RepairFormatter::format_analysis_summary(&analysis));

        if !analysis.has_issues() {
            println!("{}", RepairFormatter::format_healthy());
            return Ok(());
        }

        // Check if any selected options have issues to fix
        let has_work = (options.add_orphans && !analysis.orphan_files.is_empty())
            || (options.remove_missing && !analysis.missing_files.is_empty());

        if args.dry_run {
            // Dry-run: show what WILL be fixed
            println!(
                "{}",
                RepairFormatter::format_issues_found(&analysis, options.add_orphans, options.remove_missing, false)
            );
            println!(
                "{}",
                RepairFormatter::format_dry_run_details(&analysis, options.add_orphans, options.remove_missing)
            );
            return Ok(());
        }

        if !has_work {
            println!(
                "{}",
                RepairFormatter::format_issues_found(&analysis, options.add_orphans, options.remove_missing, false)
            );
            println!("{}", RepairFormatter::format_no_work());
            return Ok(());
        }

        // Confirm before destructive operation
        if !confirm_destructive(
            "This will modify table metadata.",
            args.force,
            args.dry_run,
        ) {
            return Ok(());
        }

        // Execute the repair
        let result = service.execute(&metadata_service).await?;

        // Show issues with [fixed] / [skipped] after completion
        println!(
            "{}",
            RepairFormatter::format_issues_found(&analysis, options.add_orphans, options.remove_missing, true)
        );
        println!("{}", RepairFormatter::format_result(&result));

        Ok(())
    }

}

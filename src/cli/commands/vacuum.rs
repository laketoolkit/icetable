//! Vacuum command implementation
//!
//! Thin wrapper that delegates to VacuumService in core.

use colored::Colorize;
use std::io;

use super::common::{TableResolution, create_spinner, resolve_table_from_context};
use crate::cli::output::{OrphanFileInfo, VacuumFormatter};
use crate::cli::parser::{CatalogContext, VacuumArgs};
use crate::core::maintenance::{VacuumConfig, VacuumResult, VacuumService};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for vacuum command
pub struct VacuumCommand;

impl VacuumCommand {
    /// Execute vacuum command
    pub async fn execute(args: VacuumArgs, ctx: &CatalogContext) -> Result<()> {
        let resolution = resolve_table_from_context(ctx).await?;
        let table_path = resolution.location().to_string();

        use super::constants::MEMORY_INTENSIVE_OPS;
        with_resource_limits(
            MEMORY_INTENSIVE_OPS,
            Self::vacuum_iceberg(&table_path, &args, &resolution),
        )
        .await
    }

    /// Vacuum Iceberg table using VacuumService
    async fn vacuum_iceberg(
        table_path: &str,
        args: &VacuumArgs,
        resolution: &TableResolution,
    ) -> Result<()> {
        // Print header
        println!(
            "{}",
            VacuumFormatter::format_header(table_path, args.dry_run, args.branch.as_deref())
        );

        // Handle confirmation for destructive operations
        if !Self::confirm_operation(args)? {
            return Ok(());
        }

        // Create service with config
        let config = VacuumConfig {
            retention_hours: args.retention_hours,
            dry_run: args.dry_run,
            parallelism: 32,
        };
        let vacuum_service = VacuumService::with_config(config);

        // Show progress while analyzing
        let pb = create_spinner("Scanning manifests");

        // Create metadata service using factory method - handles catalog vs path context automatically
        let metadata_service = resolution.to_readonly_service().await?;

        // Execute vacuum (analyze + optionally delete)
        let result = vacuum_service.execute(&metadata_service).await?;

        pb.finish_and_clear();

        // Output results
        Self::output_result(&result, args)
    }

    /// Handle confirmation for destructive operations
    fn confirm_operation(args: &VacuumArgs) -> Result<bool> {
        // Safety warning for vacuum without --dry-run or --force
        if !args.dry_run && !args.force {
            println!();
            println!(
                "{} {}",
                "⚠".yellow(),
                "This will permanently delete orphan files.".yellow().bold()
            );
            println!();

            // Ask for confirmation
            print!("Continue? [y/N] ");
            io::Write::flush(&mut io::stdout()).ok();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err()
                || !matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
            {
                println!("{}", "Cancelled.".dimmed());
                return Ok(false);
            }
            println!();
        }

        // Warning for low retention period
        if args.retention_hours < 24 && !args.force {
            println!(
                "{} {} {}",
                "⚠".yellow(),
                "Retention period is less than 24 hours.".yellow(),
                format!(
                    "Files newer than {} hours will be kept.",
                    args.retention_hours
                )
                .dimmed()
            );
            println!();
        }

        Ok(true)
    }

    /// Output results based on format
    fn output_result(result: &VacuumResult, args: &VacuumArgs) -> Result<()> {
        let analysis = &result.analysis;

        // Output analysis summary using formatter
        println!(
            "{}",
            VacuumFormatter::format_summary(
                analysis.referenced_count,
                analysis.orphan_files.len(),
                analysis.orphan_bytes,
                analysis.retention_hours,
            )
        );

        if analysis.orphan_files.is_empty() {
            println!("{}", VacuumFormatter::format_no_orphans());
            return Ok(());
        }

        // Convert to formatter types
        let orphan_files: Vec<OrphanFileInfo> = analysis
            .orphan_files
            .iter()
            .map(|f| OrphanFileInfo {
                path: f.path.clone(),
                size: f.size,
            })
            .collect();

        if result.dry_run {
            Self::output_dry_run(
                &orphan_files,
                analysis.orphan_bytes,
                analysis.retention_hours,
                args,
            )
        } else {
            Self::output_execution(result, args)
        }
    }

    /// Output dry-run results
    fn output_dry_run(
        files: &[OrphanFileInfo],
        total_bytes: u64,
        retention_hours: u64,
        args: &VacuumArgs,
    ) -> Result<()> {
        if args.output == "json" {
            let json_str =
                VacuumFormatter::format_dry_run_json(files, total_bytes, retention_hours).map_err(
                    |e| Error::Serialization {
                        message: e.to_string(),
                    },
                )?;
            println!("{}", json_str);
        } else {
            println!(
                "{}",
                VacuumFormatter::format_dry_run_table(files, total_bytes)
            );
        }
        Ok(())
    }

    /// Output execution results
    fn output_execution(result: &VacuumResult, args: &VacuumArgs) -> Result<()> {
        if args.output == "json" {
            let json_str = VacuumFormatter::format_execution_json(
                result.deleted_count,
                result.deleted_bytes,
                result.errors.len(),
            )
            .map_err(|e| Error::Serialization {
                message: e.to_string(),
            })?;
            println!("{}", json_str);
        } else {
            println!(
                "{}",
                VacuumFormatter::format_execution_table(
                    result.deleted_count,
                    result.deleted_bytes,
                    &result.errors,
                )
            );
        }
        Ok(())
    }
}

//! Vacuum command implementation
//!
//! Thin wrapper that delegates to VacuumService in core.

use colored::Colorize;
use std::io;

use super::common::{create_spinner, print_json, resolve_table_path};
use crate::cli::parser::VacuumArgs;
use crate::core::CatalogConfig;
use crate::core::format_bytes;
use crate::core::maintenance::{VacuumConfig, VacuumResult, VacuumService};
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for vacuum command
pub struct VacuumCommand;

impl VacuumCommand {
    /// Execute vacuum command
    pub async fn execute(args: VacuumArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        let table_path = resolve_table_path(&args.path, catalog_config.as_ref()).await?;

        // Apply resource limits (timeout, cancellation, memory tracking)
        const ESTIMATED_MEMORY: u64 = 256 * 1024 * 1024; // 256MB for manifest scanning
        with_resource_limits(ESTIMATED_MEMORY, Self::vacuum_iceberg(&table_path, &args)).await
    }

    /// Vacuum Iceberg table using VacuumService
    async fn vacuum_iceberg(table_path: &str, args: &VacuumArgs) -> Result<()> {
        // Print header
        Self::print_header(table_path, args);

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
        let service = VacuumService::with_config(config);

        // Show progress while analyzing
        let pb = create_spinner("Scanning manifests");

        // Execute vacuum (analyze + optionally delete)
        let result = service.execute(table_path).await?;

        pb.finish_and_clear();

        // Output results
        Self::output_result(&result, args)
    }

    /// Print header message
    fn print_header(table_path: &str, args: &VacuumArgs) {
        let action = if args.dry_run { "Analyzing" } else { "Vacuuming" };

        if let Some(ref branch) = args.branch {
            println!(
                "{} Iceberg table at {} (branch: {})",
                action,
                table_path,
                branch.cyan()
            );
            println!(
                "{}",
                "Note: Vacuum always considers all snapshots for safety".dimmed()
            );
        } else {
            println!("{} Iceberg table at {}", action, table_path);
        }
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
                format!("Files newer than {} hours will be kept.", args.retention_hours).dimmed()
            );
            println!();
        }

        Ok(true)
    }

    /// Output results based on format
    fn output_result(result: &VacuumResult, args: &VacuumArgs) -> Result<()> {
        let analysis = &result.analysis;

        // Output analysis summary
        println!();
        println!(
            "Referenced files: {}",
            analysis.referenced_count.to_string().cyan()
        );
        println!(
            "Files to delete:  {} ({})",
            analysis.orphan_files.len().to_string().cyan(),
            format_bytes(analysis.orphan_bytes)
        );
        println!(
            "Retention:        {} hours",
            analysis.retention_hours.to_string().cyan()
        );

        if analysis.orphan_files.is_empty() {
            println!();
            println!("{}", "No orphan files to delete".yellow());
            return Ok(());
        }

        if result.dry_run {
            Self::output_dry_run(result, args)
        } else {
            Self::output_execution(result, args)
        }
    }

    /// Output dry-run results
    fn output_dry_run(result: &VacuumResult, args: &VacuumArgs) -> Result<()> {
        let analysis = &result.analysis;

        println!();
        println!("{}", "DRY RUN - No files will be deleted".yellow().bold());

        if args.output == "json" {
            let files: Vec<&str> = analysis
                .orphan_files
                .iter()
                .map(|f| f.path.as_str())
                .collect();
            let json = serde_json::json!({
                "dry_run": true,
                "files_to_delete": files,
                "files_count": analysis.orphan_files.len(),
                "bytes_to_free": analysis.orphan_bytes,
                "retention_hours": analysis.retention_hours,
            });
            print_json(&json)?;
        } else {
            println!();
            println!("{}", "Would delete the following files:".cyan());

            // Show first 10 files, then summary if more
            let show_count = 10;
            for file in analysis.orphan_files.iter().take(show_count) {
                let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                println!("  - {} ({})", name, format_bytes(file.size));
            }

            if analysis.orphan_files.len() > show_count {
                println!(
                    "  {} {} more files...",
                    "...and".dimmed(),
                    (analysis.orphan_files.len() - show_count)
                        .to_string()
                        .dimmed()
                );
            }

            println!();
            println!(
                "Total: {} files, {} to free",
                analysis.orphan_files.len().to_string().yellow(),
                format_bytes(analysis.orphan_bytes).yellow()
            );
            println!();
            println!(
                "{}",
                "Run without --dry-run to delete these files.".dimmed()
            );
        }

        Ok(())
    }

    /// Output execution results
    fn output_execution(result: &VacuumResult, args: &VacuumArgs) -> Result<()> {
        if args.output == "json" {
            let json = serde_json::json!({
                "files_deleted": result.deleted_count,
                "bytes_freed": result.deleted_bytes,
                "errors": result.errors.len(),
            });
            print_json(&json)?;
        } else {
            println!();
            println!(
                "{} {} files, freed {}",
                "Deleted".green().bold(),
                result.deleted_count,
                format_bytes(result.deleted_bytes)
            );
            if !result.errors.is_empty() {
                println!("{} errors occurred:", result.errors.len().to_string().red());
                for err in result.errors.iter().take(5) {
                    println!("  - {}", err);
                }
                if result.errors.len() > 5 {
                    println!("  ... and {} more", result.errors.len() - 5);
                }
            }
        }

        Ok(())
    }
}

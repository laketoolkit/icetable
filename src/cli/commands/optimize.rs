//! Optimize command implementation
//!
//! Thin wrapper that delegates to core services.
//!
//! Subcommands:
//! - `data`: Compact small data files into larger ones
//! - `manifests`: Rewrite and compact manifest files

use colored::Colorize;

use super::common::{create_committer, print_dry_run_header, print_json, resolve_table, TableResolution};
use crate::cli::parser::{OptimizeCommands, OptimizeDataArgs, OptimizeManifestsArgs};
use crate::core::catalog::TableCommitter;
use crate::core::maintenance::{
    MaintenanceConfig, ManifestConfig, ManifestService, OptimizeService,
};
use crate::core::metadata::MaintenanceResult;
use crate::utils::core::parse_bytes;
use crate::core::{CatalogConfig, format_bytes};
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for optimize command
pub struct OptimizeCommand;

impl OptimizeCommand {
    /// Execute optimize command
    pub async fn execute(
        cmd: OptimizeCommands,
        catalog_config: Option<CatalogConfig>,
    ) -> Result<()> {
        match cmd {
            OptimizeCommands::Data(args) => Self::execute_data(args, catalog_config).await,
            OptimizeCommands::Manifests(args) => {
                Self::execute_manifests(args, catalog_config).await
            }
        }
    }

    /// Execute optimize data subcommand
    async fn execute_data(
        args: OptimizeDataArgs,
        catalog_config: Option<CatalogConfig>,
    ) -> Result<()> {
        let resolution = resolve_table(&args.path, catalog_config.as_ref()).await?;
        let table_path = resolution.location().to_string();

        let max_bytes = args
            .max_bytes
            .as_ref()
            .map(|s| parse_bytes(s))
            .transpose()
            .map_err(Error::General)?;

        let config = MaintenanceConfig {
            target_size: args.target_size,
            min_size: args.min_file_size.unwrap_or(args.target_size / 16),
            dry_run: args.dry_run,
            parallelism: args.max_concurrent_tasks,
            partition_filter: args.partition.clone(),
            max_files: args.max_files,
            max_bytes,
            ..Default::default()
        };

        let service = OptimizeService::with_config(config);

        // Apply resource limits (timeout, cancellation, memory tracking)
        let estimated_memory = args.target_size * args.max_concurrent_tasks as u64;
        let result = with_resource_limits(
            estimated_memory,
            Self::optimize_iceberg_data(&table_path, &service, args.branch.as_deref(), &resolution, catalog_config.as_ref()),
        )
        .await?;

        Self::output_data_result(&result, &args.output)?;
        Ok(())
    }

    /// Execute optimize manifests subcommand
    async fn execute_manifests(
        args: OptimizeManifestsArgs,
        catalog_config: Option<CatalogConfig>,
    ) -> Result<()> {
        let resolution = resolve_table(&args.path, catalog_config.as_ref()).await?;
        let table_path = resolution.location().to_string();

        let committer = create_committer(catalog_config.as_ref(), &resolution);

        // Apply resource limits (timeout, cancellation, memory tracking)
        let estimated_memory = args.target_size * 2;
        with_resource_limits(
            estimated_memory,
            Self::rewrite_iceberg_manifests(&table_path, &args, committer),
        )
        .await
    }

    /// Optimize Iceberg data files
    async fn optimize_iceberg_data(
        table_path: &str,
        service: &OptimizeService,
        branch: Option<&str>,
        resolution: &TableResolution,
        cli_catalog: Option<&CatalogConfig>,
    ) -> Result<MaintenanceResult> {
        use crate::core::metadata::IcebergMetadataService;

        if let Some(b) = branch {
            println!(
                "{} Iceberg table at {} (branch: {})",
                "Optimizing".green(),
                table_path,
                b.cyan()
            );
        } else {
            println!("{} Iceberg table at {}", "Optimizing".green(), table_path);
        }

        // For write operations (non-dry-run), we need a catalog
        // Read-only operations can use storage directly
        let metadata_service = match resolution {
            TableResolution::CatalogTable { table, namespace, name, catalog_config } => {
                // Use catalog table's metadata for proper UUID/snapshot consistency
                let config = cli_catalog.unwrap_or(catalog_config);
                let committer = crate::core::TableCommitter::with_catalog(
                    config.clone(),
                    namespace.clone(),
                    name.clone(),
                );
                IcebergMetadataService::from_catalog_table(
                    table,
                    branch.map(|s| s.to_string()),
                    committer,
                )
                .await?
            }
            TableResolution::Path(_) => {
                // Direct path - read-only operations only
                IcebergMetadataService::new_with_branch(
                    table_path.to_string(),
                    branch.map(|s| s.to_string()),
                )
                .await?
            }
        };
        service.execute(&metadata_service).await
    }

    /// Rewrite Iceberg manifest files - delegates to ManifestService
    async fn rewrite_iceberg_manifests(
        table_path: &str,
        args: &OptimizeManifestsArgs,
        committer: Option<TableCommitter>,
    ) -> Result<()> {
        let target_branch = args.branch.as_deref().unwrap_or("main");

        // Print header
        if args.branch.is_some() {
            println!(
                "{} Iceberg manifests at {} (branch: {})",
                "Rewriting".green(),
                table_path,
                target_branch.cyan()
            );
        } else {
            println!(
                "{} Iceberg manifests at {}",
                "Rewriting".green(),
                table_path
            );
        }

        let config = ManifestConfig {
            target_size: args.target_size,
            min_manifests: args.min_manifests,
            dry_run: args.dry_run,
            branch: args.branch.clone(),
        };

        let service = ManifestService::with_config(config);

        if args.dry_run {
            // Dry-run mode: analyze only
            let analysis = service.analyze(table_path).await?;
            Self::output_manifest_analysis(&analysis, &args.output)?;
        } else {
            // Execute rewrite
            let result = service.rewrite(table_path, committer).await?;
            Self::output_manifest_result(&result, &args.output)?;
        }

        Ok(())
    }

    /// Output manifest analysis (dry-run mode)
    fn output_manifest_analysis(
        analysis: &crate::core::maintenance::ManifestAnalysis,
        output_format: &str,
    ) -> Result<()> {
        if !analysis.should_rewrite {
            println!();
            println!(
                "{} {}",
                "Skipping:".yellow(),
                analysis
                    .skip_reason
                    .as_deref()
                    .unwrap_or("No rewrite needed")
            );
            return Ok(());
        }

        println!(
            "Current manifests: {}",
            analysis.current_manifests.to_string().cyan()
        );
        println!(
            "  Data manifests:   {}",
            analysis.data_manifests.to_string().cyan()
        );
        println!(
            "  Delete manifests: {}",
            analysis.delete_manifests.to_string().cyan()
        );
        println!();
        print_dry_run_header();
        println!(
            "Total data entries: {}",
            analysis.total_entries.to_string().cyan()
        );
        println!(
            "Would rewrite into: {} manifests",
            analysis.estimated_after.to_string().cyan()
        );

        if output_format == "json" {
            let json = serde_json::json!({
                "dry_run": true,
                "current_manifests": analysis.current_manifests,
                "data_manifests": analysis.data_manifests,
                "delete_manifests": analysis.delete_manifests,
                "total_entries": analysis.total_entries,
                "estimated_after": analysis.estimated_after,
            });
            print_json(&json)?;
        }

        Ok(())
    }

    /// Output manifest rewrite result
    fn output_manifest_result(
        result: &crate::core::maintenance::ManifestRewriteResult,
        output_format: &str,
    ) -> Result<()> {
        println!();
        println!("{}", "Manifests rewritten successfully!".green().bold());
        println!(
            "Manifests: {} -> {}",
            result.previous_manifests.to_string().cyan(),
            result.new_manifests.to_string().cyan()
        );
        println!("Snapshot:  {}", result.snapshot_id.to_string().cyan());
        println!("Version:   {}", result.metadata_version.to_string().cyan());

        if output_format == "json" {
            let json = serde_json::json!({
                "previous_manifests": result.previous_manifests,
                "new_manifests": result.new_manifests,
                "data_manifests_rewritten": result.data_manifests_rewritten,
                "delete_manifests_kept": result.delete_manifests_kept,
                "total_entries": result.total_entries,
                "snapshot_id": result.snapshot_id,
                "metadata_version": result.metadata_version,
            });
            print_json(&json)?;
        }

        Ok(())
    }

    /// Output data optimization result
    fn output_data_result(result: &MaintenanceResult, output_format: &str) -> Result<()> {
        let is_dry_run = result.operation.contains("dry-run")
            || result
                .details
                .get("mode")
                .map(|m| m == "dry-run")
                .unwrap_or(false);

        match output_format {
            "json" => {
                let json = serde_json::json!({
                    "dry_run": is_dry_run,
                    "operation": result.operation,
                    "files_added": result.files_added,
                    "files_removed": result.files_removed,
                    "bytes_added": result.bytes_added,
                    "bytes_removed": result.bytes_removed,
                    "records_affected": result.records_affected,
                    "details": result.details,
                });
                print_json(&json)?;
            }
            _ => {
                println!();

                if is_dry_run {
                    print_dry_run_header();
                    if result.files_added == 0 && result.files_removed == 0 {
                        println!("{}", "Table is already optimized.".green());
                        if let Some(reason) = result.details.get("reason") {
                            println!("{}", reason);
                        }
                    } else {
                        println!("{}", "Would perform the following changes:".cyan());
                        println!();
                        println!(
                            "  Files to compact:  {} -> {}",
                            result.files_removed.to_string().yellow(),
                            result.files_added.to_string().yellow()
                        );

                        if let Some(partitions) = result.details.get("partitions") {
                            println!("  Partitions:        {}", partitions.yellow());
                        }

                        if let Some(would_compact) = result.details.get("would_compact") {
                            println!("  Summary:           {}", would_compact.yellow());
                        }

                        println!();
                        println!(
                            "{}",
                            "Run without --dry-run to apply these changes.".dimmed()
                        );
                    }
                } else if result.files_added == 0 && result.files_removed == 0 {
                    println!("{}", "Table is already optimized.".green());
                    if let Some(reason) = result.details.get("reason") {
                        println!("{}", reason);
                    }
                } else {
                    println!("{}", "Compaction complete!".green().bold());
                    println!();
                    println!(
                        "Files compacted:  {} -> {}",
                        result.files_removed.to_string().cyan(),
                        result.files_added.to_string().cyan()
                    );
                    println!(
                        "Bytes saved:      {}",
                        format_bytes(result.bytes_removed.saturating_sub(result.bytes_added))
                    );
                    println!(
                        "Records affected: {}",
                        result.records_affected.to_string().cyan()
                    );

                    if let Some(snapshot_id) = result.details.get("snapshot_id") {
                        println!("Snapshot:         {}", snapshot_id.cyan());
                    }
                }
            }
        }
        Ok(())
    }
}

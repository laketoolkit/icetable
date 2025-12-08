//! Repair command implementation
//!
//! Thin wrapper that delegates to RepairService for both Delta Lake and Iceberg tables.

use colored::Colorize;

use super::common::resolve_table_path;
use crate::cli::parser::RepairArgs;
use crate::core::maintenance::{MaintenanceConfig, RepairAnalysis, RepairService};
use crate::core::metadata::MaintenanceResult;
use crate::core::storage::StorageBackendFactory;
use crate::core::utils::detect_table_format_with_storage;
use crate::core::{CatalogConfig, TableFormat, format_bytes};
use crate::error::{Error, Result};
use crate::utils::{with_timeout, track_memory_usage, with_cancellation};

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
    pub async fn execute(args: RepairArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        let table_path = resolve_table_path(&args.path, catalog_config.as_ref()).await?;
        
        // Apply timeout and cancellation from global resource limits
        with_timeout(async {
            with_cancellation(async {
                // Estimate memory usage: storage scanning + metadata
                let estimated_memory = 256 * 1024 * 1024; // 256MB for repair operations
                track_memory_usage(estimated_memory)?;
                
                Self::repair_inner(table_path, args, catalog_config).await
            }).await
        }).await
    }
    
    async fn repair_inner(table_path: String, mut args: RepairArgs, _catalog_config: Option<CatalogConfig>) -> Result<()> {
        args.path = Some(table_path.clone());

        // Validate at least one repair option is specified
        if !args.sync_metadata && !args.remove_missing && !args.add_orphans {
            return Err(Error::General(
                "Must specify at least one repair option: --sync-metadata, --remove-missing, or --add-orphans".to_string(),
            ));
        }

        // Create storage backend (supports local and cloud)
        let storage = StorageBackendFactory::create_backend(&table_path).await?;

        // Detect table format (use explicit format if provided, otherwise auto-detect)
        let format = if let Some(format_str) = &args.format {
            match format_str.as_str() {
                "delta" => TableFormat::Delta,
                "iceberg" => TableFormat::Iceberg,
                _ => TableFormat::Unknown,
            }
        } else {
            detect_table_format_with_storage(&table_path, &storage).await
        };

        // Determine repair options
        let options = RepairOptions {
            add_orphans: args.add_orphans || args.sync_metadata,
            remove_missing: args.remove_missing || args.sync_metadata,
        };

        // Create service configuration
        let config = MaintenanceConfig {
            dry_run: args.dry_run,
            ..Default::default()
        };

        let service = RepairService::with_config(config);

        match format {
            TableFormat::Delta => Self::repair_delta(&args, &service, options).await,
            TableFormat::Iceberg => {
                Self::repair_iceberg(&args, &service, options, &table_path).await
            }
            TableFormat::Unknown => Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                table_path
            ))),
        }
    }

    /// Repair Delta Lake table - not supported, use Iceberg instead
    async fn repair_delta(
        _args: &RepairArgs,
        _service: &RepairService,
        _options: RepairOptions,
    ) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake repair is not supported. Use 'icetable import delta' to convert to Iceberg.".to_string(),
        })
    }

    /// Repair Iceberg table
    async fn repair_iceberg(
        args: &RepairArgs,
        service: &RepairService,
        options: RepairOptions,
        table_path: &str,
    ) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;
        println!(
            "{} Iceberg table at {}",
            if args.dry_run {
                "Analyzing"
            } else {
                "Repairing"
            }
            .green(),
            table_path
        );

        let metadata_service = IcebergMetadataService::new_async(table_path.to_string()).await?;

        // First analyze to show what will be done
        let analysis = service.analyze(&metadata_service).await?;
        Self::print_analysis(&analysis, args, options)?;

        if !analysis.has_issues() {
            return Ok(());
        }

        // Check if any selected options have issues to fix
        let has_work = (options.add_orphans && !analysis.orphan_files.is_empty())
            || (options.remove_missing && !analysis.missing_files.is_empty());

        if !has_work {
            println!();
            println!(
                "{}",
                "No issues match the selected repair options.".yellow()
            );
            return Ok(());
        }

        if args.dry_run {
            return Ok(());
        }

        // Execute the repair
        let result = service.execute(&metadata_service).await?;
        Self::print_result(&result)?;

        Ok(())
    }

    /// Print analysis results
    fn print_analysis(
        analysis: &RepairAnalysis,
        args: &RepairArgs,
        options: RepairOptions,
    ) -> Result<()> {
        println!();
        println!(
            "Tracked files in metadata: {}",
            analysis.total_tracked.to_string().cyan()
        );
        println!(
            "Parquet files on disk:     {}",
            analysis.total_on_disk.to_string().cyan()
        );

        if !analysis.has_issues() {
            println!();
            println!("{}", "No issues found - table is healthy!".green());
            return Ok(());
        }

        println!();
        println!("Issues found:");

        if !analysis.missing_files.is_empty() {
            let status = if options.remove_missing {
                "will fix".green()
            } else {
                "skipped".dimmed()
            };
            println!(
                "  Missing files:  {} ({}) [{}]",
                analysis.missing_files.len().to_string().red(),
                format_bytes(analysis.missing_bytes()),
                status
            );
        }

        if !analysis.orphan_files.is_empty() {
            let status = if options.add_orphans {
                "will fix".green()
            } else {
                "skipped".dimmed()
            };
            println!(
                "  Orphan files:   {} ({}) [{}]",
                analysis.orphan_files.len().to_string().yellow(),
                format_bytes(analysis.orphan_bytes()),
                status
            );
        }

        if args.dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());

            if options.remove_missing {
                for file in &analysis.missing_files {
                    let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                    println!("  Would remove reference: {}", name.red());
                }
            }

            if options.add_orphans {
                for file in &analysis.orphan_files {
                    let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                    println!(
                        "  Would add: {} ({})",
                        name.green(),
                        format_bytes(file.size)
                    );
                }
            }
        }

        Ok(())
    }

    /// Print repair result
    fn print_result(result: &MaintenanceResult) -> Result<()> {
        println!();
        println!("{}", "Repair complete!".green().bold());
        println!(
            "Removed {} missing references, added {} orphan files",
            result.files_removed, result.files_added
        );

        if let Some(snapshot_id) = result.details.get("snapshot_id") {
            println!("New snapshot: {}", snapshot_id.cyan());
        }

        Ok(())
    }
}

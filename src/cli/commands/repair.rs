//! Repair command implementation
//!
//! Thin wrapper that delegates to RepairService for both Delta Lake and Iceberg tables.

use colored::Colorize;

use crate::cli::parser::RepairArgs;
use crate::core::maintenance::{MaintenanceConfig, RepairAnalysis, RepairService};
use crate::core::metadata::{utils, MaintenanceResult};
use crate::error::{Error, Result};

/// Handler for repair command
pub struct RepairCommand;

impl RepairCommand {
    /// Execute repair command
    pub async fn execute(args: RepairArgs) -> Result<()> {
        // Validate at least one repair option is specified
        if !args.sync_metadata && !args.remove_missing && !args.add_orphans {
            return Err(Error::General(
                "Must specify at least one repair option: --sync-metadata, --remove-missing, or --add-orphans".to_string(),
            ));
        }

        let path = std::path::Path::new(&args.path);

        // Detect table format
        let is_delta = path.join("_delta_log").exists();
        let is_iceberg = path.join("metadata").exists();

        // Create service configuration
        let config = MaintenanceConfig {
            dry_run: args.dry_run,
            ..Default::default()
        };

        let service = RepairService::with_config(config);

        if is_delta {
            Self::repair_delta(&args, &service).await
        } else if is_iceberg {
            Self::repair_iceberg(&args, &service).await
        } else {
            Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                args.path
            )))
        }
    }

    /// Repair Delta Lake table
    #[cfg(feature = "delta")]
    async fn repair_delta(args: &RepairArgs, service: &RepairService) -> Result<()> {
        use crate::core::metadata::DeltaMetadataService;

        println!(
            "{} Delta table at {}",
            if args.dry_run { "Analyzing" } else { "Repairing" }.green(),
            args.path
        );

        let metadata_service = DeltaMetadataService::new(args.path.clone().into())?;

        // First analyze to show what will be done
        let analysis = service.analyze(&metadata_service).await?;
        Self::print_analysis(&analysis, args)?;

        if !analysis.has_issues() {
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

    #[cfg(not(feature = "delta"))]
    async fn repair_delta(_args: &RepairArgs, _service: &RepairService) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    /// Repair Iceberg table
    #[cfg(feature = "iceberg")]
    async fn repair_iceberg(args: &RepairArgs, service: &RepairService) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;

        println!(
            "{} Iceberg table at {}",
            if args.dry_run { "Analyzing" } else { "Repairing" }.green(),
            args.path
        );

        let metadata_service = IcebergMetadataService::new(args.path.clone().into())?;

        // First analyze to show what will be done
        let analysis = service.analyze(&metadata_service).await?;
        Self::print_analysis(&analysis, args)?;

        if !analysis.has_issues() {
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

    #[cfg(not(feature = "iceberg"))]
    async fn repair_iceberg(_args: &RepairArgs, _service: &RepairService) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Iceberg support not enabled".to_string(),
        })
    }

    /// Print analysis results
    fn print_analysis(analysis: &RepairAnalysis, args: &RepairArgs) -> Result<()> {
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
            println!(
                "  Missing files:  {} ({})",
                analysis.missing_files.len().to_string().red(),
                utils::format_bytes(analysis.missing_bytes())
            );
        }

        if !analysis.orphan_files.is_empty() {
            println!(
                "  Orphan files:   {} ({})",
                analysis.orphan_files.len().to_string().yellow(),
                utils::format_bytes(analysis.orphan_bytes())
            );
        }

        if args.dry_run {
            println!();
            println!("{}", "DRY RUN - No changes made".yellow().bold());

            for file in &analysis.missing_files {
                let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                println!("  Would remove reference: {}", name.red());
            }

            for file in &analysis.orphan_files {
                let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                println!(
                    "  Would add: {} ({})",
                    name.green(),
                    utils::format_bytes(file.size)
                );
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

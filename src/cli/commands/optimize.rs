//! Optimize command implementation
//!
//! Thin wrapper that delegates to OptimizeService for both Delta Lake and Iceberg tables.

use colored::Colorize;

use crate::cli::parser::OptimizeArgs;
use crate::core::maintenance::{MaintenanceConfig, OptimizeService};
use crate::core::metadata::MaintenanceResult;
use crate::core::{detect_table_format, format_bytes, TableFormat};
use crate::error::{Error, Result};

/// Handler for optimize command
pub struct OptimizeCommand;

impl OptimizeCommand {
    /// Execute optimize command
    pub async fn execute(args: OptimizeArgs) -> Result<()> {
        let path = std::path::Path::new(&args.path);

        // Detect table format
        let format = detect_table_format(path);

        // Create service configuration
        let config = MaintenanceConfig {
            target_size: args.target_size,
            min_size: args.min_file_size.unwrap_or(args.target_size / 16),
            dry_run: args.dry_run,
            parallelism: args.max_concurrent_tasks,
            ..Default::default()
        };

        let service = OptimizeService::with_config(config);

        let result = match format {
            TableFormat::Delta => Self::optimize_delta(&args, &service).await?,
            TableFormat::Iceberg => Self::optimize_iceberg(&args, &service).await?,
            TableFormat::Unknown => {
                return Err(Error::General(format!(
                    "Path '{}' is not a Delta Lake or Iceberg table",
                    args.path
                )));
            }
        };

        Self::output_result(&result, &args.output)?;
        Ok(())
    }

    /// Optimize Delta Lake table
    #[cfg(feature = "delta")]
    async fn optimize_delta(
        args: &OptimizeArgs,
        service: &OptimizeService,
    ) -> Result<MaintenanceResult> {
        use crate::core::metadata::DeltaMetadataService;

        println!("{} Delta table at {}", "Optimizing".green(), args.path);

        let metadata_service = DeltaMetadataService::new(args.path.clone().into())?;
        service.execute(&metadata_service).await
    }

    #[cfg(not(feature = "delta"))]
    async fn optimize_delta(
        _args: &OptimizeArgs,
        _service: &OptimizeService,
    ) -> Result<MaintenanceResult> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    /// Optimize Iceberg table
    #[cfg(feature = "iceberg")]
    async fn optimize_iceberg(
        args: &OptimizeArgs,
        service: &OptimizeService,
    ) -> Result<MaintenanceResult> {
        use crate::core::metadata::IcebergMetadataService;

        println!("{} Iceberg table at {}", "Optimizing".green(), args.path);

        let metadata_service = IcebergMetadataService::new(args.path.clone().into())?;
        service.execute(&metadata_service).await
    }

    #[cfg(not(feature = "iceberg"))]
    async fn optimize_iceberg(
        _args: &OptimizeArgs,
        _service: &OptimizeService,
    ) -> Result<MaintenanceResult> {
        Err(Error::UnsupportedFeature {
            feature: "Iceberg support not enabled".to_string(),
        })
    }

    /// Output result in the requested format
    fn output_result(result: &MaintenanceResult, output_format: &str) -> Result<()> {
        match output_format {
            "json" => {
                let json = serde_json::json!({
                    "operation": result.operation,
                    "files_added": result.files_added,
                    "files_removed": result.files_removed,
                    "bytes_added": result.bytes_added,
                    "bytes_removed": result.bytes_removed,
                    "records_affected": result.records_affected,
                    "details": result.details,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
                );
            }
            _ => {
                println!();
                if result.files_added == 0 && result.files_removed == 0 {
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

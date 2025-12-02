//! Vacuum command implementation
//!
//! Removes old files no longer referenced by Delta Lake and Iceberg tables.
//! Delta uses native DeltaOps.vacuum() for optimal performance.
//! Iceberg uses VacuumService for consistent behavior.

use colored::Colorize;

use crate::cli::parser::VacuumArgs;
use crate::core::maintenance::{VacuumAnalysis, VacuumConfig, VacuumService};
use crate::core::{detect_table_format, format_bytes, TableFormat};
use crate::error::{Error, Result};

/// Handler for vacuum command
pub struct VacuumCommand;

impl VacuumCommand {
    /// Execute vacuum command
    pub async fn execute(args: VacuumArgs) -> Result<()> {
        let path = std::path::Path::new(&args.path);

        // Detect table format
        let format = detect_table_format(path);

        match format {
            TableFormat::Delta => Self::vacuum_delta(&args).await,
            TableFormat::Iceberg => Self::vacuum_iceberg(&args).await,
            TableFormat::Unknown => Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                args.path
            ))),
        }
    }

    /// Vacuum Delta Lake table using native DeltaOps
    #[cfg(feature = "delta")]
    async fn vacuum_delta(args: &VacuumArgs) -> Result<()> {
        use deltalake::DeltaOps;

        println!(
            "{} Delta table at {}",
            if args.dry_run {
                "Analyzing".yellow()
            } else {
                "Vacuuming".green()
            },
            args.path
        );

        // Open the table
        let table = deltalake::open_table(&args.path)
            .await
            .map_err(|e| Error::General(format!("Failed to open Delta table: {}", e)))?;

        // Build vacuum operation
        let retention = chrono::Duration::hours(args.retention_hours as i64);
        let mut vacuum = DeltaOps(table).vacuum().with_retention_period(retention);

        if args.dry_run {
            vacuum = vacuum.with_dry_run(true);
        }

        if args.force {
            vacuum = vacuum.with_enforce_retention_duration(false);
        }

        // Execute vacuum
        let (table, metrics) = vacuum
            .await
            .map_err(|e| Error::General(format!("Vacuum failed: {}", e)))?;

        // Output results
        match args.output.as_str() {
            "json" => Self::output_delta_json(&metrics, args.dry_run)?,
            _ => Self::output_delta_text(&metrics, args.dry_run, table.version())?,
        }

        Ok(())
    }

    #[cfg(not(feature = "delta"))]
    async fn vacuum_delta(_args: &VacuumArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Delta Lake support not enabled".to_string(),
        })
    }

    /// Output Delta vacuum results as text
    #[cfg(feature = "delta")]
    fn output_delta_text(
        metrics: &deltalake::operations::vacuum::VacuumMetrics,
        dry_run: bool,
        version: Option<i64>,
    ) -> Result<()> {
        println!();

        if dry_run {
            println!("{}", "DRY RUN - No files were deleted".yellow().bold());
            println!();
        }

        println!(
            "Files {}:   {}",
            if dry_run { "to delete" } else { "deleted" },
            metrics.files_deleted.len().to_string().cyan()
        );

        if !metrics.files_deleted.is_empty() {
            println!();
            println!("Files:");
            for file in &metrics.files_deleted {
                println!("  - {}", file.dimmed());
            }
        }

        if !dry_run {
            if let Some(v) = version {
                println!();
                println!("Table version: {}", v.to_string().green());
            }
        }

        Ok(())
    }

    /// Output Delta vacuum results as JSON
    #[cfg(feature = "delta")]
    fn output_delta_json(
        metrics: &deltalake::operations::vacuum::VacuumMetrics,
        dry_run: bool,
    ) -> Result<()> {
        let json = serde_json::json!({
            "dry_run": dry_run,
            "files_deleted": metrics.files_deleted,
            "files_count": metrics.files_deleted.len(),
        });

        println!(
            "{}",
            serde_json::to_string_pretty(&json)
                .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
        );

        Ok(())
    }

    /// Vacuum Iceberg table using VacuumService
    #[cfg(feature = "iceberg")]
    async fn vacuum_iceberg(args: &VacuumArgs) -> Result<()> {
        use crate::core::metadata::IcebergMetadataService;

        println!(
            "{} Iceberg table at {}",
            if args.dry_run {
                "Analyzing".yellow()
            } else {
                "Vacuuming".green()
            },
            args.path
        );

        let config = VacuumConfig {
            retention_hours: args.retention_hours,
            dry_run: args.dry_run,
            include_metadata: true, // Iceberg vacuum includes old metadata
        };

        let service = VacuumService::with_config(config);
        let metadata_service = IcebergMetadataService::new(args.path.clone().into())?;

        // Get analysis first to show details
        let analysis = service.analyze(&metadata_service).await?;

        // Output results
        match args.output.as_str() {
            "json" => Self::output_iceberg_json(&analysis, args.dry_run)?,
            _ => Self::output_iceberg_text(&analysis, args.dry_run)?,
        }

        // Execute if not dry run and there are files to delete
        if !args.dry_run && analysis.has_files_to_delete() {
            let result = service.execute(&metadata_service).await?;
            println!();
            println!(
                "{} {} files, freed {}",
                "Deleted".green().bold(),
                result.files_removed,
                format_bytes(result.bytes_removed)
            );
        }

        Ok(())
    }

    #[cfg(not(feature = "iceberg"))]
    async fn vacuum_iceberg(_args: &VacuumArgs) -> Result<()> {
        Err(Error::UnsupportedFeature {
            feature: "Iceberg support not enabled".to_string(),
        })
    }

    /// Output Iceberg vacuum analysis as text
    #[cfg(feature = "iceberg")]
    fn output_iceberg_text(analysis: &VacuumAnalysis, dry_run: bool) -> Result<()> {
        println!();

        if dry_run {
            println!("{}", "DRY RUN - No files will be deleted".yellow().bold());
            println!();
        }

        println!(
            "Referenced files: {}",
            analysis.referenced_count.to_string().cyan()
        );
        println!(
            "Files {}:   {} ({})",
            if dry_run { "to delete" } else { "deleted" },
            analysis.orphan_files.len().to_string().cyan(),
            format_bytes(analysis.orphan_bytes)
        );
        println!(
            "Retention:        {} hours",
            analysis.retention_hours.to_string().cyan()
        );

        if !analysis.orphan_files.is_empty() {
            println!();
            println!("Files:");
            for file in &analysis.orphan_files {
                let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                println!(
                    "  - {} ({})",
                    name.dimmed(),
                    format_bytes(file.size)
                );
            }
        }

        Ok(())
    }

    /// Output Iceberg vacuum analysis as JSON
    #[cfg(feature = "iceberg")]
    fn output_iceberg_json(analysis: &VacuumAnalysis, dry_run: bool) -> Result<()> {
        let files: Vec<&str> = analysis
            .orphan_files
            .iter()
            .map(|f| f.path.as_str())
            .collect();

        let json = serde_json::json!({
            "dry_run": dry_run,
            "files_to_delete": files,
            "files_count": analysis.orphan_files.len(),
            "bytes_to_free": analysis.orphan_bytes,
            "retention_hours": analysis.retention_hours,
        });

        println!(
            "{}",
            serde_json::to_string_pretty(&json)
                .map_err(|e| Error::General(format!("Failed to serialize: {}", e)))?
        );

        Ok(())
    }
}

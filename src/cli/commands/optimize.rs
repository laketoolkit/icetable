//! Optimize command implementation
//!
//! Thin wrapper that delegates to core services.
//!
//! Subcommands:
//! - `data`: Compact small data files into larger ones
//! - `manifests`: Rewrite and compact manifest files
//! - `vacuum`: Clean up unreferenced files

use super::VacuumCommand;
use super::common::{TableResolution, resolve_table_from_context};
use crate::cli::output::OptimizeFormatter;
use crate::cli::parser::{
    CatalogContext, OptimizeCommands, OptimizeDataArgs, OptimizeManifestsArgs,
};
use crate::cli::utils::IndicatifReporter;
use crate::core::CatalogConfig;
use crate::core::maintenance::{
    MaintenanceConfig, ManifestConfig, ManifestService, OptimizeService,
};
use crate::core::metadata::MaintenanceResult;
use crate::core::utils::parse_bytes;
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for optimize command
pub struct OptimizeCommand;

impl OptimizeCommand {
    /// Execute optimize command
    pub async fn execute(cmd: OptimizeCommands, ctx: &CatalogContext) -> Result<()> {
        match cmd {
            OptimizeCommands::Data(args) => Self::execute_data(args, ctx).await,
            OptimizeCommands::Manifests(args) => Self::execute_manifests(args, ctx).await,
            OptimizeCommands::Vacuum(args) => VacuumCommand::execute(args, ctx).await,
        }
    }

    /// Execute optimize data subcommand
    async fn execute_data(args: OptimizeDataArgs, ctx: &CatalogContext) -> Result<()> {
        let resolution = resolve_table_from_context(ctx).await?;
        let table_path = resolution.location().to_string();

        let max_bytes = args
            .max_bytes
            .as_ref()
            .map(|s| parse_bytes(s))
            .transpose()
            .map_err(|e| Error::Parse {
                message: format!("Invalid --max-bytes value: {}", e),
                source: None,
            })?;

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

        // Create service with progress reporter (skip for dry-run as it doesn't process files)
        let service = if args.dry_run {
            OptimizeService::with_config(config)
        } else {
            let progress = IndicatifReporter::spinner("Analyzing files...").arc();
            OptimizeService::with_config(config).with_progress(progress)
        };

        // Apply resource limits (timeout, cancellation, memory tracking)
        let estimated_memory = args.target_size * args.max_concurrent_tasks as u64;
        let result = with_resource_limits(
            estimated_memory,
            Self::optimize_iceberg_data(
                &table_path,
                &service,
                args.branch.as_deref(),
                &resolution,
                ctx.catalog_config.as_ref(),
            ),
        )
        .await?;

        Self::output_data_result(&result, &args.output)?;
        Ok(())
    }

    /// Execute optimize manifests subcommand
    async fn execute_manifests(args: OptimizeManifestsArgs, ctx: &CatalogContext) -> Result<()> {
        let resolution = resolve_table_from_context(ctx).await?;
        let table_path = resolution.location().to_string();

        // Apply resource limits (timeout, cancellation, memory tracking)
        let estimated_memory = args.target_size * 2;
        with_resource_limits(
            estimated_memory,
            Self::rewrite_iceberg_manifests(
                &table_path,
                &args,
                &resolution,
                ctx.catalog_config.as_ref(),
            ),
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
        println!(
            "{}",
            OptimizeFormatter::format_data_header(table_path, branch)
        );

        // Create metadata service using factory method - handles catalog vs path context automatically
        let metadata_service = resolution.to_writable_service(cli_catalog, branch).await?;
        service.execute(&metadata_service).await
    }

    /// Rewrite Iceberg manifest files - delegates to ManifestService
    async fn rewrite_iceberg_manifests(
        table_path: &str,
        args: &OptimizeManifestsArgs,
        resolution: &TableResolution,
        cli_catalog: Option<&CatalogConfig>,
    ) -> Result<()> {
        println!(
            "{}",
            OptimizeFormatter::format_manifests_header(table_path, args.branch.as_deref())
        );

        let config = ManifestConfig {
            target_size: args.target_size,
            min_manifests: args.min_manifests,
            dry_run: args.dry_run,
            branch: args.branch.clone(),
        };

        let service = ManifestService::with_config(config);

        // Create metadata service using factory method - handles catalog vs path context automatically
        let metadata_service = resolution
            .to_writable_service(cli_catalog, args.branch.as_deref())
            .await?;

        if args.dry_run {
            // Dry-run mode: analyze only
            let analysis = service.analyze(&metadata_service).await?;
            Self::output_manifest_analysis(&analysis, &args.output)?;
        } else {
            // Execute rewrite
            let result = service.rewrite(&metadata_service).await?;
            Self::output_manifest_result(&result, &args.output)?;
        }

        Ok(())
    }

    /// Output manifest analysis (dry-run mode)
    fn output_manifest_analysis(
        analysis: &crate::core::maintenance::ManifestAnalysis,
        output_format: &str,
    ) -> Result<()> {
        if output_format == "json" {
            let json_str = OptimizeFormatter::format_manifest_analysis_json(analysis)?;
            println!("{}", json_str);
        } else {
            println!("{}", OptimizeFormatter::format_manifest_analysis(analysis));
        }
        Ok(())
    }

    /// Output manifest rewrite result
    fn output_manifest_result(
        result: &crate::core::maintenance::ManifestRewriteResult,
        output_format: &str,
    ) -> Result<()> {
        if output_format == "json" {
            let json_str = OptimizeFormatter::format_manifest_result_json(result)?;
            println!("{}", json_str);
        } else {
            println!("{}", OptimizeFormatter::format_manifest_result(result));
        }
        Ok(())
    }

    /// Output data optimization result
    fn output_data_result(result: &MaintenanceResult, output_format: &str) -> Result<()> {
        if output_format == "json" {
            let json_str = OptimizeFormatter::format_data_result_json(result)?;
            println!("{}", json_str);
        } else {
            println!("{}", OptimizeFormatter::format_data_result_text(result));
        }
        Ok(())
    }
}

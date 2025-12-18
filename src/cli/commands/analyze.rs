//! Analyze command implementation
//!
//! Analyzes table health and provides optimization recommendations.
//! The business logic is delegated to AnalyzeService in core::analysis.

use colored::Colorize;

use super::common::{create_spinner, extract_table_name, resolve_table_from_context};
use crate::cli::output::AnalyzeFormatter;
use crate::cli::parser::{AnalyzeArgs, CatalogContext};
use crate::core::analysis::{AnalysisConfig, AnalyzeService};
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for analyze command
pub struct AnalyzeCommand;

impl AnalyzeCommand {
    /// Execute analyze command
    pub async fn execute(args: AnalyzeArgs, ctx: &CatalogContext) -> Result<()> {
        let resolution = resolve_table_from_context(ctx).await?;
        let table_path = resolution.location();

        use super::constants::MEMORY_HEAVY_OPS;
        with_resource_limits(
            MEMORY_HEAVY_OPS,
            Self::analyze_iceberg(&table_path, &args, &resolution),
        )
        .await
    }

    async fn analyze_iceberg(
        table_path: &str,
        args: &AnalyzeArgs,
        resolution: &super::common::TableResolution,
    ) -> Result<()> {
        let is_json = args.output == "json";

        if !is_json {
            let table_name = extract_table_name(table_path);
            println!("{} {}", "Analyzing".green(), table_name.cyan());
            println!("{}", table_path.dimmed());
            println!();
        }

        // Load metadata service
        let service = resolution.to_readonly_service().await?;

        // Create analysis service with configuration from args
        let config = AnalysisConfig {
            min_file_size: args.min_file_size,
            skip_orphans: args.skip_orphans,
            all_snapshots: args.all_snapshots,
        };
        let analyze_service = AnalyzeService::with_config(config);

        // Run analysis with progress indicators
        let pb = create_spinner("Analyzing current snapshot");
        let data_analysis = analyze_service.analyze_data_compaction(&service).await?;
        pb.finish_and_clear();

        let manifest_analysis = analyze_service.analyze_manifests(&service).await?;
        let (metadata, _) = service.load_metadata().await?;
        let snapshot_analysis = analyze_service.analyze_snapshots(&metadata);

        let orphan_analysis = if !args.skip_orphans {
            let pb = create_spinner("Scanning for orphan files");
            let result = analyze_service.analyze_orphans(&service).await?;
            pb.finish_and_clear();
            Some(result)
        } else {
            None
        };

        // Format and display results
        if is_json {
            let json = AnalyzeFormatter::format_json(
                table_path,
                &data_analysis,
                &manifest_analysis,
                &snapshot_analysis,
                orphan_analysis.as_ref(),
            )?;
            println!("{}", json);
        } else {
            let output = AnalyzeFormatter::format_table(
                &data_analysis,
                &manifest_analysis,
                &snapshot_analysis,
                orphan_analysis.as_ref(),
                args.verbose,
            );
            println!("{}", output);
        }

        Ok(())
    }
}

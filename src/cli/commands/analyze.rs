//! Analyze command implementation
//!
//! Analyzes table health and provides optimization recommendations.
//! The business logic is delegated to AnalyzeService in core::analysis.

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::common::{create_spinner, extract_table_name, print_json, resolve_table, TableResolution};
use crate::cli::output::create_styled_table;
use crate::cli::parser::AnalyzeArgs;
use crate::core::analysis::{
    AnalysisConfig, AnalyzeService, DataCompactionAnalysis, ManifestCompactionAnalysis,
    OrphanFilesAnalysis, SnapshotExpirationAnalysis,
};
use crate::core::metadata::IcebergMetadataService;
use crate::core::{format_bytes, format_count};
use crate::core::CatalogConfig;
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for analyze command
pub struct AnalyzeCommand;

impl AnalyzeCommand {
    /// Execute analyze command
    pub async fn execute(args: AnalyzeArgs, catalog_config: Option<CatalogConfig>) -> Result<()> {
        let resolution = resolve_table(&args.path, catalog_config.as_ref()).await?;
        let table_path = resolution.location();

        // Apply resource limits (timeout, cancellation, memory tracking)
        const ESTIMATED_MEMORY: u64 = 128 * 1024 * 1024; // 128MB for analysis
        with_resource_limits(ESTIMATED_MEMORY, Self::analyze_iceberg(&table_path, &args, &resolution)).await
    }

    async fn analyze_iceberg(table_path: &str, args: &AnalyzeArgs, resolution: &TableResolution) -> Result<()> {
        let is_json = args.output == "json";

        if !is_json {
            let table_name = extract_table_name(table_path);

            println!("{} {}", "Analyzing".green(), table_name.cyan());
            println!("{}", table_path.dimmed());
            println!();
        }

        // Load metadata service: use catalog table when available for proper metadata consistency
        let service = match resolution {
            TableResolution::CatalogTable { table, .. } => {
                // Use catalog table's metadata - read-only, no committer needed
                IcebergMetadataService::from_catalog_table_readonly(table).await?
            }
            TableResolution::Path(_) => {
                // Direct path - use storage metadata
                IcebergMetadataService::new_async(table_path.to_string()).await?
            }
        };

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

        let manifest_analysis = analyze_service
            .analyze_manifests(&service)
            .await?;
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

        // Print results
        Self::print_analysis(
            table_path,
            &data_analysis,
            &manifest_analysis,
            &snapshot_analysis,
            &orphan_analysis,
            &args.output,
            args.verbose,
        )
    }

    fn print_analysis(
        table_path: &str,
        data: &DataCompactionAnalysis,
        manifest: &ManifestCompactionAnalysis,
        snapshot: &SnapshotExpirationAnalysis,
        orphan: &Option<OrphanFilesAnalysis>,
        output: &str,
        verbose: bool,
    ) -> Result<()> {
        if output == "json" {
            return Self::print_json(table_path, data, manifest, snapshot, orphan);
        }

        // Build summary table
        let mut table = create_styled_table();

        table.set_header(vec![
            Cell::new("Metric".cyan().to_string()).set_alignment(CellAlignment::Center),
            Cell::new("Count".cyan().to_string()).set_alignment(CellAlignment::Center),
            Cell::new("Size".cyan().to_string()).set_alignment(CellAlignment::Center),
            Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Center),
        ]);

        // Data row
        let data_status = if data.needs_action() {
            "⚠".yellow().to_string()
        } else {
            "✓".green().to_string()
        };
        table.add_row(vec![
            Cell::new("Data files"),
            Cell::new(format_count(data.total_files)).set_alignment(CellAlignment::Right),
            Cell::new(format_bytes(data.total_size)).set_alignment(CellAlignment::Right),
            Cell::new(data_status).set_alignment(CellAlignment::Center),
        ]);

        // Manifests row
        let manifest_status = if manifest.needs_action() {
            "⚠".yellow().to_string()
        } else {
            "✓".green().to_string()
        };
        table.add_row(vec![
            Cell::new("Manifests"),
            Cell::new(format_count(manifest.total_manifests)).set_alignment(CellAlignment::Right),
            Cell::new("-").set_alignment(CellAlignment::Right),
            Cell::new(manifest_status).set_alignment(CellAlignment::Center),
        ]);

        // Snapshots row
        let snapshot_status = if snapshot.needs_action() {
            "⚠".yellow().to_string()
        } else {
            "✓".green().to_string()
        };
        table.add_row(vec![
            Cell::new("Snapshots"),
            Cell::new(format_count(snapshot.total_snapshots)).set_alignment(CellAlignment::Right),
            Cell::new("-").set_alignment(CellAlignment::Right),
            Cell::new(snapshot_status).set_alignment(CellAlignment::Center),
        ]);

        // Orphans row (if checked)
        if let Some(orphan) = orphan {
            let orphan_status = if orphan.has_missing_files() {
                "✗".red().to_string()
            } else if orphan.has_orphan_files() {
                "⚠".yellow().to_string()
            } else {
                "✓".green().to_string()
            };
            table.add_row(vec![
                Cell::new("Orphans"),
                Cell::new(format_count(orphan.orphan_count)).set_alignment(CellAlignment::Right),
                Cell::new(format_bytes(orphan.orphan_size)).set_alignment(CellAlignment::Right),
                Cell::new(orphan_status).set_alignment(CellAlignment::Center),
            ]);
        }

        println!("{}", table);
        println!();

        // Collect recommendations
        let mut recommendations: Vec<(String, String)> = Vec::new();

        // Data compaction recommendation
        if data.needs_action() {
            let detail = format!(
                "Compact {} small files in {} partitions",
                data.small_files, data.groups_needing_compaction
            );
            recommendations.push((detail, "icetable optimize data --dry-run".to_string()));
        }

        // Manifest compaction recommendation
        if manifest.needs_action() {
            let detail = format!(
                "Rewrite {} manifests (target: {})",
                manifest.total_manifests, manifest.recommended_max
            );
            recommendations.push((detail, "icetable optimize manifests --dry-run".to_string()));
        }

        // Snapshot expiration recommendation
        if snapshot.needs_action() {
            let detail = format!(
                "Expire {} snapshots older than 7 days",
                snapshot.snapshots_older_than_7d
            );
            recommendations.push((
                detail,
                "icetable snapshot expire --older-than 7d --dry-run".to_string(),
            ));
        }

        // Orphan files recommendation
        if let Some(orphan) = orphan {
            if orphan.has_orphan_files() {
                let detail = format!(
                    "Remove {} orphan files ({})",
                    orphan.orphan_count,
                    format_bytes(orphan.orphan_size)
                );
                recommendations.push((detail, "icetable vacuum --dry-run".to_string()));
            }
            if orphan.has_missing_files() {
                let detail = format!("Repair {} missing file references", orphan.missing_count);
                recommendations.push((
                    detail,
                    "icetable repair --remove-missing --dry-run".to_string(),
                ));
            }
        }

        // Print recommendations
        if recommendations.is_empty() {
            println!("{}", "✓ Table is healthy!".green().bold());
        } else {
            println!("{}", "Recommendations:".bold());
            for (detail, command) in &recommendations {
                println!("  {} {}", "⚠".yellow(), detail.yellow());
                println!("    → {}", command.dimmed());
            }
        }

        // Verbose: show partition details for data compaction
        if verbose && data.needs_action() && !data.partitions.is_empty() {
            println!();
            println!("{}", "Top partitions by priority:".dimmed());
            let max_show = 10;
            for (i, p) in data.partitions.iter().take(max_show).enumerate() {
                let priority_color = match p.priority.as_str() {
                    "high" => p.priority.red(),
                    "medium" => p.priority.yellow(),
                    _ => p.priority.dimmed(),
                };
                let records_str = if p.records > 1_000_000 {
                    format!("{:.1}M rows", p.records as f64 / 1_000_000.0)
                } else if p.records > 1_000 {
                    format!("{:.1}K rows", p.records as f64 / 1_000.0)
                } else {
                    format!("{} rows", p.records)
                };
                println!(
                    "  {}. {} {} files, {} ({}) [{}]",
                    i + 1,
                    p.partition.cyan(),
                    p.files.to_string().white(),
                    records_str,
                    format_bytes(p.size_bytes),
                    priority_color
                );
            }
            if data.partitions.len() > max_show {
                println!(
                    "  {} ({} more)",
                    "...".dimmed(),
                    data.partitions.len() - max_show
                );
            }
        }

        Ok(())
    }

    fn print_json(
        table_path: &str,
        data: &DataCompactionAnalysis,
        manifest: &ManifestCompactionAnalysis,
        snapshot: &SnapshotExpirationAnalysis,
        orphan: &Option<OrphanFilesAnalysis>,
    ) -> Result<()> {
        let json = serde_json::json!({
            "table_path": table_path,
            "data_compaction": {
                "total_files": data.total_files,
                "small_files": data.small_files,
                "total_size_bytes": data.total_size,
                "small_files_size_bytes": data.small_files_size,
                "min_size_threshold_bytes": data.min_size_threshold,
                "needs_action": data.needs_action(),
                "partitions": data.partitions,
            },
            "manifest_compaction": {
                "total_manifests": manifest.total_manifests,
                "recommended_max": manifest.recommended_max,
                "needs_action": manifest.needs_action(),
            },
            "snapshot_expiration": {
                "total_snapshots": snapshot.total_snapshots,
                "older_than_7_days": snapshot.snapshots_older_than_7d,
                "older_than_30_days": snapshot.snapshots_older_than_30d,
                "oldest_age_days": snapshot.oldest_snapshot_age_days,
                "needs_action": snapshot.needs_action(),
            },
            "orphan_files": orphan.as_ref().map(|o| serde_json::json!({
                "orphan_count": o.orphan_count,
                "orphan_size_bytes": o.orphan_size,
                "missing_count": o.missing_count,
                "needs_action": o.needs_action(),
            })),
        });

        print_json(&json)?;

        Ok(())
    }
}

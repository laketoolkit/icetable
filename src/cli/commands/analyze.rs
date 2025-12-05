//! Analyze command implementation
//!
//! Analyzes table health and provides optimization recommendations.
//! The business logic is delegated to AnalyzeService in core::analysis.

use colored::Colorize;
use comfy_table::{presets::UTF8_FULL, Cell, CellAlignment, ContentArrangement};

use crate::cli::parser::AnalyzeArgs;
use crate::config::ResolvePath;
use crate::core::analysis::{
    AnalysisConfig, AnalyzeService, DataCompactionAnalysis, ManifestCompactionAnalysis,
    OrphanFilesAnalysis, SnapshotExpirationAnalysis,
};
use crate::core::metadata::IcebergMetadataService;
use crate::core::utils::format_bytes;
use crate::core::{TableFormat, detect_table_format_async};
use crate::error::{Error, Result};

/// Handler for analyze command
pub struct AnalyzeCommand;

impl AnalyzeCommand {
    /// Execute analyze command
    pub async fn execute(args: AnalyzeArgs) -> Result<()> {
        let table_path = args.path.resolve()?;

        // Detect table format
        let format = detect_table_format_async(&table_path).await;

        match format {
            TableFormat::Delta => {
                println!(
                    "{}",
                    "Delta Lake analysis is not supported. Use 'icetable import delta' to convert to Iceberg."
                        .yellow()
                );
                Ok(())
            }
            TableFormat::Iceberg => Self::analyze_iceberg(&table_path, &args).await,
            TableFormat::Unknown => Err(Error::General(format!(
                "Path '{}' is not a Delta Lake or Iceberg table",
                table_path
            ))),
        }
    }

    async fn analyze_iceberg(table_path: &str, args: &AnalyzeArgs) -> Result<()> {
        use indicatif::{ProgressBar, ProgressStyle};

        let is_json = args.output == "json";

        if !is_json {
            let table_name = table_path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("table");

            println!("{} {}", "Analyzing".green(), table_name.cyan());
            println!("{}", table_path.dimmed());
            println!();
        }

        // Load metadata service
        let service = IcebergMetadataService::new_async(table_path.to_string()).await?;

        // Create analysis service with configuration from args
        let config = AnalysisConfig {
            min_file_size: args.min_file_size,
            skip_orphans: args.skip_orphans,
        };
        let analyze_service = AnalyzeService::with_config(config);

        // Run analysis with progress indicators
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} Analyzing current snapshot...")
                .unwrap(),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let data_analysis = analyze_service.analyze_data_compaction(&service).await?;
        pb.finish_and_clear();

        let (metadata, _) = service.load_metadata().await?;
        let manifest_analysis = analyze_service
            .analyze_manifests(&metadata, &service)
            .await?;
        let snapshot_analysis = analyze_service.analyze_snapshots(&metadata);

        let orphan_analysis = if !args.skip_orphans {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} Scanning for orphan files...")
                    .unwrap(),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(100));
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
        _table_path: &str,
        data: &DataCompactionAnalysis,
        manifest: &ManifestCompactionAnalysis,
        snapshot: &SnapshotExpirationAnalysis,
        orphan: &Option<OrphanFilesAnalysis>,
        output: &str,
        verbose: bool,
    ) -> Result<()> {
        if output == "json" {
            return Self::print_json(_table_path, data, manifest, snapshot, orphan);
        }

        // Build summary table
        let mut table = comfy_table::Table::new();
        table.load_preset(UTF8_FULL);
        table.set_content_arrangement(ContentArrangement::Dynamic);

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
            Cell::new(format_count(manifest.total_manifests as usize))
                .set_alignment(CellAlignment::Right),
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
            recommendations.push((detail, "icetable snapshot expire --older-than 7d --dry-run".to_string()));
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
                recommendations.push((detail, "icetable repair --remove-missing --dry-run".to_string()));
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

        println!(
            "{}",
            serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
        );

        Ok(())
    }
}

/// Format a count with thousands separators
fn format_count(count: usize) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

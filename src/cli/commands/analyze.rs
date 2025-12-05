//! Analyze command implementation
//!
//! Analyzes table health and provides optimization recommendations.
//! The business logic is delegated to AnalyzeService in core::analysis.

use colored::Colorize;

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
                    "Delta Lake analysis is not supported. Use 'icectl import delta' to convert to Iceberg."
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

        println!("{}", "Recommendations:".bold());
        println!();

        let mut has_recommendations = false;

        // Data compaction
        if data.needs_action() {
            has_recommendations = true;
            println!(
                "  {} {}",
                "!".yellow(),
                "Data compaction recommended".yellow().bold()
            );
            println!(
                "    Small files: {} of {} (< {})",
                data.small_files.to_string().cyan(),
                data.total_files.to_string().cyan(),
                format_bytes(data.min_size_threshold)
            );
            println!(
                "    Partitions to compact: {}",
                data.groups_needing_compaction.to_string().cyan()
            );

            // Verbose: show partition details
            if verbose && !data.partitions.is_empty() {
                println!();
                println!("    {}", "Top partitions by priority:".dimmed());
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
                        "      {}. {} {} files, {} ({}) [{}]",
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
                        "      {} ({} more)",
                        "...".dimmed(),
                        data.partitions.len() - max_show
                    );
                }
            }

            println!();
            println!(
                "    Run: {}",
                "icectl optimize data --partition <partition> --dry-run".dimmed()
            );
            println!();
        }

        // Manifest compaction
        if manifest.needs_action() {
            has_recommendations = true;
            println!(
                "  {} {}",
                "!".yellow(),
                "Manifest compaction recommended".yellow().bold()
            );
            println!(
                "    Current manifests: {} (recommended: < {})",
                manifest.total_manifests.to_string().cyan(),
                manifest.recommended_max
            );
            println!(
                "    Run: {}",
                "icectl optimize manifests --dry-run".dimmed()
            );
            println!();
        }

        // Snapshot expiration
        if snapshot.needs_action() {
            has_recommendations = true;
            println!(
                "  {} {}",
                "!".yellow(),
                "Old snapshots can be expired".yellow().bold()
            );
            println!(
                "    Total snapshots: {}",
                snapshot.total_snapshots.to_string().cyan()
            );
            println!(
                "    Older than 7 days: {}",
                snapshot.snapshots_older_than_7d.to_string().cyan()
            );
            if snapshot.snapshots_older_than_30d > 0 {
                println!(
                    "    Older than 30 days: {}",
                    snapshot.snapshots_older_than_30d.to_string().cyan()
                );
            }
            println!(
                "    Oldest snapshot: {} days old",
                snapshot.oldest_snapshot_age_days.to_string().cyan()
            );
            println!(
                "    Run: {}",
                "icectl snapshot expire --older-than 7d --dry-run".dimmed()
            );
            println!();
        }

        // Orphan files (if checked)
        if let Some(orphan) = orphan {
            if orphan.has_orphan_files() {
                has_recommendations = true;
                println!(
                    "  {} {}",
                    "!".yellow(),
                    "Orphan files detected".yellow().bold()
                );
                println!(
                    "    Orphan files: {} ({})",
                    orphan.orphan_count.to_string().cyan(),
                    format_bytes(orphan.orphan_size)
                );
                println!("    Run: {}", "icectl vacuum --dry-run".dimmed());
                println!();
            }

            if orphan.has_missing_files() {
                has_recommendations = true;
                println!("  {} {}", "x".red(), "Missing files detected".red().bold());
                println!(
                    "    Missing files: {}",
                    orphan.missing_count.to_string().red()
                );
                println!(
                    "    Run: {}",
                    "icectl repair --remove-missing --dry-run".dimmed()
                );
                println!();
            }

            if !orphan.needs_action() {
                println!("  {} {}", "+".green(), "No orphan or missing files".green());
                println!();
            }
        }

        if !has_recommendations {
            println!("  {} {}", "+".green(), "Table is healthy!".green());
            println!();
        }

        // Summary stats
        println!("{}", "Summary:".bold());
        println!(
            "  Data files:  {} ({})",
            data.total_files.to_string().cyan(),
            format_bytes(data.total_size)
        );
        println!(
            "  Manifests:   {}",
            manifest.total_manifests.to_string().cyan()
        );
        println!(
            "  Snapshots:   {}",
            snapshot.total_snapshots.to_string().cyan()
        );

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

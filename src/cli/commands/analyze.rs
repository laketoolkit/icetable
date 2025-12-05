//! Analyze command implementation
//!
//! Analyzes table health and provides optimization recommendations.

use colored::Colorize;
use serde::Serialize;
use std::collections::HashMap;

use crate::cli::parser::AnalyzeArgs;
use crate::config::ResolvePath;
use crate::core::maintenance::group_files_by_partition;
use crate::core::metadata::IcebergMetadataService;
use crate::core::utils::format_bytes;
use crate::core::{TableFormat, detect_table_format_async};
use crate::error::{Error, Result};

/// Extract partition key=value pairs from a file path
/// e.g., "s3://bucket/data/day=2024-01-01/currency=USD/file.parquet" -> {"day": "2024-01-01", "currency": "USD"}
fn extract_partition_from_path(path: &str) -> HashMap<String, String> {
    let mut partition = HashMap::new();

    for segment in path.split('/') {
        if let Some(idx) = segment.find('=') {
            let key = &segment[..idx];
            let value = &segment[idx + 1..];
            // Skip if it looks like a file, not a partition
            if !value.contains('.') {
                partition.insert(key.to_string(), value.to_string());
            }
        }
    }

    partition
}

/// Partition compaction info for detailed reporting
#[derive(Debug, Clone, Serialize)]
struct PartitionCompactionInfo {
    partition: String,
    files: usize,
    small_files: usize,
    size_bytes: u64,
    records: u64,
    priority: String,
    /// Score used for sorting (higher = more urgent)
    #[serde(skip_serializing)]
    priority_score: u64,
}

/// Data compaction analysis results
#[derive(Debug)]
struct DataCompactionAnalysis {
    total_files: usize,
    small_files: usize,
    groups_needing_compaction: usize,
    total_size: u64,
    small_files_size: u64,
    min_size_threshold: u64,
    partitions: Vec<PartitionCompactionInfo>,
}

/// Manifest compaction analysis results
#[derive(Debug)]
struct ManifestCompactionAnalysis {
    total_manifests: usize,
    recommended_max: usize,
}

/// Snapshot expiration analysis results
#[derive(Debug)]
struct SnapshotExpirationAnalysis {
    total_snapshots: usize,
    snapshots_older_than_7d: usize,
    snapshots_older_than_30d: usize,
    oldest_snapshot_age_days: i64,
}

/// Orphan files analysis results
#[derive(Debug)]
struct OrphanFilesAnalysis {
    orphan_count: usize,
    orphan_size: u64,
    missing_count: usize,
}

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
        let (metadata, _) = service.load_metadata().await?;

        // Analyze data files for compaction (with progress bar)
        let data_analysis =
            Self::analyze_data_compaction_with_progress(&service, args.min_file_size).await?;

        // Analyze manifests (with progress bar)
        let manifest_analysis = Self::analyze_manifests_with_progress(&metadata, &service).await?;

        // Analyze snapshots (instant, no bar needed - just in-memory iteration)
        let snapshot_analysis = Self::analyze_snapshots(&metadata);

        // Analyze orphan files (default, can be skipped with --skip-orphans)
        let orphan_analysis = if !args.skip_orphans {
            let result = Self::analyze_orphans_with_progress(&service).await?;
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

    /// Analyze data compaction with progress bar
    async fn analyze_data_compaction_with_progress(
        service: &IcebergMetadataService,
        min_size: u64,
    ) -> Result<DataCompactionAnalysis> {
        use crate::core::metadata::DataFileInfo;
        use iceberg::spec::{ManifestContentType, ManifestStatus};
        use indicatif::{ProgressBar, ProgressStyle};
        use std::collections::HashSet;

        let (metadata, _) = service.load_metadata().await?;
        let file_io = service.file_io();

        // Get current snapshot
        let current_snapshot = match metadata.current_snapshot() {
            Some(s) => s,
            None => {
                return Ok(DataCompactionAnalysis {
                    total_files: 0,
                    small_files: 0,
                    groups_needing_compaction: 0,
                    total_size: 0,
                    small_files_size: 0,
                    min_size_threshold: min_size,
                    partitions: Vec::new(),
                });
            }
        };

        // Load manifest list
        let manifest_list = current_snapshot
            .load_manifest_list(file_io, &metadata)
            .await
            .map_err(|e| Error::General(format!("Failed to load manifest list: {}", e)))?;

        let data_manifests: Vec<_> = manifest_list
            .entries()
            .iter()
            .filter(|e| e.content == ManifestContentType::Data)
            .collect();

        // Progress spinner for analyzing current snapshot
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} Analyzing current snapshot...")
                .unwrap(),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        // Read all data files
        let mut seen_paths: HashSet<String> = HashSet::new();
        let mut deleted_paths: HashSet<String> = HashSet::new();
        let mut files: Vec<DataFileInfo> = Vec::new();

        // First pass: collect deleted paths
        for manifest_entry in &data_manifests {
            if let Ok(manifest) = manifest_entry.load_manifest(file_io).await {
                for entry in manifest.entries() {
                    if entry.status() == ManifestStatus::Deleted {
                        deleted_paths.insert(entry.data_file().file_path().to_string());
                    }
                }
            }
        }

        // Second pass: collect alive files
        for manifest_entry in &data_manifests {
            if let Ok(manifest) = manifest_entry.load_manifest(file_io).await {
                for entry in manifest.entries() {
                    if entry.status() == ManifestStatus::Deleted {
                        continue;
                    }
                    let data_file = entry.data_file();
                    let path = data_file.file_path().to_string();

                    if deleted_paths.contains(&path) || seen_paths.contains(&path) {
                        continue;
                    }
                    seen_paths.insert(path.clone());

                    let partition = extract_partition_from_path(&path);
                    files.push(DataFileInfo {
                        path,
                        size: data_file.file_size_in_bytes(),
                        record_count: data_file.record_count(),
                        partition,
                    });
                }
            }
        }
        pb.finish_and_clear();

        // Calculate statistics
        let total_files = files.len();
        let total_size: u64 = files.iter().map(|f| f.size).sum();

        let small_files: Vec<_> = files.iter().filter(|f| f.size < min_size).collect();
        let small_files_count = small_files.len();
        let small_files_size: u64 = small_files.iter().map(|f| f.size).sum();

        let groups = group_files_by_partition(files);

        // Collect partition-level details
        let mut partitions: Vec<PartitionCompactionInfo> = groups
            .iter()
            .filter(|(_, g)| g.needs_compaction(min_size))
            .map(|(key, g)| {
                let small_count = g.files.iter().filter(|f| f.size < min_size).count();
                let total_size: u64 = g.files.iter().map(|f| f.size).sum();
                let total_records: u64 = g.files.iter().map(|f| f.record_count).sum();

                // Priority score based on:
                // 1. Reduction ratio (files → 1): more files = more benefit
                // 2. Number of records: more records = more query impact
                // Score = files * log2(records + 1) to balance both factors
                let reduction_ratio = g.files.len() as u64;
                let records_factor = ((total_records + 1) as f64).log2() as u64;
                let priority_score = reduction_ratio * records_factor.max(1);

                PartitionCompactionInfo {
                    partition: if key.is_empty() {
                        "(unpartitioned)".to_string()
                    } else {
                        key.clone()
                    },
                    files: g.files.len(),
                    small_files: small_count,
                    size_bytes: total_size,
                    records: total_records,
                    priority: String::new(), // Will be set after sorting
                    priority_score,
                }
            })
            .collect();

        // Sort by priority_score descending (highest priority first)
        partitions.sort_by(|a, b| b.priority_score.cmp(&a.priority_score));

        // Assign priority labels based on percentiles
        let total = partitions.len();
        for (i, p) in partitions.iter_mut().enumerate() {
            let percentile = (i as f64) / (total.max(1) as f64);
            p.priority = if percentile < 0.1 {
                "high".to_string() // Top 10%
            } else if percentile < 0.4 {
                "medium".to_string() // Next 30%
            } else {
                "low".to_string() // Bottom 60%
            };
        }

        let groups_needing_compaction = partitions.len();

        Ok(DataCompactionAnalysis {
            total_files,
            small_files: small_files_count,
            groups_needing_compaction,
            total_size,
            small_files_size,
            min_size_threshold: min_size,
            partitions,
        })
    }

    /// Analyze manifests (instant - just counts entries)
    async fn analyze_manifests_with_progress(
        metadata: &std::sync::Arc<iceberg::spec::TableMetadata>,
        service: &IcebergMetadataService,
    ) -> Result<ManifestCompactionAnalysis> {
        let current_snapshot = match metadata.current_snapshot() {
            Some(s) => s,
            None => {
                return Ok(ManifestCompactionAnalysis {
                    total_manifests: 0,
                    recommended_max: 10,
                });
            }
        };

        let manifest_list = current_snapshot
            .load_manifest_list(service.file_io(), metadata)
            .await
            .map_err(|e| Error::General(format!("Failed to load manifest list: {}", e)))?;

        let total_manifests = manifest_list.entries().len();

        Ok(ManifestCompactionAnalysis {
            total_manifests,
            recommended_max: 10,
        })
    }

    fn analyze_snapshots(
        metadata: &std::sync::Arc<iceberg::spec::TableMetadata>,
    ) -> SnapshotExpirationAnalysis {
        let now = chrono::Utc::now().timestamp_millis();
        let day_ms: i64 = 24 * 60 * 60 * 1000;

        let snapshots: Vec<_> = metadata.snapshots().collect();
        let total_snapshots = snapshots.len();

        let mut oldest_age_days: i64 = 0;
        let mut older_than_7d = 0;
        let mut older_than_30d = 0;

        for snapshot in &snapshots {
            let age_ms = now - snapshot.timestamp_ms();
            let age_days = age_ms / day_ms;

            if age_days > oldest_age_days {
                oldest_age_days = age_days;
            }

            if age_days >= 7 {
                older_than_7d += 1;
            }
            if age_days >= 30 {
                older_than_30d += 1;
            }
        }

        SnapshotExpirationAnalysis {
            total_snapshots,
            snapshots_older_than_7d: older_than_7d,
            snapshots_older_than_30d: older_than_30d,
            oldest_snapshot_age_days: oldest_age_days,
        }
    }

    /// Analyze orphan files with progress bar for manifest scanning
    async fn analyze_orphans_with_progress(
        service: &IcebergMetadataService,
    ) -> Result<OrphanFilesAnalysis> {
        use crate::core::metadata::DataFileInfo;
        use crate::core::storage::traits::ListOptions;
        use iceberg::spec::{ManifestContentType, ManifestList, ManifestStatus};
        use indicatif::{ProgressBar, ProgressStyle};
        use std::collections::HashSet;

        let (metadata, _) = service.load_metadata().await?;
        let file_io = service.file_io();

        // Count snapshots for progress bar
        let snapshots: Vec<_> = metadata.snapshots().collect();
        let snapshot_count = snapshots.len();

        // Progress bar by snapshot (what the user understands)
        let pb = ProgressBar::new(snapshot_count as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.cyan} Scanning all snapshots {bar:20.dim.white/dim} {pos}/{len}",
                )
                .unwrap()
                .progress_chars("━━╺"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        // Collect all referenced files from all snapshots
        let mut seen_manifest_paths: HashSet<String> = HashSet::new();
        let mut referenced: HashSet<String> = HashSet::new();

        for snapshot in &snapshots {
            let manifest_list_path = snapshot.manifest_list();
            let manifest_list_content = match file_io
                .new_input(manifest_list_path)
                .map_err(|e| Error::General(format!("Failed to open manifest list: {}", e)))?
                .read()
                .await
            {
                Ok(content) => content,
                Err(_) => {
                    pb.inc(1);
                    continue;
                }
            };

            let manifest_list = match ManifestList::parse_with_version(
                &manifest_list_content,
                metadata.format_version(),
            ) {
                Ok(ml) => ml,
                Err(_) => {
                    pb.inc(1);
                    continue;
                }
            };

            // Read manifests for this snapshot (skip already seen)
            for entry in manifest_list.entries() {
                if entry.content == ManifestContentType::Data {
                    if seen_manifest_paths.contains(&entry.manifest_path) {
                        continue;
                    }
                    seen_manifest_paths.insert(entry.manifest_path.clone());

                    if let Ok(manifest) = entry.load_manifest(file_io).await {
                        for file_entry in manifest.entries() {
                            if file_entry.status() != ManifestStatus::Deleted {
                                referenced.insert(file_entry.data_file().file_path().to_string());
                            }
                        }
                    }
                }
            }
            pb.inc(1);
        }
        pb.finish_and_clear();

        // List files on storage with counter
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} Listing storage files... {msg}")
                .unwrap(),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let table_path = service.table_path();
        let data_prefix = format!("{}/data/", table_path.trim_end_matches('/'));
        let storage = service.storage();

        let list_opts = ListOptions {
            prefix: Some(data_prefix),
            delimiter: None,
            max_results: None,
            continuation_token: None,
        };
        let result = storage.list(&list_opts).await?;

        let on_storage: Vec<DataFileInfo> = result
            .objects
            .iter()
            .filter(|obj| obj.path.ends_with(".parquet"))
            .map(|obj| DataFileInfo {
                path: obj.path.clone(),
                size: obj.size,
                record_count: 0,
                partition: std::collections::HashMap::new(),
            })
            .collect();

        pb.set_message(format!("{} found", on_storage.len()));
        pb.finish_and_clear();

        // Calculate orphans and missing
        let mut orphan_count = 0;
        let mut orphan_size = 0u64;
        let mut missing_count = 0;

        for file in &on_storage {
            if !referenced.contains(&file.path) {
                orphan_count += 1;
                orphan_size += file.size;
            }
        }

        let storage_paths: HashSet<_> = on_storage.iter().map(|f| &f.path).collect();
        for path in &referenced {
            if !storage_paths.contains(path) {
                missing_count += 1;
            }
        }

        Ok(OrphanFilesAnalysis {
            orphan_count,
            orphan_size,
            missing_count,
        })
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
        if data.groups_needing_compaction > 0 {
            has_recommendations = true;
            println!(
                "  {} {}",
                "⚠".yellow(),
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
        if manifest.total_manifests > manifest.recommended_max {
            has_recommendations = true;
            println!(
                "  {} {}",
                "⚠".yellow(),
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
        if snapshot.snapshots_older_than_7d > 0 {
            has_recommendations = true;
            println!(
                "  {} {}",
                "⚠".yellow(),
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
            if orphan.orphan_count > 0 {
                has_recommendations = true;
                println!(
                    "  {} {}",
                    "⚠".yellow(),
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

            if orphan.missing_count > 0 {
                has_recommendations = true;
                println!("  {} {}", "✗".red(), "Missing files detected".red().bold());
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

            if orphan.orphan_count == 0 && orphan.missing_count == 0 {
                println!("  {} {}", "✓".green(), "No orphan or missing files".green());
                println!();
            }
        }

        if !has_recommendations {
            println!("  {} {}", "✓".green(), "Table is healthy!".green());
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
                "needs_action": data.groups_needing_compaction > 0,
                "partitions": data.partitions,
            },
            "manifest_compaction": {
                "total_manifests": manifest.total_manifests,
                "recommended_max": manifest.recommended_max,
                "needs_action": manifest.total_manifests > manifest.recommended_max,
            },
            "snapshot_expiration": {
                "total_snapshots": snapshot.total_snapshots,
                "older_than_7_days": snapshot.snapshots_older_than_7d,
                "older_than_30_days": snapshot.snapshots_older_than_30d,
                "oldest_age_days": snapshot.oldest_snapshot_age_days,
                "needs_action": snapshot.snapshots_older_than_7d > 0,
            },
            "orphan_files": orphan.as_ref().map(|o| serde_json::json!({
                "orphan_count": o.orphan_count,
                "orphan_size_bytes": o.orphan_size,
                "missing_count": o.missing_count,
                "needs_action": o.orphan_count > 0 || o.missing_count > 0,
            })),
        });

        println!(
            "{}",
            serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
        );

        Ok(())
    }
}

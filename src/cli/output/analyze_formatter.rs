//! Analyze command formatting utilities

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::formatter::create_styled_table;
use crate::core::analysis::{
    DataCompactionAnalysis, ManifestCompactionAnalysis, OrphanFilesAnalysis,
    SnapshotExpirationAnalysis,
};
use crate::core::{format_bytes, format_count};

/// Formatter for analyze command results
pub struct AnalyzeFormatter;

impl AnalyzeFormatter {
    /// Format analysis results as a table
    pub fn format_table(
        data: &DataCompactionAnalysis,
        manifest: &ManifestCompactionAnalysis,
        snapshot: &SnapshotExpirationAnalysis,
        orphan: Option<&OrphanFilesAnalysis>,
        verbose: bool,
    ) -> String {
        let mut output = Vec::new();

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

        output.push(table.to_string());
        output.push(String::new());

        // Collect recommendations
        let recommendations = Self::build_recommendations(data, manifest, snapshot, orphan);

        // Print recommendations
        if recommendations.is_empty() {
            output.push(format!("{}", "✓ Table is healthy!".green().bold()));
        } else {
            output.push(format!("{}", "Recommendations:".bold()));
            for (detail, command) in &recommendations {
                output.push(format!("  {} {}", "⚠".yellow(), detail.yellow()));
                output.push(format!("    → {}", command.dimmed()));
            }
        }

        // Verbose: show partition details for data compaction
        if verbose && data.needs_action() && !data.partitions.is_empty() {
            output.push(String::new());
            output.push(format!("{}", "Top partitions by priority:".dimmed()));
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
                output.push(format!(
                    "  {}. {} {} files, {} ({}) [{}]",
                    i + 1,
                    p.partition.cyan(),
                    p.files.to_string().white(),
                    records_str,
                    format_bytes(p.size_bytes),
                    priority_color
                ));
            }
            if data.partitions.len() > max_show {
                output.push(format!(
                    "  {} ({} more)",
                    "...".dimmed(),
                    data.partitions.len() - max_show
                ));
            }
        }

        output.join("\n")
    }

    /// Format analysis results as JSON
    pub fn format_json(
        table_path: &str,
        data: &DataCompactionAnalysis,
        manifest: &ManifestCompactionAnalysis,
        snapshot: &SnapshotExpirationAnalysis,
        orphan: Option<&OrphanFilesAnalysis>,
    ) -> Result<String, serde_json::Error> {
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
            "orphan_files": orphan.map(|o| serde_json::json!({
                "orphan_count": o.orphan_count,
                "orphan_size_bytes": o.orphan_size,
                "missing_count": o.missing_count,
                "needs_action": o.needs_action(),
            })),
        });

        serde_json::to_string_pretty(&json)
    }

    /// Build recommendations based on analysis results
    fn build_recommendations(
        data: &DataCompactionAnalysis,
        manifest: &ManifestCompactionAnalysis,
        snapshot: &SnapshotExpirationAnalysis,
        orphan: Option<&OrphanFilesAnalysis>,
    ) -> Vec<(String, String)> {
        let mut recommendations = Vec::new();

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

        recommendations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::analysis::PartitionCompactionInfo;

    fn sample_data_analysis() -> DataCompactionAnalysis {
        DataCompactionAnalysis {
            total_files: 100,
            small_files: 20,
            total_size: 1_000_000_000,
            small_files_size: 50_000_000,
            min_size_threshold: 128_000_000,
            groups_needing_compaction: 5,
            partitions: vec![PartitionCompactionInfo {
                partition: "date=2024-01-01".to_string(),
                files: 10,
                small_files: 8,
                records: 50000,
                size_bytes: 25_000_000,
                priority: "high".to_string(),
                priority_score: 100,
            }],
        }
    }

    fn sample_manifest_analysis() -> ManifestCompactionAnalysis {
        ManifestCompactionAnalysis {
            total_manifests: 50,
            recommended_max: 10,
        }
    }

    fn sample_snapshot_analysis() -> SnapshotExpirationAnalysis {
        SnapshotExpirationAnalysis {
            total_snapshots: 100,
            snapshots_older_than_7d: 80,
            snapshots_older_than_30d: 50,
            oldest_snapshot_age_days: 90,
        }
    }

    fn sample_orphan_analysis() -> OrphanFilesAnalysis {
        OrphanFilesAnalysis {
            orphan_count: 5,
            orphan_size: 500_000_000,
            missing_count: 0,
        }
    }

    #[test]
    fn test_format_table_healthy() {
        let data = DataCompactionAnalysis {
            total_files: 10,
            small_files: 0,
            total_size: 1_000_000_000,
            small_files_size: 0,
            min_size_threshold: 128_000_000,
            groups_needing_compaction: 0,
            partitions: vec![],
        };
        let manifest = ManifestCompactionAnalysis {
            total_manifests: 5,
            recommended_max: 10,
        };
        let snapshot = SnapshotExpirationAnalysis {
            total_snapshots: 3,
            snapshots_older_than_7d: 0,
            snapshots_older_than_30d: 0,
            oldest_snapshot_age_days: 2,
        };

        let result = AnalyzeFormatter::format_table(&data, &manifest, &snapshot, None, false);
        assert!(result.contains("Table is healthy"));
    }

    #[test]
    fn test_format_table_with_recommendations() {
        let data = sample_data_analysis();
        let manifest = sample_manifest_analysis();
        let snapshot = sample_snapshot_analysis();

        let result = AnalyzeFormatter::format_table(&data, &manifest, &snapshot, None, false);
        assert!(result.contains("Recommendations"));
        assert!(result.contains("icetable optimize data"));
        assert!(result.contains("icetable optimize manifests"));
        assert!(result.contains("icetable snapshot expire"));
    }

    #[test]
    fn test_format_table_with_orphans() {
        let data = sample_data_analysis();
        let manifest = sample_manifest_analysis();
        let snapshot = sample_snapshot_analysis();
        let orphan = sample_orphan_analysis();

        let result =
            AnalyzeFormatter::format_table(&data, &manifest, &snapshot, Some(&orphan), false);
        assert!(result.contains("Orphans"));
        assert!(result.contains("icetable vacuum"));
    }

    #[test]
    fn test_format_table_verbose() {
        let data = sample_data_analysis();
        let manifest = sample_manifest_analysis();
        let snapshot = sample_snapshot_analysis();

        let result = AnalyzeFormatter::format_table(&data, &manifest, &snapshot, None, true);
        assert!(result.contains("Top partitions by priority"));
        assert!(result.contains("date=2024-01-01"));
    }

    #[test]
    fn test_format_json() {
        let data = sample_data_analysis();
        let manifest = sample_manifest_analysis();
        let snapshot = sample_snapshot_analysis();

        let result =
            AnalyzeFormatter::format_json("s3://bucket/table", &data, &manifest, &snapshot, None)
                .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["table_path"], "s3://bucket/table");
        assert_eq!(parsed["data_compaction"]["total_files"], 100);
        assert_eq!(parsed["manifest_compaction"]["total_manifests"], 50);
        assert_eq!(parsed["snapshot_expiration"]["total_snapshots"], 100);
    }

    #[test]
    fn test_format_json_with_orphans() {
        let data = sample_data_analysis();
        let manifest = sample_manifest_analysis();
        let snapshot = sample_snapshot_analysis();
        let orphan = sample_orphan_analysis();

        let result = AnalyzeFormatter::format_json(
            "s3://bucket/table",
            &data,
            &manifest,
            &snapshot,
            Some(&orphan),
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["orphan_files"]["orphan_count"], 5);
    }

    #[test]
    fn test_build_recommendations_empty() {
        let data = DataCompactionAnalysis {
            total_files: 10,
            small_files: 0,
            total_size: 1_000_000_000,
            small_files_size: 0,
            min_size_threshold: 128_000_000,
            groups_needing_compaction: 0,
            partitions: vec![],
        };
        let manifest = ManifestCompactionAnalysis {
            total_manifests: 5,
            recommended_max: 10,
        };
        let snapshot = SnapshotExpirationAnalysis {
            total_snapshots: 3,
            snapshots_older_than_7d: 0,
            snapshots_older_than_30d: 0,
            oldest_snapshot_age_days: 2,
        };

        let recommendations =
            AnalyzeFormatter::build_recommendations(&data, &manifest, &snapshot, None);
        assert!(recommendations.is_empty());
    }
}

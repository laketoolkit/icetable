//! Optimize command formatting utilities

use colored::Colorize;

use crate::core::format_bytes;
use crate::core::maintenance::{ManifestAnalysis, ManifestRewriteResult};
use crate::core::metadata::MaintenanceResult;

/// Formatter for optimize command results
pub struct OptimizeFormatter;

impl OptimizeFormatter {
    /// Format optimize data header
    pub fn format_data_header(table_path: &str, branch: Option<&str>) -> String {
        if let Some(b) = branch {
            format!(
                "{} Iceberg table at {} (branch: {})",
                "Optimizing".green(),
                table_path,
                b.cyan()
            )
        } else {
            format!("{} Iceberg table at {}", "Optimizing".green(), table_path)
        }
    }

    /// Format manifest rewrite header
    pub fn format_manifests_header(table_path: &str, branch: Option<&str>) -> String {
        if let Some(b) = branch {
            format!(
                "{} Iceberg manifests at {} (branch: {})",
                "Rewriting".green(),
                table_path,
                b.cyan()
            )
        } else {
            format!(
                "{} Iceberg manifests at {}",
                "Rewriting".green(),
                table_path
            )
        }
    }

    /// Format data optimization result as JSON
    pub fn format_data_result_json(
        result: &MaintenanceResult,
    ) -> Result<String, serde_json::Error> {
        let is_dry_run = result.operation.contains("dry-run")
            || result
                .details
                .get("mode")
                .map(|m| m == "dry-run")
                .unwrap_or(false);

        let json = serde_json::json!({
            "dry_run": is_dry_run,
            "operation": result.operation,
            "files_added": result.files_added,
            "files_removed": result.files_removed,
            "bytes_added": result.bytes_added,
            "bytes_removed": result.bytes_removed,
            "records_affected": result.records_affected,
            "details": result.details,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format data optimization result as text
    pub fn format_data_result_text(result: &MaintenanceResult) -> String {
        let mut output = Vec::new();
        let is_dry_run = result.operation.contains("dry-run")
            || result
                .details
                .get("mode")
                .map(|m| m == "dry-run")
                .unwrap_or(false);

        output.push(String::new());

        if is_dry_run {
            output.push("DRY RUN - No changes made".yellow().bold().to_string());
            output.push(String::new());

            if result.files_added == 0 && result.files_removed == 0 {
                output.push("Table is already optimized.".green().to_string());
                if let Some(reason) = result.details.get("reason") {
                    output.push(reason.clone());
                }
            } else {
                output.push("Would perform the following changes:".cyan().to_string());
                output.push(String::new());
                output.push(format!(
                    "  Files to compact:  {} -> {}",
                    result.files_removed.to_string().yellow(),
                    result.files_added.to_string().yellow()
                ));

                if let Some(partitions) = result.details.get("partitions") {
                    output.push(format!("  Partitions:        {}", partitions.yellow()));
                }

                if let Some(would_compact) = result.details.get("would_compact") {
                    output.push(format!("  Summary:           {}", would_compact.yellow()));
                }

                output.push(String::new());
                output.push(
                    "Run without --dry-run to apply these changes."
                        .dimmed()
                        .to_string(),
                );
            }
        } else if result.files_added == 0 && result.files_removed == 0 {
            output.push("Table is already optimized.".green().to_string());
            if let Some(reason) = result.details.get("reason") {
                output.push(reason.clone());
            }
        } else {
            output.push("Compaction complete!".green().bold().to_string());
            output.push(String::new());
            output.push(format!(
                "Files compacted:  {} -> {}",
                result.files_removed.to_string().cyan(),
                result.files_added.to_string().cyan()
            ));
            output.push(format!(
                "Bytes saved:      {}",
                format_bytes(result.bytes_removed.saturating_sub(result.bytes_added))
            ));
            output.push(format!(
                "Records affected: {}",
                result.records_affected.to_string().cyan()
            ));

            if let Some(snapshot_id) = result.details.get("snapshot_id") {
                output.push(format!("Snapshot:         {}", snapshot_id.cyan()));
            }
        }

        output.join("\n")
    }

    /// Format manifest analysis (dry-run)
    pub fn format_manifest_analysis(analysis: &ManifestAnalysis) -> String {
        let mut output = Vec::new();

        if !analysis.should_rewrite {
            output.push(String::new());
            output.push(format!(
                "{} {}",
                "Skipping:".yellow(),
                analysis
                    .skip_reason
                    .as_deref()
                    .unwrap_or("No rewrite needed")
            ));
            return output.join("\n");
        }

        output.push(format!(
            "Current manifests: {}",
            analysis.current_manifests.to_string().cyan()
        ));
        output.push(format!(
            "  Data manifests:   {}",
            analysis.data_manifests.to_string().cyan()
        ));
        output.push(format!(
            "  Delete manifests: {}",
            analysis.delete_manifests.to_string().cyan()
        ));
        output.push(String::new());
        output.push("DRY RUN - No changes made".yellow().bold().to_string());
        output.push(String::new());
        output.push(format!(
            "Total data entries: {}",
            analysis.total_entries.to_string().cyan()
        ));
        output.push(format!(
            "Would rewrite into: {} manifests",
            analysis.estimated_after.to_string().cyan()
        ));

        output.join("\n")
    }

    /// Format manifest analysis as JSON
    pub fn format_manifest_analysis_json(
        analysis: &ManifestAnalysis,
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "dry_run": true,
            "current_manifests": analysis.current_manifests,
            "data_manifests": analysis.data_manifests,
            "delete_manifests": analysis.delete_manifests,
            "total_entries": analysis.total_entries,
            "estimated_after": analysis.estimated_after,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format manifest rewrite result
    pub fn format_manifest_result(result: &ManifestRewriteResult) -> String {
        let mut output = Vec::new();
        output.push(String::new());
        output.push(
            "Manifests rewritten successfully!"
                .green()
                .bold()
                .to_string(),
        );
        output.push(format!(
            "Manifests: {} -> {}",
            result.previous_manifests.to_string().cyan(),
            result.new_manifests.to_string().cyan()
        ));
        output.push(format!(
            "Snapshot:  {}",
            result.snapshot_id.to_string().cyan()
        ));
        output.push(format!(
            "Version:   {}",
            result.metadata_version.to_string().cyan()
        ));
        output.join("\n")
    }

    /// Format manifest rewrite result as JSON
    pub fn format_manifest_result_json(
        result: &ManifestRewriteResult,
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "previous_manifests": result.previous_manifests,
            "new_manifests": result.new_manifests,
            "data_manifests_rewritten": result.data_manifests_rewritten,
            "delete_manifests_kept": result.delete_manifests_kept,
            "total_entries": result.total_entries,
            "snapshot_id": result.snapshot_id,
            "metadata_version": result.metadata_version,
        });
        serde_json::to_string_pretty(&json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_result() -> MaintenanceResult {
        let mut result = MaintenanceResult {
            operation: "optimize".to_string(),
            files_added: 5,
            files_removed: 20,
            bytes_added: 1024 * 1024 * 500,
            bytes_removed: 1024 * 1024 * 600,
            records_affected: 10000,
            details: HashMap::new(),
        };
        result
            .details
            .insert("snapshot_id".to_string(), "123456789".to_string());
        result
    }

    #[test]
    fn test_format_data_header() {
        let header = OptimizeFormatter::format_data_header("s3://bucket/table", None);
        assert!(header.contains("Optimizing"));
        assert!(header.contains("s3://bucket/table"));

        let header = OptimizeFormatter::format_data_header("s3://bucket/table", Some("develop"));
        assert!(header.contains("branch:"));
        assert!(header.contains("develop"));
    }

    #[test]
    fn test_format_manifests_header() {
        let header = OptimizeFormatter::format_manifests_header("s3://bucket/table", None);
        assert!(header.contains("Rewriting"));

        let header = OptimizeFormatter::format_manifests_header("s3://bucket/table", Some("main"));
        assert!(header.contains("branch:"));
    }

    #[test]
    fn test_format_data_result_json() {
        let result = create_test_result();
        let json = OptimizeFormatter::format_data_result_json(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["files_added"], 5);
        assert_eq!(parsed["files_removed"], 20);
    }

    #[test]
    fn test_format_data_result_text_with_changes() {
        let result = create_test_result();
        let text = OptimizeFormatter::format_data_result_text(&result);

        assert!(text.contains("Compaction complete"));
        assert!(text.contains("Files compacted"));
        assert!(text.contains("Bytes saved"));
    }

    #[test]
    fn test_format_data_result_text_dry_run() {
        let mut result = create_test_result();
        result.operation = "optimize dry-run".to_string();
        let text = OptimizeFormatter::format_data_result_text(&result);

        assert!(text.contains("DRY RUN"));
        assert!(text.contains("Would perform"));
    }

    #[test]
    fn test_format_data_result_text_already_optimized() {
        let mut result = create_test_result();
        result.files_added = 0;
        result.files_removed = 0;
        let text = OptimizeFormatter::format_data_result_text(&result);

        assert!(text.contains("already optimized"));
    }

    #[test]
    fn test_format_manifest_analysis() {
        let analysis = ManifestAnalysis {
            current_manifests: 10,
            data_manifests: 8,
            delete_manifests: 2,
            total_entries: 1000,
            estimated_after: 3,
            should_rewrite: true,
            skip_reason: None,
        };
        let text = OptimizeFormatter::format_manifest_analysis(&analysis);

        assert!(text.contains("Current manifests: 10"));
        assert!(text.contains("Data manifests:   8"));
        assert!(text.contains("DRY RUN"));
        assert!(text.contains("Would rewrite into: 3"));
    }

    #[test]
    fn test_format_manifest_analysis_skip() {
        let analysis = ManifestAnalysis {
            current_manifests: 2,
            data_manifests: 2,
            delete_manifests: 0,
            total_entries: 100,
            estimated_after: 2,
            should_rewrite: false,
            skip_reason: Some("Too few manifests".to_string()),
        };
        let text = OptimizeFormatter::format_manifest_analysis(&analysis);

        assert!(text.contains("Skipping:"));
        assert!(text.contains("Too few manifests"));
    }

    #[test]
    fn test_format_manifest_result() {
        let result = ManifestRewriteResult {
            previous_manifests: 10,
            new_manifests: 3,
            data_manifests_rewritten: 8,
            delete_manifests_kept: 2,
            total_entries: 1000,
            snapshot_id: 123456789,
            metadata_version: 5,
        };
        let text = OptimizeFormatter::format_manifest_result(&result);

        assert!(text.contains("Manifests rewritten"));
        assert!(text.contains("10 -> 3"));
        assert!(text.contains("123456789"));
    }
}

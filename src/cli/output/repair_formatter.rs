//! Repair command formatting utilities

use colored::Colorize;

use crate::core::extract_filename;
use crate::core::format_bytes;
use crate::core::maintenance::RepairAnalysis;
use crate::core::metadata::MaintenanceResult;

/// Formatter for repair command results
pub struct RepairFormatter;

impl RepairFormatter {
    /// Format repair analysis header
    pub fn format_analysis_header(table_path: &str, dry_run: bool) -> String {
        format!(
            "{} Iceberg table at {}",
            if dry_run { "Analyzing" } else { "Repairing" }.green(),
            table_path
        )
    }

    /// Format repair analysis summary
    pub fn format_analysis_summary(analysis: &RepairAnalysis) -> String {
        let mut output = Vec::new();
        output.push(String::new());
        output.push(format!(
            "Tracked files in metadata: {}",
            analysis.total_tracked.to_string().cyan()
        ));
        output.push(format!(
            "Parquet files on disk:     {}",
            analysis.total_on_disk.to_string().cyan()
        ));
        output.join("\n")
    }

    /// Format healthy table message
    pub fn format_healthy() -> String {
        format!("\n{}", "No issues found - table is healthy!".green())
    }

    /// Format issues found summary
    ///
    /// `completed` indicates if repair has already run (changes tense of status)
    pub fn format_issues_found(
        analysis: &RepairAnalysis,
        add_orphans: bool,
        remove_missing: bool,
        completed: bool,
    ) -> String {
        let mut output = Vec::new();
        output.push(String::new());
        output.push("Issues found:".to_string());

        if !analysis.missing_files.is_empty() {
            let status = if remove_missing {
                if completed { "fixed".green() } else { "will fix".green() }
            } else {
                "skipped".dimmed()
            };
            output.push(format!(
                "  Missing files:  {} ({}) [{}]",
                analysis.missing_files.len().to_string().red(),
                format_bytes(analysis.missing_bytes()),
                status
            ));
        }

        if !analysis.orphan_files.is_empty() {
            let status = if add_orphans {
                if completed { "fixed".green() } else { "will fix".green() }
            } else {
                "skipped".dimmed()
            };
            output.push(format!(
                "  Orphan files:   {} ({}) [{}]",
                analysis.orphan_files.len().to_string().yellow(),
                format_bytes(analysis.orphan_bytes()),
                status
            ));
        }

        output.join("\n")
    }

    /// Format dry-run details
    pub fn format_dry_run_details(
        analysis: &RepairAnalysis,
        add_orphans: bool,
        remove_missing: bool,
    ) -> String {
        let mut output = Vec::new();
        output.push(String::new());
        output.push("DRY RUN - No changes made".yellow().bold().to_string());
        output.push(String::new());

        if remove_missing {
            for file in &analysis.missing_files {
                let name = extract_filename(&file.path);
                output.push(format!("  Would remove reference: {}", name.red()));
            }
        }

        if add_orphans {
            for file in &analysis.orphan_files {
                let name = extract_filename(&file.path);
                output.push(format!(
                    "  Would add: {} ({})",
                    name.green(),
                    format_bytes(file.size)
                ));
            }
        }

        output.join("\n")
    }

    /// Format repair result
    pub fn format_result(result: &MaintenanceResult) -> String {
        let mut output = Vec::new();
        output.push(String::new());
        output.push("Repair complete!".green().bold().to_string());
        output.push(format!(
            "Removed {} missing references, added {} orphan files",
            result.files_removed, result.files_added
        ));

        if let Some(snapshot_id) = result.details.get("snapshot_id") {
            output.push(format!("New snapshot: {}", snapshot_id.cyan()));
        }

        output.join("\n")
    }

    /// Format no work message
    pub fn format_no_work() -> String {
        format!(
            "\n{}",
            "No issues match the selected repair options.".yellow()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::metadata::DataFileInfo;
    use std::collections::HashMap;

    fn create_test_analysis() -> RepairAnalysis {
        RepairAnalysis {
            total_tracked: 10,
            total_on_disk: 12,
            missing_files: vec![DataFileInfo {
                path: "s3://bucket/data/missing.parquet".to_string(),
                size: 1000,
                record_count: 100,
                partition: HashMap::new(),
            }],
            orphan_files: vec![DataFileInfo {
                path: "s3://bucket/data/orphan.parquet".to_string(),
                size: 2000,
                record_count: 200,
                partition: HashMap::new(),
            }],
        }
    }

    #[test]
    fn test_format_analysis_header() {
        let header = RepairFormatter::format_analysis_header("s3://bucket/table", true);
        assert!(header.contains("Analyzing"));

        let header = RepairFormatter::format_analysis_header("s3://bucket/table", false);
        assert!(header.contains("Repairing"));
    }

    #[test]
    fn test_format_analysis_summary() {
        let analysis = create_test_analysis();
        let summary = RepairFormatter::format_analysis_summary(&analysis);
        assert!(summary.contains("Tracked files"));
        assert!(summary.contains("10"));
        assert!(summary.contains("12"));
    }

    #[test]
    fn test_format_healthy() {
        let result = RepairFormatter::format_healthy();
        assert!(result.contains("healthy"));
    }

    #[test]
    fn test_format_issues_found_will_fix() {
        let analysis = create_test_analysis();
        let issues = RepairFormatter::format_issues_found(&analysis, true, true, false);
        assert!(issues.contains("Missing files:"));
        assert!(issues.contains("Orphan files:"));
        assert!(issues.contains("will fix"));
    }

    #[test]
    fn test_format_issues_found_fixed() {
        let analysis = create_test_analysis();
        let issues = RepairFormatter::format_issues_found(&analysis, true, true, true);
        assert!(issues.contains("Missing files:"));
        assert!(issues.contains("Orphan files:"));
        assert!(issues.contains("fixed"));
    }

    #[test]
    fn test_format_dry_run_details() {
        let analysis = create_test_analysis();
        let details = RepairFormatter::format_dry_run_details(&analysis, true, true);
        assert!(details.contains("DRY RUN"));
        assert!(details.contains("Would remove reference"));
        assert!(details.contains("Would add"));
    }

    #[test]
    fn test_format_result() {
        let mut result = MaintenanceResult {
            operation: "repair".to_string(),
            files_added: 1,
            files_removed: 1,
            bytes_added: 2000,
            bytes_removed: 1000,
            records_affected: 100,
            details: HashMap::new(),
        };
        result
            .details
            .insert("snapshot_id".to_string(), "123456".to_string());

        let formatted = RepairFormatter::format_result(&result);
        assert!(formatted.contains("Repair complete"));
        assert!(formatted.contains("123456"));
    }
}

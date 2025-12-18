//! Vacuum command formatting utilities

use colored::Colorize;

use crate::core::extract_filename;
use crate::core::format_bytes;

/// Formatter for vacuum command results
pub struct VacuumFormatter;

impl VacuumFormatter {
    /// Format header message for vacuum operation
    pub fn format_header(table_path: &str, dry_run: bool, branch: Option<&str>) -> String {
        let action = if dry_run { "Analyzing" } else { "Vacuuming" };

        match branch {
            Some(b) => {
                let mut lines = vec![format!(
                    "{} Iceberg table at {} (branch: {})",
                    action,
                    table_path,
                    b.cyan()
                )];
                lines.push(
                    "Note: Vacuum always considers all snapshots for safety"
                        .dimmed()
                        .to_string(),
                );
                lines.join("\n")
            }
            None => format!("{} Iceberg table at {}", action, table_path),
        }
    }

    /// Format analysis summary (shown for both dry-run and execution)
    pub fn format_summary(
        referenced_count: usize,
        orphan_count: usize,
        orphan_bytes: u64,
        retention_hours: u64,
    ) -> String {
        let mut lines = Vec::new();
        lines.push(String::new());
        lines.push(format!(
            "Referenced files: {}",
            referenced_count.to_string().cyan()
        ));
        lines.push(format!(
            "Files to delete:  {} ({})",
            orphan_count.to_string().cyan(),
            format_bytes(orphan_bytes)
        ));
        lines.push(format!(
            "Retention:        {} hours",
            retention_hours.to_string().cyan()
        ));
        lines.join("\n")
    }

    /// Format dry-run results as table
    pub fn format_dry_run_table(files: &[OrphanFileInfo], total_bytes: u64) -> String {
        let mut lines = vec![
            String::new(),
            "DRY RUN - No files will be deleted"
                .yellow()
                .bold()
                .to_string(),
            String::new(),
            "Would delete the following files:".cyan().to_string(),
        ];

        // Show first 10 files, then summary if more
        let show_count = 10;
        for file in files.iter().take(show_count) {
            let name = extract_filename(&file.path);
            lines.push(format!("  - {} ({})", name, format_bytes(file.size)));
        }

        if files.len() > show_count {
            lines.push(format!(
                "  {} {} more files...",
                "...and".dimmed(),
                (files.len() - show_count).to_string().dimmed()
            ));
        }

        lines.push(String::new());
        lines.push(format!(
            "Total: {} files, {} to free",
            files.len().to_string().yellow(),
            format_bytes(total_bytes).yellow()
        ));
        lines.push(String::new());
        lines.push(
            "Run without --dry-run to delete these files."
                .dimmed()
                .to_string(),
        );

        lines.join("\n")
    }

    /// Format dry-run results as JSON
    pub fn format_dry_run_json(
        files: &[OrphanFileInfo],
        total_bytes: u64,
        retention_hours: u64,
    ) -> Result<String, serde_json::Error> {
        let file_paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        let json = serde_json::json!({
            "dry_run": true,
            "files_to_delete": file_paths,
            "files_count": files.len(),
            "bytes_to_free": total_bytes,
            "retention_hours": retention_hours,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format execution results as table
    pub fn format_execution_table(
        deleted_count: usize,
        deleted_bytes: u64,
        errors: &[String],
    ) -> String {
        let mut lines = Vec::new();
        lines.push(String::new());
        lines.push(format!(
            "{} {} files, freed {}",
            "Deleted".green().bold(),
            deleted_count,
            format_bytes(deleted_bytes)
        ));

        if !errors.is_empty() {
            lines.push(format!(
                "{} errors occurred:",
                errors.len().to_string().red()
            ));
            for err in errors.iter().take(5) {
                lines.push(format!("  - {}", err));
            }
            if errors.len() > 5 {
                lines.push(format!("  ... and {} more", errors.len() - 5));
            }
        }

        lines.join("\n")
    }

    /// Format execution results as JSON
    pub fn format_execution_json(
        deleted_count: usize,
        deleted_bytes: u64,
        error_count: usize,
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "files_deleted": deleted_count,
            "bytes_freed": deleted_bytes,
            "errors": error_count,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format "no orphan files" message
    pub fn format_no_orphans() -> String {
        format!("\n{}", "No orphan files to delete".yellow())
    }
}

/// Orphan file information for formatting
#[derive(Debug, Clone)]
pub struct OrphanFileInfo {
    /// File path
    pub path: String,
    /// File size in bytes
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_files() -> Vec<OrphanFileInfo> {
        vec![
            OrphanFileInfo {
                path: "s3://bucket/data/file1.parquet".to_string(),
                size: 1000,
            },
            OrphanFileInfo {
                path: "s3://bucket/data/file2.parquet".to_string(),
                size: 2000,
            },
        ]
    }

    #[test]
    fn test_format_summary() {
        let result = VacuumFormatter::format_summary(100, 5, 5000, 168);
        assert!(result.contains("Referenced files:"));
        assert!(result.contains("100"));
        assert!(result.contains("Files to delete:"));
        assert!(result.contains("5"));
        assert!(result.contains("168 hours"));
    }

    #[test]
    fn test_format_dry_run_table() {
        let files = sample_files();
        let result = VacuumFormatter::format_dry_run_table(&files, 3000);
        assert!(result.contains("DRY RUN"));
        assert!(result.contains("file1.parquet"));
        assert!(result.contains("file2.parquet"));
        assert!(result.contains("2 files"));
    }

    #[test]
    fn test_format_dry_run_json() {
        let files = sample_files();
        let result = VacuumFormatter::format_dry_run_json(&files, 3000, 168).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["dry_run"], true);
        assert_eq!(parsed["files_count"], 2);
        assert_eq!(parsed["bytes_to_free"], 3000);
    }

    #[test]
    fn test_format_execution_table() {
        let result = VacuumFormatter::format_execution_table(10, 50000, &[]);
        assert!(result.contains("Deleted"));
        assert!(result.contains("10 files"));
    }

    #[test]
    fn test_format_execution_table_with_errors() {
        let errors = vec!["Error 1".to_string(), "Error 2".to_string()];
        let result = VacuumFormatter::format_execution_table(8, 40000, &errors);
        assert!(result.contains("2 errors"));
        assert!(result.contains("Error 1"));
    }

    #[test]
    fn test_format_execution_json() {
        let result = VacuumFormatter::format_execution_json(10, 50000, 0).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["files_deleted"], 10);
        assert_eq!(parsed["bytes_freed"], 50000);
    }

    #[test]
    fn test_format_no_orphans() {
        let result = VacuumFormatter::format_no_orphans();
        assert!(result.contains("No orphan files"));
    }
}

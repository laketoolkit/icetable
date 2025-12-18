//! Diff command formatting utilities

use colored::Colorize;

use super::formatter::format_timestamp_ms;
use crate::core::extract_filename;
use crate::core::operations::SnapshotDiffResult;

/// Formatter for diff command results
pub struct DiffFormatter;

impl DiffFormatter {
    /// Format diff result as JSON string
    pub fn format_diff_json(result: &SnapshotDiffResult) -> Result<String, serde_json::Error> {
        if result.is_identical {
            let json = serde_json::json!({
                "reference": result.reference.snapshot_id,
                "base": result.base.snapshot_id,
                "identical": true,
            });
            return serde_json::to_string_pretty(&json);
        }

        let json = serde_json::json!({
            "base": {
                "ref": result.base.label,
                "snapshot_id": result.base.snapshot_id,
                "timestamp": format_timestamp_ms(result.base.timestamp_ms),
                "manifest_count": result.base.manifest_count,
            },
            "reference": {
                "ref": result.reference.label,
                "snapshot_id": result.reference.snapshot_id,
                "timestamp": format_timestamp_ms(result.reference.timestamp_ms),
                "manifest_count": result.reference.manifest_count,
            },
            "diff": {
                "manifests_added": result.manifests_added.len(),
                "manifests_removed": result.manifests_removed.len(),
                "added_paths": result.manifests_added,
                "removed_paths": result.manifests_removed,
            }
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format diff result as text table
    pub fn format_diff_text(
        result: &SnapshotDiffResult,
        ref_label: &str,
        base_label: &str,
    ) -> String {
        let mut output = Vec::new();

        if result.is_identical {
            return "References point to the same snapshot".yellow().to_string();
        }

        output.push(format!(
            "{} {} (base: {})",
            "Comparing".green(),
            ref_label,
            base_label
        ));
        output.push(String::new());
        output.push(format!(
            "{:<20} {:<20} {:<20} {}",
            "REF".cyan(),
            "SNAPSHOT".cyan(),
            "TIMESTAMP".cyan(),
            "MANIFESTS".cyan()
        ));
        output.push("-".repeat(75));
        output.push(format!(
            "{:<20} {:<20} {:<20} {}",
            base_label,
            result.base.snapshot_id,
            format_timestamp_ms(result.base.timestamp_ms),
            result.base.manifest_count
        ));
        output.push(format!(
            "{:<20} {:<20} {:<20} {}",
            ref_label,
            result.reference.snapshot_id,
            format_timestamp_ms(result.reference.timestamp_ms),
            result.reference.manifest_count
        ));
        output.push(String::new());

        if result.manifests_added.is_empty() && result.manifests_removed.is_empty() {
            output.push("No manifest changes".yellow().to_string());
        } else {
            output.push("Changes:".to_string());
            for path in &result.manifests_added {
                let filename = extract_filename(path);
                output.push(format!("  {} {}", "+".green(), filename));
            }
            for path in &result.manifests_removed {
                let filename = extract_filename(path);
                output.push(format!("  {} {}", "-".red(), filename));
            }
            output.push(String::new());
            output.push(format!(
                "Summary: {} added, {} removed",
                result.manifests_added.len().to_string().green(),
                result.manifests_removed.len().to_string().red()
            ));
        }

        output.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::operations::SnapshotRef;

    fn create_test_diff_result(is_identical: bool) -> SnapshotDiffResult {
        SnapshotDiffResult {
            is_identical,
            base: SnapshotRef {
                label: "base".to_string(),
                snapshot_id: 1000,
                timestamp_ms: 1700000000000,
                manifest_count: 5,
            },
            reference: SnapshotRef {
                label: "current".to_string(),
                snapshot_id: 2000,
                timestamp_ms: 1700001000000,
                manifest_count: 7,
            },
            manifests_added: vec![
                "s3://bucket/table/metadata/snap-2000-m0.avro".to_string(),
                "s3://bucket/table/metadata/snap-2000-m1.avro".to_string(),
            ],
            manifests_removed: vec!["s3://bucket/table/metadata/snap-1000-m0.avro".to_string()],
        }
    }

    #[test]
    fn test_format_diff_json_identical() {
        let result = create_test_diff_result(true);
        let json = DiffFormatter::format_diff_json(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["identical"], true);
        assert_eq!(parsed["reference"], 2000);
        assert_eq!(parsed["base"], 1000);
    }

    #[test]
    fn test_format_diff_json_with_changes() {
        let result = create_test_diff_result(false);
        let json = DiffFormatter::format_diff_json(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["base"]["snapshot_id"], 1000);
        assert_eq!(parsed["reference"]["snapshot_id"], 2000);
        assert_eq!(parsed["diff"]["manifests_added"], 2);
        assert_eq!(parsed["diff"]["manifests_removed"], 1);
    }

    #[test]
    fn test_format_diff_text_identical() {
        let result = create_test_diff_result(true);
        let text = DiffFormatter::format_diff_text(&result, "current", "parent");

        assert!(text.contains("same snapshot"));
    }

    #[test]
    fn test_format_diff_text_with_changes() {
        let result = create_test_diff_result(false);
        let text = DiffFormatter::format_diff_text(&result, "current", "parent");

        assert!(text.contains("Comparing"));
        assert!(text.contains("REF"));
        assert!(text.contains("SNAPSHOT"));
        assert!(text.contains("Changes:"));
        assert!(text.contains("Summary:"));
        assert!(text.contains("2 added"));
        assert!(text.contains("1 removed"));
    }

    #[test]
    fn test_format_diff_text_no_changes() {
        let mut result = create_test_diff_result(false);
        result.manifests_added.clear();
        result.manifests_removed.clear();

        let text = DiffFormatter::format_diff_text(&result, "current", "parent");

        assert!(text.contains("No manifest changes"));
    }

    #[test]
    fn test_format_diff_text_shows_filenames() {
        let result = create_test_diff_result(false);
        let text = DiffFormatter::format_diff_text(&result, "current", "parent");

        // Should show just filename, not full path
        assert!(text.contains("snap-2000-m0.avro"));
        assert!(text.contains("snap-1000-m0.avro"));
        // Should not contain full path prefix
        assert!(!text.contains("s3://bucket/table/metadata/snap-2000-m0.avro"));
    }
}

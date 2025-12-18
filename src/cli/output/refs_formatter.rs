//! Reference (branch/tag) formatting utilities

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::formatter::create_styled_table;

/// Formatter for branch/tag reference results
pub struct RefsFormatter;

impl RefsFormatter {
    /// Format a list of references as a table
    pub fn format_list_table(
        refs: &[RefInfo],
        ref_type_name: &str,
        show_current: bool,
        current_snapshot_id: Option<i64>,
    ) -> String {
        if refs.is_empty() {
            // Handle irregular plural for "branch"
            let plural = if ref_type_name.to_lowercase() == "branch" {
                "branches".to_string()
            } else {
                format!("{}s", ref_type_name.to_lowercase())
            };
            return format!("No {} found", plural).dimmed().to_string();
        }

        let mut table = create_styled_table();

        let mut headers = vec![
            Cell::new(ref_type_name.cyan().to_string()).set_alignment(CellAlignment::Left),
            Cell::new("Snapshot ID".cyan().to_string()).set_alignment(CellAlignment::Right),
        ];
        if show_current {
            headers
                .push(Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Center));
        }
        table.set_header(headers);

        for r in refs {
            let mut row = vec![
                Cell::new(&r.name).set_alignment(CellAlignment::Left),
                Cell::new(r.snapshot_id.to_string()).set_alignment(CellAlignment::Right),
            ];
            if show_current {
                let is_main_current =
                    r.name == "main" && Some(r.snapshot_id) == current_snapshot_id;
                let status = if is_main_current {
                    "● current".green().to_string()
                } else {
                    String::new()
                };
                row.push(Cell::new(status).set_alignment(CellAlignment::Center));
            }
            table.add_row(row);
        }

        table.to_string()
    }

    /// Format a list of references as JSON
    pub fn format_list_json(
        refs: &[RefInfo],
        show_current: bool,
        current_snapshot_id: Option<i64>,
    ) -> Result<String, serde_json::Error> {
        let json_refs: Vec<serde_json::Value> = refs
            .iter()
            .map(|r| {
                let mut obj = serde_json::json!({
                    "name": r.name,
                    "snapshot_id": r.snapshot_id,
                });
                if show_current {
                    obj["is_current"] = serde_json::json!(
                        Some(r.snapshot_id) == current_snapshot_id && r.name == "main"
                    );
                }
                obj
            })
            .collect();

        serde_json::to_string_pretty(&json_refs)
    }

    /// Format a create result as JSON
    pub fn format_create_json(
        name: &str,
        snapshot_id: i64,
        new_version: Option<i64>,
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "name": name,
            "snapshot_id": snapshot_id,
            "new_version": new_version,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format a create result as table output
    pub fn format_create_table(
        ref_type: &str,
        name: &str,
        snapshot_id: i64,
        new_version: Option<i64>,
    ) -> String {
        let mut lines = vec![format!(
            "{} Created {} '{}' at snapshot {}",
            "Success:".green(),
            ref_type.to_lowercase(),
            name.cyan(),
            snapshot_id
        )];
        if let Some(version) = new_version {
            lines.push(format!(
                "  New table version: {}",
                version.to_string().dimmed()
            ));
        }
        lines.join("\n")
    }

    /// Format a delete result as JSON
    pub fn format_delete_json(
        name: &str,
        snapshot_id: i64,
        new_version: Option<i64>,
        dry_run: bool,
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "name": name,
            "snapshot_id": snapshot_id,
            "new_version": new_version,
            "dry_run": dry_run,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format a delete result as table output
    pub fn format_delete_table(ref_type: &str, name: &str, new_version: Option<i64>) -> String {
        let mut lines = vec![format!(
            "{} Deleted {} '{}'",
            "Success:".green(),
            ref_type.to_lowercase(),
            name.red()
        )];
        if let Some(version) = new_version {
            lines.push(format!(
                "  New table version: {}",
                version.to_string().dimmed()
            ));
        }
        lines.join("\n")
    }

    /// Format a delete dry run result
    pub fn format_delete_dry_run(ref_type: &str, name: &str, snapshot_id: i64) -> String {
        format!(
            "{}\nWould delete {} '{}' (currently at snapshot {})\nRun without --dry-run to apply.",
            "DRY RUN".yellow().bold(),
            ref_type.to_lowercase(),
            name.cyan(),
            snapshot_id
        )
    }

    /// Format a rename result as JSON
    pub fn format_rename_json(
        old_name: &str,
        new_name: &str,
        snapshot_id: i64,
        new_version: Option<i64>,
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "old_name": old_name,
            "new_name": new_name,
            "snapshot_id": snapshot_id,
            "new_version": new_version,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format a rename result as table output
    pub fn format_rename_table(
        ref_type: &str,
        old_name: &str,
        new_name: &str,
        new_version: Option<i64>,
    ) -> String {
        let mut lines = vec![format!(
            "{} Renamed {} '{}' to '{}'",
            "Success:".green(),
            ref_type.to_lowercase(),
            old_name.yellow(),
            new_name.cyan()
        )];
        if let Some(version) = new_version {
            lines.push(format!(
                "  New table version: {}",
                version.to_string().dimmed()
            ));
        }
        lines.join("\n")
    }

    /// Format a fast-forward result as JSON
    pub fn format_fast_forward_json(
        name: &str,
        snapshot_id: i64,
        new_version: Option<i64>,
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "name": name,
            "snapshot_id": snapshot_id,
            "new_version": new_version,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format a fast-forward result as table output
    pub fn format_fast_forward_table(
        name: &str,
        snapshot_id: i64,
        new_version: Option<i64>,
    ) -> String {
        let mut lines = vec![format!(
            "{} Fast-forwarded branch '{}' to snapshot {}",
            "Success:".green(),
            name.cyan(),
            snapshot_id
        )];
        if let Some(version) = new_version {
            lines.push(format!(
                "  New table version: {}",
                version.to_string().dimmed()
            ));
        }
        lines.join("\n")
    }
}

/// Reference information for formatting
#[derive(Debug, Clone)]
pub struct RefInfo {
    /// Reference name (branch or tag name)
    pub name: String,
    /// Snapshot ID this reference points to
    pub snapshot_id: i64,
    /// Reference type ("branch" or "tag")
    pub ref_type: String,
}

impl RefInfo {
    /// Create a new RefInfo
    pub fn new(name: String, snapshot_id: i64, ref_type: String) -> Self {
        Self {
            name,
            snapshot_id,
            ref_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_refs() -> Vec<RefInfo> {
        vec![
            RefInfo::new("main".to_string(), 1000, "branch".to_string()),
            RefInfo::new("feature".to_string(), 2000, "branch".to_string()),
        ]
    }

    #[test]
    fn test_format_list_table_empty() {
        let result = RefsFormatter::format_list_table(&[], "Branch", false, None);
        assert!(result.contains("No branches found"));
    }

    #[test]
    fn test_format_list_table_with_refs() {
        let refs = sample_refs();
        let result = RefsFormatter::format_list_table(&refs, "Branch", false, None);
        assert!(result.contains("main"));
        assert!(result.contains("feature"));
        assert!(result.contains("1000"));
        assert!(result.contains("2000"));
    }

    #[test]
    fn test_format_list_table_with_current() {
        let refs = sample_refs();
        let result = RefsFormatter::format_list_table(&refs, "Branch", true, Some(1000));
        assert!(result.contains("current"));
        assert!(result.contains("Status"));
    }

    #[test]
    fn test_format_list_json() {
        let refs = sample_refs();
        let result = RefsFormatter::format_list_json(&refs, false, None).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["name"], "main");
        assert_eq!(parsed[0]["snapshot_id"], 1000);
    }

    #[test]
    fn test_format_list_json_with_current() {
        let refs = sample_refs();
        let result = RefsFormatter::format_list_json(&refs, true, Some(1000)).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed[0]["is_current"], true);
        assert_eq!(parsed[1]["is_current"], false);
    }
}

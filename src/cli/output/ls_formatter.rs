//! List command formatting utilities

use colored::Colorize;

/// Tree drawing characters
const TREE_BRANCH: &str = "├── ";
const TREE_LAST: &str = "└── ";
const TREE_INDENT: &str = "│   ";

/// Formatter for ls command results
pub struct LsFormatter;

impl LsFormatter {
    /// Format namespaces listing as JSON
    pub fn format_namespaces_json(
        catalog: &str,
        namespaces: &[Vec<String>],
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "catalog": catalog,
            "namespaces": namespaces.iter().map(|ns| ns.join(".")).collect::<Vec<_>>(),
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format namespaces listing as tree
    pub fn format_namespaces_tree(catalog: &str, namespaces: &[Vec<String>]) -> String {
        let mut output = Vec::new();

        output.push(format!(
            "{} {}",
            catalog.cyan(),
            format!("({})", namespaces.len()).dimmed()
        ));

        if namespaces.is_empty() {
            output.push(format!("{}{}", TREE_LAST, "(empty)".dimmed()));
            return output.join("\n");
        }

        let len = namespaces.len();
        for (i, ns) in namespaces.iter().enumerate() {
            let is_last = i == len - 1;
            let prefix = if is_last { TREE_LAST } else { TREE_BRANCH };
            output.push(format!("{}{}", prefix, ns.join(".")));
        }

        output.join("\n")
    }

    /// Format tables listing as JSON
    pub fn format_tables_json(
        catalog: &str,
        namespace: &str,
        tables: &[String],
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "catalog": catalog,
            "namespace": namespace,
            "tables": tables,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format tables listing as tree
    pub fn format_tables_tree(catalog: &str, namespace: &str, tables: &[String]) -> String {
        let mut output = Vec::new();

        output.push(format!(
            "{}.{} {}",
            catalog.cyan(),
            namespace.cyan(),
            format!("({})", tables.len()).dimmed()
        ));

        if tables.is_empty() {
            output.push(format!("{}{}", TREE_LAST, "(empty)".dimmed()));
            return output.join("\n");
        }

        let len = tables.len();
        for (i, table) in tables.iter().enumerate() {
            let is_last = i == len - 1;
            let prefix = if is_last { TREE_LAST } else { TREE_BRANCH };
            output.push(format!("{}{}", prefix, table));
        }

        output.join("\n")
    }

    /// Format table info as JSON
    pub fn format_table_info_json(
        catalog: &str,
        namespace: &str,
        table: &str,
        location: &str,
        snapshot_count: usize,
        branches: &[&str],
        tags: &[&str],
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "catalog": catalog,
            "namespace": namespace,
            "table": table,
            "location": location,
            "snapshots": snapshot_count,
            "branches": branches,
            "tags": tags,
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format table info as tree
    pub fn format_table_info_tree(
        catalog: &str,
        namespace: &str,
        table: &str,
        snapshots: &[LsSnapshotInfo],
        branches: &[LsRefInfo],
        tags: &[LsRefInfo],
    ) -> String {
        let mut output = Vec::new();

        output.push(format!(
            "{}.{}.{}",
            catalog.cyan(),
            namespace.cyan(),
            table.cyan().bold()
        ));

        // Snapshots section
        let is_last_section = branches.is_empty() && tags.is_empty();
        let prefix = if is_last_section {
            TREE_LAST
        } else {
            TREE_BRANCH
        };
        output.push(format!(
            "{}{} {}",
            prefix,
            "snapshots".yellow(),
            format!("({})", snapshots.len()).dimmed()
        ));

        // Show last 3 snapshots
        let recent: Vec<_> = snapshots.iter().take(3).collect();
        let indent = if is_last_section { "    " } else { TREE_INDENT };
        for (i, snap) in recent.iter().enumerate() {
            let snap_prefix = if i == recent.len() - 1 {
                TREE_LAST
            } else {
                TREE_BRANCH
            };
            output.push(format!(
                "{}{}#{} {}",
                indent,
                snap_prefix,
                snap.id.to_string().dimmed(),
                snap.operation.dimmed()
            ));
        }
        if snapshots.len() > 3 {
            output.push(format!(
                "{}{}... and {} more",
                indent,
                TREE_LAST,
                snapshots.len() - 3
            ));
        }

        // Branches section
        if !branches.is_empty() || !tags.is_empty() {
            let is_last_section = tags.is_empty();
            let prefix = if is_last_section {
                TREE_LAST
            } else {
                TREE_BRANCH
            };
            output.push(format!(
                "{}{} {}",
                prefix,
                "branches".yellow(),
                format!("({})", branches.len()).dimmed()
            ));

            let indent = if is_last_section { "    " } else { TREE_INDENT };
            for (i, branch) in branches.iter().enumerate() {
                let is_last = i == branches.len() - 1;
                let branch_prefix = if is_last { TREE_LAST } else { TREE_BRANCH };
                let current_marker = if branch.name == "main" { " *" } else { "" };
                output.push(format!(
                    "{}{}{}{}",
                    indent,
                    branch_prefix,
                    branch.name,
                    current_marker.green()
                ));
            }
        }

        // Tags section
        if !tags.is_empty() {
            output.push(format!(
                "{}{} {}",
                TREE_LAST,
                "tags".yellow(),
                format!("({})", tags.len()).dimmed()
            ));

            for (i, tag) in tags.iter().enumerate() {
                let is_last = i == tags.len() - 1;
                let tag_prefix = if is_last { TREE_LAST } else { TREE_BRANCH };
                output.push(format!("    {}{}", tag_prefix, tag.name));
            }
        }

        output.join("\n")
    }
}

/// Snapshot info for ls formatting
#[derive(Debug, Clone)]
pub struct LsSnapshotInfo {
    /// Snapshot ID
    pub id: i64,
    /// Operation name
    pub operation: String,
}

/// Reference info for ls formatting
#[derive(Debug, Clone)]
pub struct LsRefInfo {
    /// Reference name
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_namespaces_tree_empty() {
        let result = LsFormatter::format_namespaces_tree("demo", &[]);
        assert!(result.contains("demo"));
        assert!(result.contains("(0)"));
        assert!(result.contains("(empty)"));
    }

    #[test]
    fn test_format_namespaces_tree_with_data() {
        let namespaces = vec![vec!["db1".to_string()], vec!["db2".to_string()]];
        let result = LsFormatter::format_namespaces_tree("catalog", &namespaces);
        assert!(result.contains("catalog"));
        assert!(result.contains("(2)"));
        assert!(result.contains("db1"));
        assert!(result.contains("db2"));
    }

    #[test]
    fn test_format_namespaces_json() {
        let namespaces = vec![vec!["ns1".to_string()]];
        let result = LsFormatter::format_namespaces_json("cat", &namespaces).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["catalog"], "cat");
        assert_eq!(parsed["namespaces"][0], "ns1");
    }

    #[test]
    fn test_format_tables_tree_empty() {
        let result = LsFormatter::format_tables_tree("cat", "ns", &[]);
        assert!(result.contains("cat"));
        assert!(result.contains("ns"));
        assert!(result.contains("(0)"));
        assert!(result.contains("(empty)"));
    }

    #[test]
    fn test_format_tables_tree_with_data() {
        let tables = vec!["t1".to_string(), "t2".to_string()];
        let result = LsFormatter::format_tables_tree("cat", "ns", &tables);
        assert!(result.contains("t1"));
        assert!(result.contains("t2"));
        assert!(result.contains("(2)"));
    }

    #[test]
    fn test_format_tables_json() {
        let tables = vec!["table1".to_string()];
        let result = LsFormatter::format_tables_json("c", "n", &tables).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["catalog"], "c");
        assert_eq!(parsed["namespace"], "n");
        assert_eq!(parsed["tables"][0], "table1");
    }

    #[test]
    fn test_format_table_info_tree_basic() {
        let snapshots = vec![
            LsSnapshotInfo {
                id: 1,
                operation: "append".to_string(),
            },
            LsSnapshotInfo {
                id: 2,
                operation: "append".to_string(),
            },
        ];
        let branches = vec![LsRefInfo {
            name: "main".to_string(),
        }];
        let tags = vec![];

        let result = LsFormatter::format_table_info_tree(
            "catalog", "ns", "tbl", &snapshots, &branches, &tags,
        );
        assert!(result.contains("catalog"));
        assert!(result.contains("ns"));
        assert!(result.contains("tbl"));
        assert!(result.contains("snapshots"));
        assert!(result.contains("(2)"));
        assert!(result.contains("branches"));
        assert!(result.contains("main"));
    }

    #[test]
    fn test_format_table_info_tree_with_tags() {
        let snapshots = vec![LsSnapshotInfo {
            id: 1,
            operation: "append".to_string(),
        }];
        let branches = vec![LsRefInfo {
            name: "main".to_string(),
        }];
        let tags = vec![LsRefInfo {
            name: "v1.0".to_string(),
        }];

        let result =
            LsFormatter::format_table_info_tree("c", "n", "t", &snapshots, &branches, &tags);
        assert!(result.contains("tags"));
        assert!(result.contains("v1.0"));
    }

    #[test]
    fn test_format_table_info_json() {
        let branches = vec!["main", "dev"];
        let tags = vec!["v1"];
        let result =
            LsFormatter::format_table_info_json("c", "n", "t", "/path", 5, &branches, &tags)
                .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["snapshots"], 5);
        assert_eq!(parsed["branches"][0], "main");
        assert_eq!(parsed["tags"][0], "v1");
    }

    #[test]
    fn test_format_table_info_truncation() {
        let snapshots: Vec<LsSnapshotInfo> = (1..=10)
            .map(|i| LsSnapshotInfo {
                id: i,
                operation: "append".to_string(),
            })
            .collect();
        let result = LsFormatter::format_table_info_tree("c", "n", "t", &snapshots, &[], &[]);
        assert!(result.contains("... and 7 more"));
    }
}

//! Config command formatting utilities

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::formatter::create_styled_table;

/// Formatter for config command results
pub struct ConfigFormatter;

impl ConfigFormatter {
    /// Format config list as JSON
    pub fn format_config_list_json(
        current_catalog: Option<&str>,
        current_warehouse: Option<&str>,
        current_namespace: Option<&str>,
        current_table: Option<&str>,
        tables: &[ConfigTableInfo],
        catalogs: &[ConfigCatalogInfo],
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "current_catalog": current_catalog,
            "current_warehouse": current_warehouse,
            "current_namespace": current_namespace,
            "current_table": current_table,
            "tables": tables.iter().map(|t| serde_json::json!({
                "name": t.name,
                "path": t.path,
            })).collect::<Vec<_>>(),
            "catalogs": catalogs.iter().map(|c| serde_json::json!({
                "name": c.name,
                "provider": c.provider,
                "uri": c.uri,
                "warehouse": c.warehouse,
            })).collect::<Vec<_>>(),
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format config list as text
    pub fn format_config_list_text(
        current_catalog: Option<&str>,
        current_warehouse: Option<&str>,
        current_namespace: Option<&str>,
        current_table: Option<&str>,
        tables: &[ConfigTableInfo],
        catalogs: &[ConfigCatalogInfo],
        config_path: &str,
    ) -> String {
        let mut output = Vec::new();

        // Catalogs section
        output.push("Catalogs:".bold().to_string());

        if catalogs.is_empty() {
            output.push(format!("  {}", "(none)".dimmed()));
        } else {
            let mut table = create_styled_table();
            table.set_header(vec![
                Cell::new("".to_string()).set_alignment(CellAlignment::Center),
                Cell::new("Name".cyan().to_string()).set_alignment(CellAlignment::Left),
                Cell::new("Provider".cyan().to_string()).set_alignment(CellAlignment::Left),
                Cell::new("Warehouse".cyan().to_string()).set_alignment(CellAlignment::Left),
                Cell::new("Namespace".cyan().to_string()).set_alignment(CellAlignment::Left),
                Cell::new("Table".cyan().to_string()).set_alignment(CellAlignment::Left),
                Cell::new("URI".cyan().to_string()).set_alignment(CellAlignment::Left),
            ]);

            for cat in catalogs {
                let is_current = current_catalog == Some(cat.name.as_str());
                let marker = if is_current {
                    "●".green().to_string()
                } else {
                    "".to_string()
                };

                let (wh_display, ns_display, tbl_display) = if is_current {
                    (
                        current_warehouse.unwrap_or("-"),
                        current_namespace.unwrap_or("-"),
                        current_table.unwrap_or("-"),
                    )
                } else {
                    ("-", "-", "-")
                };

                table.add_row(vec![
                    Cell::new(marker).set_alignment(CellAlignment::Center),
                    Cell::new(&cat.name).set_alignment(CellAlignment::Left),
                    Cell::new(&cat.provider).set_alignment(CellAlignment::Left),
                    Cell::new(wh_display).set_alignment(CellAlignment::Left),
                    Cell::new(ns_display).set_alignment(CellAlignment::Left),
                    Cell::new(tbl_display).set_alignment(CellAlignment::Left),
                    Cell::new(&cat.uri).set_alignment(CellAlignment::Left),
                ]);
            }

            output.push(table.to_string());
        }

        // Tables section
        if !tables.is_empty() {
            output.push(String::new());
            output.push("Tables:".bold().to_string());

            let mut table = create_styled_table();
            table.set_header(vec![
                Cell::new("Name".cyan().to_string()).set_alignment(CellAlignment::Left),
                Cell::new("Path".cyan().to_string()).set_alignment(CellAlignment::Left),
            ]);

            for t in tables {
                table.add_row(vec![
                    Cell::new(&t.name).set_alignment(CellAlignment::Left),
                    Cell::new(&t.path).set_alignment(CellAlignment::Left),
                ]);
            }

            output.push(table.to_string());
        }

        // Config path
        output.push(String::new());
        output.push(format!("{} {}", "Config file:".dimmed(), config_path));

        output.join("\n")
    }

    /// Format use context success
    pub fn format_use_success(display: &str) -> String {
        format!("{} Using: {}", "✓".green(), display)
    }

    /// Format use context not found error
    pub fn format_use_not_found(name: &str) -> String {
        let mut output = Vec::new();
        output.push(format!("{} Not found: {}", "!".yellow(), name.cyan()));
        output.push(format!(
            "  Use {} to add it first",
            "icetable config add".dimmed()
        ));
        output.join("\n")
    }

    /// Format add table success
    pub fn format_add_table_success(name: &str, uri: &str) -> String {
        format!(
            "{} Added table: {} → {}",
            "✓".green(),
            name.cyan(),
            uri.dimmed()
        )
    }

    /// Format add catalog success
    pub fn format_add_catalog_success(
        name: &str,
        provider: &str,
        uri: &str,
        auth_desc: Option<&str>,
    ) -> String {
        let mut output = Vec::new();
        output.push(format!(
            "{} Added catalog: {} ({}) → {}",
            "✓".green(),
            name.cyan(),
            provider,
            uri.dimmed()
        ));
        if let Some(auth) = auth_desc
            && auth != "none"
        {
            output.push(format!("  {} {}", "Auth:".dimmed(), auth.dimmed()));
        }
        output.join("\n")
    }

    /// Format delete success
    pub fn format_delete_success(item_type: &str, name: &str) -> String {
        format!("{} Deleted {}: {}", "✓".green(), item_type, name.cyan())
    }

    /// Format delete not found
    pub fn format_delete_not_found(name: &str) -> String {
        format!("{} Not found: {}", "!".yellow(), name.cyan())
    }
}

/// Catalog info for formatting
#[derive(Debug, Clone)]
pub struct ConfigCatalogInfo {
    pub name: String,
    pub provider: String,
    pub uri: String,
    pub warehouse: Option<String>,
}

/// Table info for formatting
#[derive(Debug, Clone)]
pub struct ConfigTableInfo {
    pub name: String,
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_config_list_json() {
        let catalogs = vec![ConfigCatalogInfo {
            name: "polaris".to_string(),
            provider: "Polaris".to_string(),
            uri: "http://localhost:8181".to_string(),
            warehouse: Some("iceberg".to_string()),
        }];
        let tables = vec![ConfigTableInfo {
            name: "events".to_string(),
            path: "s3://bucket/events".to_string(),
        }];

        let json = ConfigFormatter::format_config_list_json(
            Some("polaris"),
            Some("iceberg"),
            Some("demo"),
            None,
            &tables,
            &catalogs,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["current_catalog"], "polaris");
        assert_eq!(parsed["current_warehouse"], "iceberg");
        assert_eq!(parsed["catalogs"][0]["name"], "polaris");
        assert_eq!(parsed["tables"][0]["name"], "events");
    }

    #[test]
    fn test_format_config_list_text() {
        let catalogs = vec![ConfigCatalogInfo {
            name: "polaris".to_string(),
            provider: "Polaris".to_string(),
            uri: "http://localhost:8181".to_string(),
            warehouse: None,
        }];

        let text = ConfigFormatter::format_config_list_text(
            Some("polaris"),
            None,
            None,
            None,
            &[],
            &catalogs,
            "/path/to/config",
        );

        assert!(text.contains("Catalogs:"));
        assert!(text.contains("polaris"));
        assert!(text.contains("Polaris"));
        assert!(text.contains("Config file:"));
    }

    #[test]
    fn test_format_config_list_text_empty() {
        let text = ConfigFormatter::format_config_list_text(
            None,
            None,
            None,
            None,
            &[],
            &[],
            "/path/to/config",
        );

        assert!(text.contains("Catalogs:"));
        assert!(text.contains("(none)"));
    }

    #[test]
    fn test_format_use_success() {
        let result = ConfigFormatter::format_use_success("polaris@iceberg.demo");
        assert!(result.contains("Using:"));
        assert!(result.contains("polaris@iceberg.demo"));
    }

    #[test]
    fn test_format_use_not_found() {
        let result = ConfigFormatter::format_use_not_found("unknown");
        assert!(result.contains("Not found:"));
        assert!(result.contains("unknown"));
        assert!(result.contains("config add"));
    }

    #[test]
    fn test_format_add_table_success() {
        let result = ConfigFormatter::format_add_table_success("events", "s3://bucket/events");
        assert!(result.contains("Added table:"));
        assert!(result.contains("events"));
    }

    #[test]
    fn test_format_add_catalog_success() {
        let result = ConfigFormatter::format_add_catalog_success(
            "polaris",
            "Polaris",
            "http://localhost",
            Some("oauth2"),
        );
        assert!(result.contains("Added catalog:"));
        assert!(result.contains("polaris"));
        assert!(result.contains("Auth:"));
    }

    #[test]
    fn test_format_delete_success() {
        let result = ConfigFormatter::format_delete_success("table", "events");
        assert!(result.contains("Deleted table:"));
        assert!(result.contains("events"));
    }

    #[test]
    fn test_format_delete_not_found() {
        let result = ConfigFormatter::format_delete_not_found("unknown");
        assert!(result.contains("Not found:"));
    }
}

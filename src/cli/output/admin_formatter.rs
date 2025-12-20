//! Admin command formatting utilities

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::formatter::create_styled_table;

/// Formatter for admin command results
pub struct AdminFormatter;

impl AdminFormatter {
    // =========================================================================
    // Auth Formatting
    // =========================================================================

    /// Format login success message
    pub fn format_login_success(
        catalog_name: &str,
        auth_type: &str,
        credentials_path: &str,
    ) -> String {
        let mut output = Vec::new();
        output.push(format!(
            "{} Logged in to {} ({})",
            "✓".green(),
            catalog_name.cyan().bold(),
            auth_type
        ));
        output.push(format!(
            "  {} {}",
            "Credentials saved to:".dimmed(),
            credentials_path
        ));
        output.join("\n")
    }

    /// Format logout success for all catalogs
    pub fn format_logout_all(count: usize) -> String {
        format!(
            "{} Logged out from {} catalog(s)",
            "✓".green(),
            count.to_string().cyan()
        )
    }

    /// Format logout success for single catalog
    pub fn format_logout_single(catalog_name: &str) -> String {
        format!(
            "{} Logged out from {}",
            "✓".green(),
            catalog_name.cyan().bold()
        )
    }

    /// Format logout not found
    pub fn format_logout_not_found(catalog_name: &str) -> String {
        format!(
            "{} No stored credentials for {}",
            "!".yellow(),
            catalog_name.cyan()
        )
    }

    /// Format auth status as JSON
    pub fn format_auth_status_json(
        statuses: &[AuthStatusInfo],
    ) -> Result<String, serde_json::Error> {
        let json_status: Vec<_> = statuses
            .iter()
            .map(|s| {
                serde_json::json!({
                    "catalog": s.catalog_name,
                    "auth_type": s.auth_type.as_deref(),
                    "configured": s.catalog_exists,
                })
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({ "catalogs": json_status }))
    }

    /// Format auth status as table
    pub fn format_auth_status_table(statuses: &[AuthStatusInfo]) -> String {
        if statuses.is_empty() {
            return "No stored credentials".dimmed().to_string();
        }

        let mut output = Vec::new();
        output.push("Stored credentials:".dimmed().to_string());
        output.push(String::new());

        let mut table = create_styled_table();
        table.set_header(vec![
            Cell::new("Catalog".cyan().to_string()).set_alignment(CellAlignment::Left),
            Cell::new("Auth Type".cyan().to_string()).set_alignment(CellAlignment::Left),
            Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Left),
        ]);

        for status in statuses {
            let config_status = if status.catalog_exists {
                "configured".green().to_string()
            } else {
                "orphaned".yellow().to_string()
            };
            table.add_row(vec![
                Cell::new(&status.catalog_name).set_alignment(CellAlignment::Left),
                Cell::new(status.auth_type.as_deref().unwrap_or("none"))
                    .set_alignment(CellAlignment::Left),
                Cell::new(config_status).set_alignment(CellAlignment::Left),
            ]);
        }

        output.push(table.to_string());
        output.join("\n")
    }

    /// Format single catalog auth status
    pub fn format_auth_status_single(status: &AuthStatusInfo) -> String {
        let mut output = Vec::new();
        output.push(format!(
            "{} {}",
            "Catalog:".dimmed(),
            status.catalog_name.cyan().bold()
        ));

        if !status.catalog_exists {
            output.push(format!(
                "  {} Catalog not found in config",
                "Warning:".yellow()
            ));
        }

        if let Some(ref auth_type) = status.auth_type {
            output.push(format!("  {} {}", "Auth type:".dimmed(), auth_type.green()));

            // Add auth details
            if let Some(ref details) = status.auth_details {
                for detail in details {
                    output.push(format!("  {} {}", detail.0.dimmed(), detail.1));
                }
            }
        } else {
            output.push(format!(
                "  {} {}",
                "Status:".dimmed(),
                "not authenticated".yellow()
            ));
        }

        output.join("\n")
    }

    /// Format single auth status as JSON
    pub fn format_auth_status_single_json(
        status: &AuthStatusInfo,
    ) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&serde_json::json!({
            "catalog": status.catalog_name,
            "configured": status.catalog_exists,
            "authenticated": status.auth_type.is_some(),
            "auth_type": status.auth_type,
        }))
    }

    // =========================================================================
    // Warehouse Formatting
    // =========================================================================

    /// Format warehouse list as JSON
    pub fn format_warehouse_list_json(
        catalog_name: &str,
        warehouses: &[WarehouseInfo],
    ) -> Result<String, serde_json::Error> {
        let json = serde_json::json!({
            "catalog": catalog_name,
            "warehouses": warehouses.iter().map(|w| serde_json::json!({
                "name": w.name,
                "warehouse_type": w.warehouse_type,
                "storage_type": w.storage_type,
                "location": w.location,
            })).collect::<Vec<_>>(),
        });
        serde_json::to_string_pretty(&json)
    }

    /// Format warehouse list as table
    pub fn format_warehouse_list_table(catalog_name: &str, warehouses: &[WarehouseInfo]) -> String {
        if warehouses.is_empty() {
            let mut output = Vec::new();
            output.push(format!(
                "{} {}",
                "No warehouses in".dimmed(),
                catalog_name.cyan().bold()
            ));
            output.push(String::new());
            output.push("See: icetable warehouse create --help".to_string());
            return output.join("\n");
        }

        let mut output = Vec::new();
        output.push(format!(
            "{} {}",
            "Warehouses in".dimmed(),
            catalog_name.cyan().bold()
        ));
        output.push(String::new());

        let mut table = create_styled_table();
        table.set_header(vec![
            Cell::new("Name".cyan().to_string()).set_alignment(CellAlignment::Left),
            Cell::new("Type".cyan().to_string()).set_alignment(CellAlignment::Left),
            Cell::new("Storage".cyan().to_string()).set_alignment(CellAlignment::Left),
            Cell::new("Location".cyan().to_string()).set_alignment(CellAlignment::Left),
        ]);

        for wh in warehouses {
            table.add_row(vec![
                Cell::new(&wh.name).set_alignment(CellAlignment::Left),
                Cell::new(&wh.warehouse_type).set_alignment(CellAlignment::Left),
                Cell::new(&wh.storage_type).set_alignment(CellAlignment::Left),
                Cell::new(&wh.location).set_alignment(CellAlignment::Left),
            ]);
        }

        output.push(table.to_string());
        output.join("\n")
    }

    /// Format warehouse create success
    pub fn format_warehouse_create_success(
        warehouse_name: &str,
        catalog_name: &str,
        location: &str,
    ) -> String {
        let mut output = Vec::new();
        output.push(format!(
            "{} Created warehouse '{}' in {}",
            "✓".green(),
            warehouse_name.cyan(),
            catalog_name.cyan()
        ));
        output.push(format!("  {} {}", "Location:".dimmed(), location));
        output.push(String::new());
        output.push(format!(
            "{} icetable config use {} -w {}",
            "Activate:".dimmed(),
            catalog_name,
            warehouse_name
        ));
        output.join("\n")
    }

    /// Format warehouse delete success
    pub fn format_warehouse_delete_success(warehouse_name: &str, catalog_name: &str) -> String {
        format!(
            "{} Deleted warehouse '{}' from {}",
            "✓".green(),
            warehouse_name.cyan(),
            catalog_name.cyan()
        )
    }

    /// Format warehouse examples for provider
    pub fn format_warehouse_examples(provider: &str) -> String {
        let mut output = Vec::new();
        output.push(format!("Examples for {}:\n", provider));

        match provider {
            "polaris" => {
                output.push("# MinIO (local development)".to_string());
                output.push(
                    "# Key: skipCredentialSubscopingIndirection=true disables STS".to_string(),
                );
                output.push("icetable warehouse create mywarehouse \\".to_string());
                output.push("  --location s3://bucket/warehouse \\".to_string());
                output.push("  --config '{\"endpoint\":\"http://localhost:9000\",\"pathStyleAccess\":true,\"skipCredentialSubscopingIndirection\":true,\"s3.credentials.catalog.accessKeyId\":\"minioadmin\",\"s3.credentials.catalog.secretAccessKey\":\"minioadmin\"}'".to_string());
                output.push(String::new());
                output.push("# Or use a config file (recommended for readability):".to_string());
                output.push("icetable warehouse create mywarehouse \\".to_string());
                output.push("  --location s3://bucket/warehouse \\".to_string());
                output.push("  --config ./minio-storage.json".to_string());
                output.push(String::new());
                output.push("# AWS S3 (with IAM role)".to_string());
                output.push("icetable warehouse create mywarehouse \\".to_string());
                output.push("  --location s3://bucket/warehouse \\".to_string());
                output.push("  --config-set region=eu-west-1 \\".to_string());
                output.push(
                    "  --config-set roleArn=arn:aws:iam::123456789:role/polaris-access".to_string(),
                );
            }
            "nessie" => {
                output.push("# Nessie does not require warehouse creation.".to_string());
                output.push("# Tables are organized by branches and namespaces.".to_string());
                output.push(String::new());
                output.push("# List branches:".to_string());
                output.push("icetable branch list".to_string());
            }
            "tabular" => {
                output.push("# Tabular warehouses are managed via the Tabular UI.".to_string());
                output.push("# See: https://tabular.io/docs".to_string());
            }
            "unity" => {
                output.push("# Unity Catalog warehouses are managed via Databricks.".to_string());
                output.push(
                    "# See: https://docs.databricks.com/en/data-governance/unity-catalog"
                        .to_string(),
                );
            }
            _ => {
                output.push(
                    "# Generic REST catalog - check your provider's documentation.".to_string(),
                );
                output.push(String::new());
                output.push("# Common pattern:".to_string());
                output.push("icetable warehouse create mywarehouse \\".to_string());
                output.push("  --location s3://bucket/warehouse".to_string());
            }
        }

        output.join("\n")
    }
}

/// Auth status information for formatting
#[derive(Debug, Clone)]
pub struct AuthStatusInfo {
    /// Catalog name
    pub catalog_name: String,
    /// Auth type description (if authenticated)
    pub auth_type: Option<String>,
    /// Whether catalog exists in config
    pub catalog_exists: bool,
    /// Additional auth details (key-value pairs)
    pub auth_details: Option<Vec<(String, String)>>,
}

/// Warehouse information for formatting
#[derive(Debug, Clone)]
pub struct WarehouseInfo {
    /// Warehouse name
    pub name: String,
    /// Warehouse type
    pub warehouse_type: String,
    /// Storage type
    pub storage_type: String,
    /// Default location
    pub location: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_login_success() {
        let result = AdminFormatter::format_login_success("demo", "OAuth2", "/path/to/creds");
        assert!(result.contains("demo"));
        assert!(result.contains("OAuth2"));
        assert!(result.contains("/path/to/creds"));
    }

    #[test]
    fn test_format_logout_all() {
        let result = AdminFormatter::format_logout_all(3);
        assert!(result.contains("3"));
        assert!(result.contains("catalog(s)"));
    }

    #[test]
    fn test_format_logout_single() {
        let result = AdminFormatter::format_logout_single("mycatalog");
        assert!(result.contains("mycatalog"));
    }

    #[test]
    fn test_format_warehouse_list_empty() {
        let result = AdminFormatter::format_warehouse_list_table("demo", &[]);
        assert!(result.contains("No warehouses"));
        assert!(result.contains("demo"));
    }

    #[test]
    fn test_format_warehouse_list_with_data() {
        let warehouses = vec![WarehouseInfo {
            name: "wh1".to_string(),
            warehouse_type: "internal".to_string(),
            storage_type: "s3".to_string(),
            location: "s3://bucket/path".to_string(),
        }];
        let result = AdminFormatter::format_warehouse_list_table("demo", &warehouses);
        assert!(result.contains("wh1"));
        assert!(result.contains("s3://bucket/path"));
    }

    #[test]
    fn test_format_warehouse_list_json() {
        let warehouses = vec![WarehouseInfo {
            name: "wh1".to_string(),
            warehouse_type: "internal".to_string(),
            storage_type: "s3".to_string(),
            location: "s3://bucket/path".to_string(),
        }];
        let result = AdminFormatter::format_warehouse_list_json("demo", &warehouses).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["catalog"], "demo");
        assert_eq!(parsed["warehouses"][0]["name"], "wh1");
    }

    #[test]
    fn test_format_warehouse_create_success() {
        let result = AdminFormatter::format_warehouse_create_success("mywh", "demo", "s3://bucket");
        assert!(result.contains("mywh"));
        assert!(result.contains("demo"));
        assert!(result.contains("s3://bucket"));
        assert!(result.contains("Activate:"));
    }

    #[test]
    fn test_format_auth_status_table_empty() {
        let result = AdminFormatter::format_auth_status_table(&[]);
        assert!(result.contains("No stored credentials"));
    }

    #[test]
    fn test_format_auth_status_table_with_data() {
        let statuses = vec![AuthStatusInfo {
            catalog_name: "cat1".to_string(),
            auth_type: Some("OAuth2".to_string()),
            catalog_exists: true,
            auth_details: None,
        }];
        let result = AdminFormatter::format_auth_status_table(&statuses);
        assert!(result.contains("cat1"));
        assert!(result.contains("OAuth2"));
        assert!(result.contains("configured"));
    }

    #[test]
    fn test_format_warehouse_examples_polaris() {
        let result = AdminFormatter::format_warehouse_examples("polaris");
        assert!(result.contains("MinIO"));
        assert!(result.contains("AWS S3"));
    }

    #[test]
    fn test_format_warehouse_examples_nessie() {
        let result = AdminFormatter::format_warehouse_examples("nessie");
        assert!(result.contains("does not require"));
    }
}

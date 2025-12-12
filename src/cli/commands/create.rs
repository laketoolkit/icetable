//! Create command implementation
//!
//! Creates namespaces or tables in a catalog.
//! - `create -n <namespace>` creates a namespace
//! - `create -n <namespace> -t <table> --schema <file>` creates a table

use colored::Colorize;

use crate::cli::parser::CreateArgs;
use crate::config::Config;
use crate::core::catalog::RestCatalogClient;
use crate::error::{Error, Result};
use crate::utils::with_resource_limits;

/// Handler for create command
pub struct CreateCommand;

impl CreateCommand {
    /// Execute create command
    pub async fn execute(args: CreateArgs) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 32 * 1024 * 1024;
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args)).await
    }

    async fn execute_inner(args: CreateArgs) -> Result<()> {
        let config = Config::load()?;

        // Get catalog name from args or context
        let catalog_name = args
            .catalog
            .clone()
            .or_else(|| config.get_current_catalog().map(String::from));

        let Some(catalog_name) = catalog_name else {
            println!("{} No catalog specified", "!".yellow());
            println!(
                "  Use {} or specify {}",
                "icetable config use <catalog>".dimmed(),
                "-c <catalog>".dimmed()
            );
            return Ok(());
        };

        // Get catalog config
        let Some(catalog_config) = config.catalogs.get(&catalog_name) else {
            println!(
                "{} Catalog not found: {}",
                "!".yellow(),
                catalog_name.cyan()
            );
            return Ok(());
        };

        // Must have namespace
        let Some(namespace) = &args.namespace else {
            println!("{} No namespace specified", "!".yellow());
            println!(
                "  Use {} to create a namespace",
                "icetable create -n <namespace>".dimmed()
            );
            println!(
                "  Use {} to create a table",
                "icetable create -n <namespace> -t <table> --schema <file>".dimmed()
            );
            return Ok(());
        };

        // Create REST client
        let client = RestCatalogClient::new(catalog_config).await?;

        if let Some(table_name) = &args.table {
            // Create table
            Self::create_table(&client, namespace, table_name, &args).await
        } else {
            // Create namespace
            Self::create_namespace(&client, namespace, &args).await
        }
    }

    async fn create_namespace(
        client: &RestCatalogClient,
        namespace: &str,
        args: &CreateArgs,
    ) -> Result<()> {
        let ns_parts: Vec<String> = namespace.split('.').map(String::from).collect();

        // Build properties from args
        let properties: std::collections::HashMap<String, String> = args
            .property
            .iter()
            .cloned()
            .collect();

        client.create_namespace(&ns_parts, properties).await?;

        println!(
            "{} Created namespace: {}",
            "✓".green(),
            namespace.cyan()
        );

        Ok(())
    }

    async fn create_table(
        client: &RestCatalogClient,
        namespace: &str,
        table_name: &str,
        args: &CreateArgs,
    ) -> Result<()> {
        let ns_parts: Vec<String> = namespace.split('.').map(String::from).collect();

        // Check if namespace exists
        let namespaces = client.list_namespaces(None).await?;
        let ns_exists = namespaces.iter().any(|ns| ns.join(".") == namespace);

        if !ns_exists {
            return Err(Error::General(format!(
                "Namespace '{}' does not exist. Create it first with: icetable create -n {}",
                namespace, namespace
            )));
        }

        // Schema is required for table creation
        let Some(schema_path) = &args.schema else {
            return Err(Error::General(
                "Schema file is required for table creation. Use --schema <file>".to_string()
            ));
        };

        // Read and parse schema file
        let schema_content = std::fs::read_to_string(schema_path)
            .map_err(|e| Error::General(format!("Failed to read schema file: {}", e)))?;

        let iceberg_schema: iceberg::spec::Schema = serde_json::from_str(&schema_content)
            .map_err(|e| Error::General(format!("Failed to parse schema JSON: {}", e)))?;

        // Build properties from args
        let properties: std::collections::HashMap<String, String> = args
            .property
            .iter()
            .cloned()
            .collect();

        client
            .create_table(&ns_parts, table_name, iceberg_schema, args.location.as_deref(), properties)
            .await?;

        println!(
            "{} Created table: {}.{}",
            "✓".green(),
            namespace.cyan(),
            table_name.cyan()
        );

        Ok(())
    }
}

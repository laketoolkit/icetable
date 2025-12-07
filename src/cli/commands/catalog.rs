//! Catalog command implementation
//!
//! Interact with Iceberg REST catalogs (Nessie, Polaris, etc.)

use colored::Colorize;
use iceberg::spec::Schema;
use std::collections::HashMap;

use crate::cli::parser::{
    CatalogArgs, CatalogCommands, CatalogCreateNamespaceArgs, CatalogCreateTableArgs,
    CatalogDropNamespaceArgs, CatalogDropTableArgs, CatalogNamespacesArgs,
};
use crate::config::{CatalogConfig, Config};
use crate::core::RestCatalogClient;
use crate::error::{Error, Result};

/// Handler for catalog command
pub struct CatalogCommand;

impl CatalogCommand {
    /// Execute catalog command
    pub async fn execute(args: CatalogArgs) -> Result<()> {
        let catalog = args.catalog;
        let namespace = args.namespace;
        let output = args.output;

        match args.command {
            CatalogCommands::Namespaces(sub) => Self::list_namespaces(catalog, output, sub).await,
            CatalogCommands::Tables(_) => Self::list_tables(catalog, output, namespace).await,
            CatalogCommands::Info => Self::show_info(catalog, output).await,
            CatalogCommands::CreateNamespace(sub) => Self::create_namespace(catalog, output, sub).await,
            CatalogCommands::DropNamespace(sub) => Self::drop_namespace(catalog, output, sub).await,
            CatalogCommands::CreateTable(sub) => Self::create_table(catalog, output, sub).await,
            CatalogCommands::DropTable(sub) => Self::drop_table(catalog, output, sub).await,
        }
    }

    /// Get catalog config from name or default
    fn get_catalog_config(catalog_name: Option<&str>) -> Result<(String, CatalogConfig)> {
        let config = Config::load()?;

        let name = match catalog_name {
            Some(n) => n.to_string(),
            None => {
                // Try to get from current context if it's a catalog reference
                if let Some(ctx) = &config.current_context {
                    if let Some(dot_pos) = ctx.find('.') {
                        ctx[..dot_pos].to_string()
                    } else {
                        return Err(Error::General(
                            "No catalog specified. Use --catalog or set a catalog table as current context.".to_string(),
                        ));
                    }
                } else {
                    return Err(Error::General(
                        "No catalog specified. Use --catalog <name> or configure one with 'icetable config add-catalog'".to_string(),
                    ));
                }
            }
        };

        let catalog_cfg = config.catalogs.get(&name).ok_or_else(|| {
            Error::General(format!(
                "Catalog '{}' not found. Add it with 'icetable config add-catalog {} <uri>'",
                name, name
            ))
        })?;

        Ok((name, catalog_cfg.clone()))
    }

    /// List namespaces in a catalog
    async fn list_namespaces(
        catalog: Option<String>,
        output: String,
        args: CatalogNamespacesArgs,
    ) -> Result<()> {
        let (catalog_name, config) = Self::get_catalog_config(catalog.as_deref())?;
        let client = RestCatalogClient::new(&config).await?;

        let parent: Option<Vec<String>> = args.parent.as_ref().map(|p| {
            p.split('.').map(|s| s.to_string()).collect()
        });

        let namespaces = client.list_namespaces(parent.as_deref()).await?;

        if output == "json" {
            let json = serde_json::json!({
                "catalog": catalog_name,
                "parent": args.parent,
                "namespaces": namespaces.iter().map(|n| n.join(".")).collect::<Vec<_>>(),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            // Tree-style format
            println!("{}", catalog_name.bold());

            if namespaces.is_empty() {
                println!("  {}", "(no namespaces)".dimmed());
            } else {
                let len = namespaces.len();
                for (i, ns) in namespaces.iter().enumerate() {
                    let is_last = i == len - 1;
                    let prefix = if is_last { "└── " } else { "├── " };
                    println!("{}{}", prefix.dimmed(), ns.join("."));
                }
            }
        }

        Ok(())
    }

    /// List tables in a namespace
    async fn list_tables(
        catalog: Option<String>,
        output: String,
        namespace: Option<String>,
    ) -> Result<()> {
        let (catalog_name, config) = Self::get_catalog_config(catalog.as_deref())?;
        let client = RestCatalogClient::new(&config).await?;

        // If no namespace provided, list all namespaces first then tables in each
        let namespaces: Vec<Vec<String>> = if let Some(ns) = &namespace {
            vec![ns.split('.').map(|s| s.to_string()).collect()]
        } else {
            // List root namespaces
            client.list_namespaces(None).await?
        };

        // Collect tables per namespace
        let mut ns_tables: Vec<(String, Vec<String>)> = Vec::new();
        for ns in &namespaces {
            let tables = client.list_tables(ns).await?;
            ns_tables.push((ns.join("."), tables));
        }

        if output == "json" {
            let all_tables: Vec<_> = ns_tables
                .iter()
                .flat_map(|(ns, tables)| {
                    let cat = catalog_name.clone();
                    let ns_clone = ns.clone();
                    tables.iter().map(move |t| {
                        serde_json::json!({
                            "namespace": ns_clone,
                            "name": t,
                            "full_name": format!("{}.{}.{}", cat, ns_clone, t),
                        })
                    })
                })
                .collect();
            let json = serde_json::json!({
                "catalog": &catalog_name,
                "namespace": namespace,
                "tables": all_tables,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            // Tree-style format
            println!("{}", catalog_name.bold());

            if ns_tables.is_empty() || ns_tables.iter().all(|(_, t)| t.is_empty()) {
                println!("  {}", "(no tables)".dimmed());
            } else {
                let ns_len = ns_tables.len();
                for (i, (ns, tables)) in ns_tables.iter().enumerate() {
                    let is_last_ns = i == ns_len - 1;
                    let ns_prefix = if is_last_ns { "└── " } else { "├── " };
                    let continuation = if is_last_ns { "    " } else { "│   " };

                    println!("{}{}", ns_prefix.dimmed(), ns);

                    let table_len = tables.len();
                    for (j, table) in tables.iter().enumerate() {
                        let is_last_table = j == table_len - 1;
                        let table_prefix = if is_last_table { "└── " } else { "├── " };
                        println!(
                            "{}{}{}",
                            continuation.dimmed(),
                            table_prefix.dimmed(),
                            table
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Show catalog information
    async fn show_info(catalog: Option<String>, output: String) -> Result<()> {
        let (catalog_name, config) = Self::get_catalog_config(catalog.as_deref())?;

        // Test connectivity by creating the client
        let _client = RestCatalogClient::new(&config).await?;

        if output == "json" {
            let json = serde_json::json!({
                "name": catalog_name,
                "uri": config.uri,
                "type": config.catalog_type.to_string(),
                "warehouse": config.warehouse,
                "connected": true,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            // Clean info format
            println!("{} {}", catalog_name.bold(), "●".green());
            println!();
            println!("  {}  {}", "uri".dimmed(), config.uri);
            println!("  {} {}", "type".dimmed(), config.catalog_type);
            if let Some(wh) = &config.warehouse {
                println!("  {}  {}", "warehouse".dimmed(), wh);
            }
        }

        Ok(())
    }

    /// Create a namespace
    async fn create_namespace(
        catalog: Option<String>,
        output: String,
        args: CatalogCreateNamespaceArgs,
    ) -> Result<()> {
        let (catalog_name, config) = Self::get_catalog_config(catalog.as_deref())?;
        let client = RestCatalogClient::new(&config).await?;

        let namespace: Vec<String> = args.namespace.split('.').map(|s| s.to_string()).collect();
        let properties: HashMap<String, String> = args.property.into_iter().collect();

        client.create_namespace(&namespace, properties).await?;

        if output == "json" {
            let json = serde_json::json!({
                "created": true,
                "catalog": catalog_name,
                "namespace": args.namespace,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!(
                "{} Created namespace {}.{}",
                "✓".green(),
                catalog_name.dimmed(),
                args.namespace.cyan()
            );
        }

        Ok(())
    }

    /// Drop a namespace
    async fn drop_namespace(
        catalog: Option<String>,
        output: String,
        args: CatalogDropNamespaceArgs,
    ) -> Result<()> {
        let (catalog_name, config) = Self::get_catalog_config(catalog.as_deref())?;
        let client = RestCatalogClient::new(&config).await?;

        let namespace: Vec<String> = args.namespace.split('.').map(|s| s.to_string()).collect();

        // Check if namespace has tables
        let tables = client.list_tables(&namespace).await?;

        if !tables.is_empty() && !args.force {
            return Err(Error::General(format!(
                "Namespace '{}' contains {} table(s). Use --force to drop anyway.",
                args.namespace,
                tables.len()
            )));
        }

        client.drop_namespace(&namespace).await?;

        if output == "json" {
            let json = serde_json::json!({
                "dropped": true,
                "catalog": catalog_name,
                "namespace": args.namespace,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!(
                "{} Dropped namespace {}.{}",
                "✓".green(),
                catalog_name.dimmed(),
                args.namespace.cyan()
            );
        }

        Ok(())
    }

    /// Create a table
    async fn create_table(
        catalog: Option<String>,
        output: String,
        args: CatalogCreateTableArgs,
    ) -> Result<()> {
        let (catalog_name, config) = Self::get_catalog_config(catalog.as_deref())?;
        let client = RestCatalogClient::new(&config).await?;

        // Read schema from file
        let schema_content = std::fs::read_to_string(&args.schema).map_err(|e| {
            Error::General(format!("Failed to read schema file '{}': {}", args.schema.display(), e))
        })?;

        // Parse schema JSON
        let schema: Schema = serde_json::from_str(&schema_content).map_err(|e| {
            Error::General(format!("Failed to parse schema JSON: {}", e))
        })?;

        let namespace: Vec<String> = args.namespace.split('.').map(|s| s.to_string()).collect();
        let properties: HashMap<String, String> = args.property.into_iter().collect();

        client.create_table(
            &namespace,
            &args.name,
            schema,
            args.location.as_deref(),
            properties,
        ).await?;

        if output == "json" {
            let json = serde_json::json!({
                "created": true,
                "catalog": catalog_name,
                "namespace": args.namespace,
                "table": args.name,
                "full_name": format!("{}.{}.{}", catalog_name, args.namespace, args.name),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!(
                "{} Created table {}.{}.{}",
                "✓".green(),
                catalog_name.dimmed(),
                args.namespace.dimmed(),
                args.name.cyan()
            );
        }

        Ok(())
    }

    /// Drop a table
    async fn drop_table(
        catalog: Option<String>,
        output: String,
        args: CatalogDropTableArgs,
    ) -> Result<()> {
        let (catalog_name, config) = Self::get_catalog_config(catalog.as_deref())?;
        let client = RestCatalogClient::new(&config).await?;

        let namespace: Vec<String> = args.namespace.split('.').map(|s| s.to_string()).collect();

        client.drop_table(&namespace, &args.name, args.purge).await?;

        if output == "json" {
            let json = serde_json::json!({
                "dropped": true,
                "catalog": catalog_name,
                "namespace": args.namespace,
                "table": args.name,
                "purged": args.purge,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| Error::General(e.to_string()))?
            );
        } else {
            println!(
                "{} Dropped table {}.{}.{}",
                "✓".green(),
                catalog_name.dimmed(),
                args.namespace.dimmed(),
                args.name.cyan()
            );
        }

        Ok(())
    }
}

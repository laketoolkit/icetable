//! Config command implementation
//!
//! Manages icetable configuration (aliases, catalogs, context).

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::common::print_json;
use crate::cli::output::create_styled_table;
use crate::cli::parser::{
    ConfigAddArgs, ConfigArgs, ConfigCommands, ConfigDeleteArgs, ConfigLsArgs, ConfigUseArgs,
};
use crate::config::{CatalogConfig, CatalogProvider, Config};
use crate::core::config::{CatalogAuth, CredentialSource};
use crate::error::Result;
use crate::utils::with_resource_limits;

/// Handler for config command
pub struct ConfigCommand;

impl ConfigCommand {
    /// Execute config command
    pub async fn execute(args: ConfigArgs) -> Result<()> {
        use super::constants::MEMORY_CONFIG_OPS;
        with_resource_limits(MEMORY_CONFIG_OPS, Self::execute_inner(args)).await
    }

    async fn execute_inner(args: ConfigArgs) -> Result<()> {
        match args.command {
            ConfigCommands::Use(args) => Self::use_context(args).await,
            ConfigCommands::Add(args) => Self::add(*args).await,
            ConfigCommands::Delete(args) => Self::delete(args).await,
            ConfigCommands::Ls(args) => Self::ls(args).await,
        }
    }

    /// Set the current context (catalog or table, with optional warehouse/namespace/table)
    ///
    /// Context format: `catalog[@warehouse][.namespace][.table]`
    /// Examples:
    /// - `polaris` - just catalog
    /// - `polaris@iceberg` - catalog with warehouse
    /// - `polaris@iceberg.demo` - with namespace
    /// - `polaris@iceberg.demo.events` - with table
    async fn use_context(args: ConfigUseArgs) -> Result<()> {
        let mut config = Config::load()?;

        // Validate: name must be provided
        let Some(ref name) = args.name else {
            println!("{} No name specified", "!".yellow());
            println!();
            println!("Usage:");
            println!(
                "  {} Use a catalog",
                "icetable config use <catalog>".dimmed()
            );
            println!(
                "  {} With warehouse",
                "icetable config use <catalog> -w <warehouse>".dimmed()
            );
            println!(
                "  {} With namespace",
                "icetable config use <catalog> -w <warehouse> -n <namespace>".dimmed()
            );
            println!(
                "  {} With table",
                "icetable config use <catalog> -w <warehouse> -n <namespace> -t <table>".dimmed()
            );
            return Ok(());
        };

        // Check if it's a catalog or table
        let is_catalog = config.catalogs.contains_key(name);
        let is_table = config.tables.contains_key(name);

        if !is_catalog && !is_table {
            println!("{} Not found: {}", "!".yellow(), name.cyan());
            println!("  Use {} to add it first", "icetable config add".dimmed());
            return Ok(());
        }

        if is_table {
            // For tables, -w/-n/-t don't make sense
            if args.warehouse.is_some() || args.namespace.is_some() || args.table.is_some() {
                println!(
                    "{} Options -w/-n/-t are only valid for catalogs",
                    "!".yellow()
                );
                return Ok(());
            }
            config.set_current_context(name.clone());
            config.save()?;
            println!("{} Using table: {}", "✓".green(), name.cyan());
            return Ok(());
        }

        // It's a catalog
        config.set_current_catalog(name.clone());

        // Build context string: catalog[@warehouse][.namespace][.table]
        let mut context = name.clone();
        let mut display_parts = vec![name.cyan().to_string()];

        if let Some(ref warehouse) = args.warehouse {
            context.push('@');
            context.push_str(warehouse);
            display_parts.push(format!("@{}", warehouse.cyan()));
        }

        if let Some(ref namespace) = args.namespace {
            context.push('.');
            context.push_str(namespace);
            display_parts.push(namespace.cyan().to_string());
        }

        if let Some(ref table) = args.table {
            // Table requires namespace
            if args.namespace.is_none() {
                println!("{} Table requires a namespace (-n)", "!".yellow());
                return Ok(());
            }
            context.push('.');
            context.push_str(table);
            display_parts.push(table.cyan().to_string());
        }

        config.set_current_context(context);
        config.save()?;

        // Build display: catalog@warehouse.namespace.table
        let display = if display_parts.len() > 1 && args.warehouse.is_some() {
            // Format: catalog@warehouse.namespace.table
            let catalog_part = display_parts[0].clone();
            let warehouse_part = display_parts[1].clone();
            let rest: Vec<_> = display_parts.iter().skip(2).cloned().collect();
            if rest.is_empty() {
                format!("{}{}", catalog_part, warehouse_part)
            } else {
                format!("{}{}.{}", catalog_part, warehouse_part, rest.join("."))
            }
        } else {
            display_parts.join(".")
        };

        println!("{} Using: {}", "✓".green(), display);

        Ok(())
    }

    /// Add a table alias or catalog (inferred from URI scheme)
    async fn add(args: ConfigAddArgs) -> Result<()> {
        let mut config = Config::load()?;

        // Normalize URI: remove trailing slash
        let uri = args.uri.trim_end_matches('/').to_string();

        // Infer type from URI scheme
        let is_catalog = uri.starts_with("http://") || uri.starts_with("https://");

        if is_catalog {
            // Set as current if no catalogs exist yet
            let set_as_current = config.catalogs.is_empty();

            // Add as catalog
            Self::add_catalog(&mut config, &args.name, &uri, &args, set_as_current).await?;
        } else {
            // Set as current if no tables exist yet
            let set_as_current = config.tables.is_empty();

            // Add as table alias
            config.add_table(args.name.clone(), uri.clone());

            if set_as_current {
                config.set_current_context(args.name.clone());
            }
            config.save()?;

            println!(
                "{} Added table: {} → {}",
                "✓".green(),
                args.name.cyan(),
                uri.dimmed()
            );
        }

        Ok(())
    }

    /// Add a catalog configuration
    async fn add_catalog(
        config: &mut Config,
        name: &str,
        uri: &str,
        args: &ConfigAddArgs,
        set_as_current: bool,
    ) -> Result<()> {
        // Infer auth type from provided options
        let (auth, auth_desc) = if let Some(token) = &args.token {
            // Bearer with inline token
            (
                CatalogAuth::bearer(CredentialSource::Inline(token.clone())),
                "bearer".to_string(),
            )
        } else if let Some(env_var) = &args.token_env {
            // Bearer with env var reference
            (
                CatalogAuth::bearer(CredentialSource::EnvVar(env_var.clone())),
                format!("bearer (env:{})", env_var),
            )
        } else if let Some(client_id) = &args.client_id {
            // OAuth2
            let secret_source = if let Some(secret) = &args.client_secret {
                CredentialSource::Inline(secret.clone())
            } else if let Some(env_var) = &args.client_secret_env {
                CredentialSource::EnvVar(env_var.clone())
            } else {
                println!(
                    "{} --client-id requires --client-secret or --client-secret-env",
                    "!".yellow()
                );
                return Ok(());
            };

            let desc = if let Some(env) = &args.client_secret_env {
                format!("oauth2 (client:{}, secret:env:{})", client_id, env)
            } else {
                format!("oauth2 (client:{})", client_id)
            };

            (
                CatalogAuth::oauth2(
                    client_id.clone(),
                    secret_source,
                    args.oauth2_endpoint.clone(),
                    args.oauth2_scope.clone(),
                ),
                desc,
            )
        } else if let Some(region) = &args.aws_region {
            // SigV4
            let mut auth = CatalogAuth::sigv4(region.clone());
            if let (CatalogAuth::SigV4 { signing_name, .. }, Some(sn)) =
                (&mut auth, &args.aws_signing_name)
            {
                *signing_name = sn.clone();
            }
            (auth, format!("sigv4 ({})", region))
        } else {
            // No auth
            (CatalogAuth::None, "none".to_string())
        };

        // Build properties from optional args
        let mut properties = std::collections::HashMap::new();
        if let Some(warehouse) = &args.warehouse {
            properties.insert("warehouse".to_string(), warehouse.clone());
        }

        // Determine provider: use explicit arg or auto-detect from URI
        let provider = args
            .provider
            .unwrap_or_else(|| CatalogProvider::detect_from_uri(uri));

        let catalog_config = CatalogConfig {
            catalog_type: crate::config::CatalogType::Rest,
            provider: Some(provider),
            uri: uri.to_string(),
            warehouse: args.warehouse.clone(),
            auth,
            credential: None,
            properties,
        };

        config.add_catalog(name.to_string(), catalog_config);

        if set_as_current {
            config.set_current_catalog(name.to_string());
        }
        config.save()?;

        println!(
            "{} Added catalog: {} ({}) → {}",
            "✓".green(),
            name.cyan(),
            provider,
            uri.dimmed()
        );
        if auth_desc != "none" {
            println!("  {} {}", "Auth:".dimmed(), auth_desc.dimmed());
        }

        Ok(())
    }

    /// Delete a table alias or catalog
    async fn delete(args: ConfigDeleteArgs) -> Result<()> {
        let mut config = Config::load()?;

        // Try tables first, then catalogs
        if config.delete_table(&args.name) {
            // Clear current context if it was this table
            if config.get_current_context() == Some(args.name.as_str()) {
                config.unset_current_context();
            }
            config.save()?;
            println!("{} Deleted table: {}", "✓".green(), args.name.cyan());
        } else if config.delete_catalog(&args.name) {
            // Clear current catalog if it was this one
            if config.get_current_catalog() == Some(args.name.as_str()) {
                config.unset_current_catalog();
            }
            config.save()?;
            println!("{} Deleted catalog: {}", "✓".green(), args.name.cyan());
        } else {
            println!("{} Not found: {}", "!".yellow(), args.name.cyan());
        }

        Ok(())
    }

    /// List all configured tables and catalogs
    async fn ls(args: ConfigLsArgs) -> Result<()> {
        let config = Config::load()?;

        // Parse current context to get warehouse, namespace, and table
        let (current_warehouse, current_namespace, current_table) = config
            .parse_current_context()
            .map(|ctx| (ctx.warehouse, ctx.namespace, ctx.table))
            .unwrap_or((None, None, None));

        if args.output == "json" {
            let json = serde_json::json!({
                "current_catalog": config.get_current_catalog(),
                "current_warehouse": current_warehouse,
                "current_namespace": current_namespace,
                "current_table": current_table,
                "tables": config.tables.iter().map(|(name, path)| {
                    serde_json::json!({
                        "name": name,
                        "path": path,
                    })
                }).collect::<Vec<_>>(),
                "catalogs": config.catalogs.iter().map(|(name, cat)| {
                    serde_json::json!({
                        "name": name,
                        "provider": cat.provider().to_string(),
                        "uri": cat.uri,
                        "warehouse": cat.warehouse,
                    })
                }).collect::<Vec<_>>(),
            });
            print_json(&json)?;
        } else {
            // Show catalogs with current context inline
            println!("{}", "Catalogs:".bold());
            if config.catalogs.is_empty() {
                println!("  {}", "(none)".dimmed());
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

                let mut names: Vec<_> = config.catalogs.keys().collect();
                names.sort();

                for name in names {
                    let cat = &config.catalogs[name];
                    let is_current = config.get_current_catalog() == Some(name.as_str());
                    let marker = if is_current {
                        "●".green().to_string()
                    } else {
                        "".to_string()
                    };

                    // Show warehouse/namespace/table only for the current catalog
                    let (wh_display, ns_display, tbl_display) = if is_current {
                        (
                            current_warehouse.as_deref().unwrap_or("-"),
                            current_namespace.as_deref().unwrap_or("-"),
                            current_table.as_deref().unwrap_or("-"),
                        )
                    } else {
                        ("-", "-", "-")
                    };

                    table.add_row(vec![
                        Cell::new(marker).set_alignment(CellAlignment::Center),
                        Cell::new(name).set_alignment(CellAlignment::Left),
                        Cell::new(cat.provider().to_string()).set_alignment(CellAlignment::Left),
                        Cell::new(wh_display).set_alignment(CellAlignment::Left),
                        Cell::new(ns_display).set_alignment(CellAlignment::Left),
                        Cell::new(tbl_display).set_alignment(CellAlignment::Left),
                        Cell::new(&cat.uri).set_alignment(CellAlignment::Left),
                    ]);
                }

                println!("{}", table);
            }

            // Show tables (only if there are any)
            if !config.tables.is_empty() {
                println!();
                println!("{}", "Tables:".bold());
                let mut table = create_styled_table();

                table.set_header(vec![
                    Cell::new("Name".cyan().to_string()).set_alignment(CellAlignment::Left),
                    Cell::new("Path".cyan().to_string()).set_alignment(CellAlignment::Left),
                ]);

                let mut names: Vec<_> = config.tables.keys().collect();
                names.sort();

                for name in names {
                    let path = &config.tables[name];

                    table.add_row(vec![
                        Cell::new(name).set_alignment(CellAlignment::Left),
                        Cell::new(path).set_alignment(CellAlignment::Left),
                    ]);
                }

                println!("{}", table);
            }

            // Show config path
            println!();
            println!(
                "{} {}",
                "Config file:".dimmed(),
                Config::config_path()?.display()
            );
        }

        Ok(())
    }
}

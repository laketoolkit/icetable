//! Config command implementation
//!
//! Manages icetable configuration (aliases, catalogs, context).

use colored::Colorize;

use crate::cli::output::{ConfigCatalogInfo, ConfigFormatter, ConfigTableInfo};
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
            println!("{}", ConfigFormatter::format_use_not_found(name));
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
            println!(
                "{}",
                ConfigFormatter::format_use_success(&format!("table: {}", name.cyan()))
            );
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

        println!("{}", ConfigFormatter::format_use_success(&display));

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
                "{}",
                ConfigFormatter::format_add_table_success(&args.name, &uri)
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
            properties,
        };

        config.add_catalog(name.to_string(), catalog_config);

        if set_as_current {
            config.set_current_catalog(name.to_string());
        }
        config.save()?;

        println!(
            "{}",
            ConfigFormatter::format_add_catalog_success(
                name,
                &provider.to_string(),
                uri,
                Some(&auth_desc)
            )
        );

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
            println!(
                "{}",
                ConfigFormatter::format_delete_success("table", &args.name)
            );
        } else if config.delete_catalog(&args.name) {
            // Clear current catalog if it was this one
            if config.get_current_catalog() == Some(args.name.as_str()) {
                config.unset_current_catalog();
            }
            config.save()?;
            println!(
                "{}",
                ConfigFormatter::format_delete_success("catalog", &args.name)
            );
        } else {
            println!("{}", ConfigFormatter::format_delete_not_found(&args.name));
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

        // Convert config data to formatter types
        let tables: Vec<ConfigTableInfo> = config
            .tables
            .iter()
            .map(|(name, path)| ConfigTableInfo {
                name: name.clone(),
                path: path.clone(),
            })
            .collect();

        let catalogs: Vec<ConfigCatalogInfo> = config
            .catalogs
            .iter()
            .map(|(name, cat)| ConfigCatalogInfo {
                name: name.clone(),
                provider: cat.provider().to_string(),
                uri: cat.uri.clone(),
                warehouse: cat.warehouse.clone(),
            })
            .collect();

        if args.output == "json" {
            let json_str = ConfigFormatter::format_config_list_json(
                config.get_current_catalog(),
                current_warehouse.as_deref(),
                current_namespace.as_deref(),
                current_table.as_deref(),
                &tables,
                &catalogs,
            )?;
            println!("{}", json_str);
        } else {
            let config_path = Config::config_path()?.display().to_string();
            let text = ConfigFormatter::format_config_list_text(
                config.get_current_catalog(),
                current_warehouse.as_deref(),
                current_namespace.as_deref(),
                current_table.as_deref(),
                &tables,
                &catalogs,
                &config_path,
            );
            println!("{}", text);
        }

        Ok(())
    }
}

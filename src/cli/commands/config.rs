//! Config command implementation
//!
//! Manages icetable configuration like kubectl config.

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::common::create_table;
use crate::cli::parser::{
    ConfigAddArgs, ConfigAddCatalogArgs, ConfigArgs, ConfigCommands, ConfigCurrentArgs,
    ConfigListArgs, ConfigRemoveArgs, ConfigRemoveCatalogArgs, ConfigUnsetArgs, ConfigUseArgs,
};
use crate::config::{CatalogConfig, Config, ResolvedTable};
use crate::error::Result;
use crate::utils::with_resource_limits;
use std::path::PathBuf;

/// Handler for config command
pub struct ConfigCommand;

impl ConfigCommand {
    /// Execute config command
    pub async fn execute(args: ConfigArgs) -> Result<()> {
        const ESTIMATED_MEMORY: u64 = 8 * 1024 * 1024; // 8MB for config ops
        with_resource_limits(ESTIMATED_MEMORY, Self::execute_inner(args)).await
    }

    async fn execute_inner(args: ConfigArgs) -> Result<()> {
        match args.command {
            ConfigCommands::Use(args) => Self::use_table(args).await,
            ConfigCommands::Current(args) => Self::current(args).await,
            ConfigCommands::Unset(args) => Self::unset(args).await,
            ConfigCommands::Add(args) => Self::add(args).await,
            ConfigCommands::Remove(args) => Self::remove(args).await,
            ConfigCommands::AddCatalog(args) => Self::add_catalog(args).await,
            ConfigCommands::RemoveCatalog(args) => Self::remove_catalog(args).await,
            ConfigCommands::List(args) => Self::list(args).await,
        }
    }

    /// Set current context (table alias, path, or catalog.table)
    async fn use_table(args: ConfigUseArgs) -> Result<()> {
        let mut config = Config::load()?;

        // Validate the reference exists (path, alias, or catalog.table)
        let display_name = match config.resolve_table(&args.table)? {
            ResolvedTable::Path(path) => path,
            ResolvedTable::Catalog {
                catalog_name,
                table_name,
                ..
            } => {
                format!("{}.{}", catalog_name, table_name)
            }
        };

        // Store the original reference (not resolved path) as context
        config.set_current_context(args.table.clone());
        config.save()?;

        println!(
            "{} Current context set to: {} ({})",
            "✓".green(),
            args.table.cyan(),
            display_name.dimmed()
        );

        Ok(())
    }

    /// Show current context
    async fn current(args: ConfigCurrentArgs) -> Result<()> {
        let config = Config::load()?;

        if args.output == "json" {
            let json = serde_json::json!({
                "current_context": config.get_current_context(),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_default()
            );
        } else {
            match config.get_current_context() {
                Some(context) => println!("{}", context.cyan()),
                None => println!("{}", "(none)".dimmed()),
            }
        }

        Ok(())
    }

    /// Unset current context
    async fn unset(_args: ConfigUnsetArgs) -> Result<()> {
        let mut config = Config::load()?;
        config.unset_current_context();
        config.save()?;

        println!("{} Current context unset", "✓".green());

        Ok(())
    }

    /// Add a named table alias
    async fn add(args: ConfigAddArgs) -> Result<()> {
        let mut config = Config::load()?;
        config.add_table(args.name.clone(), args.path.clone());
        config.save()?;

        println!(
            "{} Added table alias: {} → {}",
            "✓".green(),
            args.name.cyan(),
            args.path.dimmed()
        );

        Ok(())
    }

    /// Remove a named table alias
    async fn remove(args: ConfigRemoveArgs) -> Result<()> {
        let mut config = Config::load()?;

        if config.remove_table(&args.name) {
            config.save()?;
            println!("{} Removed table alias: {}", "✓".green(), args.name.cyan());
        } else {
            println!(
                "{} Table alias not found: {}",
                "!".yellow(),
                args.name.cyan()
            );
        }

        Ok(())
    }

    /// Add a catalog configuration
    async fn add_catalog(args: ConfigAddCatalogArgs) -> Result<()> {
        let mut config = Config::load()?;

        // Build properties from optional args
        let mut properties = std::collections::HashMap::new();
        if let Some(warehouse) = &args.warehouse {
            properties.insert("warehouse".to_string(), warehouse.clone());
        }

        let credential = if let Some(token) = &args.credential {
            Some(crate::utils::credentials::CredentialSource::Inline(
                token.clone(),
            ))
        } else if let Some(env_var) = &args.credential_env {
            Some(crate::utils::credentials::CredentialSource::EnvVar(
                env_var.clone(),
            ))
        } else if let Some(file_path) = &args.credential_file {
            Some(crate::utils::credentials::CredentialSource::File(
                PathBuf::from(file_path),
            ))
        } else if args.use_iam_role {
            Some(crate::utils::credentials::CredentialSource::IamRole)
        } else if args.use_oauth2 {
            Some(crate::utils::credentials::CredentialSource::OAuth2)
        } else {
            None
        };

        let catalog_config = CatalogConfig {
            catalog_type: args.catalog_type.clone(),
            uri: args.uri.clone(),
            warehouse: args.warehouse.clone(),
            credential,
            properties,
        };

        config.add_catalog(args.name.clone(), catalog_config);
        config.save()?;

        println!(
            "{} Added catalog: {} ({}) → {}",
            "✓".green(),
            args.name.cyan(),
            args.catalog_type.to_string().dimmed(),
            args.uri.dimmed()
        );

        println!();
        println!(
            "{}",
            format!(
                "Use tables with: icetable inspect -t {}.namespace.table",
                args.name
            )
            .dimmed()
        );

        Ok(())
    }

    /// Remove a catalog configuration
    async fn remove_catalog(args: ConfigRemoveCatalogArgs) -> Result<()> {
        let mut config = Config::load()?;

        if config.remove_catalog(&args.name) {
            config.save()?;
            println!("{} Removed catalog: {}", "✓".green(), args.name.cyan());
        } else {
            println!("{} Catalog not found: {}", "!".yellow(), args.name.cyan());
        }

        Ok(())
    }

    /// List all configured tables and catalogs
    async fn list(args: ConfigListArgs) -> Result<()> {
        let config = Config::load()?;

        if args.output == "json" {
            let json = serde_json::json!({
                "current_context": config.get_current_context(),
                "tables": config.tables.iter().map(|(name, path)| {
                    serde_json::json!({
                        "name": name,
                        "path": path,
                    })
                }).collect::<Vec<_>>(),
                "catalogs": config.catalogs.iter().map(|(name, cat)| {
                    serde_json::json!({
                        "name": name,
                        "type": cat.catalog_type,
                        "uri": cat.uri,
                    })
                }).collect::<Vec<_>>(),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_default()
            );
        } else {
            // Show current context
            println!("{}", "Current context:".bold());
            match config.get_current_context() {
                Some(context) => println!("  {}", context.cyan()),
                None => println!("  {}", "(none)".dimmed()),
            }

            // Show table aliases as table
            println!();
            println!("{}", "Table aliases:".bold());
            if config.tables.is_empty() {
                println!("  {}", "(none)".dimmed());
            } else {
                let mut table = create_table();

                table.set_header(vec![
                    Cell::new("".to_string()).set_alignment(CellAlignment::Center),
                    Cell::new("Name".cyan().to_string()).set_alignment(CellAlignment::Left),
                    Cell::new("Path".cyan().to_string()).set_alignment(CellAlignment::Left),
                ]);

                let mut names: Vec<_> = config.tables.keys().collect();
                names.sort();

                for name in names {
                    let path = &config.tables[name];
                    let is_current = config.get_current_context() == Some(name.as_str());
                    let marker = if is_current { "●".green().to_string() } else { "".to_string() };

                    table.add_row(vec![
                        Cell::new(marker).set_alignment(CellAlignment::Center),
                        Cell::new(name).set_alignment(CellAlignment::Left),
                        Cell::new(path).set_alignment(CellAlignment::Left),
                    ]);
                }

                println!("{}", table);
            }

            // Show catalogs as table
            println!();
            println!("{}", "Catalogs:".bold());
            if config.catalogs.is_empty() {
                println!("  {}", "(none)".dimmed());
            } else {
                let mut table = create_table();

                table.set_header(vec![
                    Cell::new("Name".cyan().to_string()).set_alignment(CellAlignment::Left),
                    Cell::new("Type".cyan().to_string()).set_alignment(CellAlignment::Left),
                    Cell::new("URI".cyan().to_string()).set_alignment(CellAlignment::Left),
                ]);

                let mut names: Vec<_> = config.catalogs.keys().collect();
                names.sort();

                for name in names {
                    let cat = &config.catalogs[name];
                    table.add_row(vec![
                        Cell::new(name).set_alignment(CellAlignment::Left),
                        Cell::new(cat.catalog_type.to_string()).set_alignment(CellAlignment::Left),
                        Cell::new(&cat.uri).set_alignment(CellAlignment::Left),
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

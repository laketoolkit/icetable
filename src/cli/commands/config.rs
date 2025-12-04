//! Config command implementation
//!
//! Manages icectl configuration like kubectl config.

use colored::Colorize;

use crate::cli::parser::{
    ConfigAddArgs, ConfigArgs, ConfigCommands, ConfigCurrentArgs, ConfigListArgs, ConfigRemoveArgs,
    ConfigUnsetArgs, ConfigUseArgs,
};
use crate::config::Config;
use crate::error::Result;

/// Handler for config command
pub struct ConfigCommand;

impl ConfigCommand {
    /// Execute config command
    pub async fn execute(args: ConfigArgs) -> Result<()> {
        match args.command {
            ConfigCommands::Use(args) => Self::use_table(args).await,
            ConfigCommands::Current(args) => Self::current(args).await,
            ConfigCommands::Unset(args) => Self::unset(args).await,
            ConfigCommands::Add(args) => Self::add(args).await,
            ConfigCommands::Remove(args) => Self::remove(args).await,
            ConfigCommands::List(args) => Self::list(args).await,
        }
    }

    /// Set current table context
    async fn use_table(args: ConfigUseArgs) -> Result<()> {
        let mut config = Config::load()?;

        // Resolve alias if it exists
        let resolved_path = config.resolve_table(&args.table);

        config.set_current_table(resolved_path.clone());
        config.save()?;

        println!(
            "{} Current table set to: {}",
            "✓".green(),
            resolved_path.cyan()
        );

        Ok(())
    }

    /// Show current table context
    async fn current(args: ConfigCurrentArgs) -> Result<()> {
        let config = Config::load()?;

        if args.output == "json" {
            let json = serde_json::json!({
                "current_table": config.get_current_table(),
            });
            println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
        } else {
            match config.get_current_table() {
                Some(table) => println!("{}", table.cyan()),
                None => println!("{}", "(none)".dimmed()),
            }
        }

        Ok(())
    }

    /// Unset current table context
    async fn unset(_args: ConfigUnsetArgs) -> Result<()> {
        let mut config = Config::load()?;
        config.unset_current_table();
        config.save()?;

        println!("{} Current table unset", "✓".green());

        Ok(())
    }

    /// Add a named table alias
    async fn add(args: ConfigAddArgs) -> Result<()> {
        let mut config = Config::load()?;
        config.add_table(args.name.clone(), args.path.clone(), args.description);
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

    /// List all configured tables
    async fn list(args: ConfigListArgs) -> Result<()> {
        let config = Config::load()?;

        if args.output == "json" {
            let json = serde_json::json!({
                "current_table": config.get_current_table(),
                "tables": config.tables.iter().map(|(name, tc)| {
                    serde_json::json!({
                        "name": name,
                        "path": tc.path,
                        "description": tc.description,
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
        } else {
            // Show current table
            println!("{}", "Current table:".bold());
            match config.get_current_table() {
                Some(table) => println!("  {}", table.cyan()),
                None => println!("  {}", "(none)".dimmed()),
            }

            // Show aliases
            println!();
            println!("{}", "Table aliases:".bold());
            if config.tables.is_empty() {
                println!("  {}", "(none)".dimmed());
            } else {
                let mut names: Vec<_> = config.tables.keys().collect();
                names.sort();

                for name in names {
                    let tc = &config.tables[name];
                    let is_current = config.get_current_table() == Some(&tc.path);
                    let marker = if is_current { "*" } else { " " };

                    print!("  {}{} → {}", marker.green(), name.cyan(), tc.path.dimmed());
                    if let Some(ref desc) = tc.description {
                        print!(" ({})", desc.dimmed());
                    }
                    println!();
                }
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

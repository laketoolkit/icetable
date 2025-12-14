//! Admin command implementation
//!
//! Handles catalog-specific management operations like warehouse CRUD and auth.

use std::collections::HashMap;
use std::path::Path;

use colored::Colorize;
use comfy_table::{Cell, CellAlignment};

use super::common::print_json;
use crate::cli::output::create_styled_table;
use crate::cli::parser::{
    AdminArgs, AdminCommands, AuthArgs, AuthCommands, AuthLoginArgs, AuthLogoutArgs,
    AuthStatusArgs, CliTableContext, WarehouseArgs, WarehouseCommands, WarehouseCreateArgs,
    WarehouseDeleteArgs, WarehouseLsArgs,
};
use crate::config::{
    AuthService, CatalogAuth, CatalogProvider, Config, CredentialSource, LogoutResult,
};
use crate::core::catalog::{
    create_management_client_with_name, CatalogManagement, CreateWarehouseRequest,
};
use crate::error::{Error, Result};

/// Handler for admin commands
pub struct AdminCommand;

impl AdminCommand {
    /// Execute admin command
    pub async fn execute(args: AdminArgs, ctx: &CliTableContext) -> Result<()> {
        match args.command {
            AdminCommands::Warehouse(args) => Self::warehouse(args, ctx).await,
            AdminCommands::Auth(args) => Self::auth(args, ctx).await,
        }
    }

    // =========================================================================
    // Auth Commands
    // =========================================================================

    /// Handle auth subcommands
    async fn auth(args: AuthArgs, ctx: &CliTableContext) -> Result<()> {
        match args.command {
            AuthCommands::Login(args) => Self::auth_login(args, ctx).await,
            AuthCommands::Logout(args) => Self::auth_logout(args, ctx).await,
            AuthCommands::Status(args) => Self::auth_status(args, ctx).await,
        }
    }

    /// Login to a catalog (store credentials)
    async fn auth_login(args: AuthLoginArgs, ctx: &CliTableContext) -> Result<()> {
        // Create service and resolve catalog name
        let mut service = AuthService::new()?;
        let catalog_name = service.resolve_catalog_name(ctx.catalog.as_deref())?;

        // Build auth from CLI args (remains in CLI - it's arg parsing)
        let auth = Self::build_auth_from_args(&args)?;

        // Delegate business logic to service
        let result = service.login(&catalog_name, auth)?;

        // Format output (CLI responsibility)
        println!(
            "{} Logged in to {} ({})",
            "✓".green(),
            result.catalog_name.cyan().bold(),
            result.auth_type
        );
        println!(
            "  {} {}",
            "Credentials saved to:".dimmed(),
            result.credentials_path.display()
        );

        Ok(())
    }

    /// Build CatalogAuth from login arguments
    fn build_auth_from_args(args: &AuthLoginArgs) -> Result<CatalogAuth> {
        // Bearer token (non-OAuth2)
        if let Some(ref token) = args.token {
            return Ok(CatalogAuth::Bearer {
                token: CredentialSource::Inline(token.clone()),
            });
        }
        if let Some(ref env_var) = args.token_env {
            return Ok(CatalogAuth::Bearer {
                token: CredentialSource::EnvVar(env_var.clone()),
            });
        }
        if let Some(ref path) = args.token_file {
            return Ok(CatalogAuth::Bearer {
                token: CredentialSource::File(path.clone()),
            });
        }

        // OAuth2 client credentials
        if let Some(ref client_id) = args.client_id {
            let client_secret = if let Some(ref secret) = args.client_secret {
                CredentialSource::Inline(secret.clone())
            } else if let Some(ref env_var) = args.secret_env {
                CredentialSource::EnvVar(env_var.clone())
            } else if let Some(ref path) = args.secret_file {
                CredentialSource::File(path.clone())
            } else {
                return Err(Error::Configuration {
                    message: "OAuth2 login requires --client-secret, --secret-env, or --secret-file"
                        .to_string(),
                });
            };

            return Ok(CatalogAuth::OAuth2 {
                client_id: client_id.clone(),
                client_secret,
                token_endpoint: args.token_endpoint.clone(),
                scope: Some(args.scope.clone()),
            });
        }

        Err(Error::Configuration {
            message: "Login requires either --client-id (OAuth2) or --token/--token-env/--token-file (Bearer)"
                .to_string(),
        })
    }

    /// Logout from a catalog (remove stored credentials)
    async fn auth_logout(args: AuthLogoutArgs, ctx: &CliTableContext) -> Result<()> {
        // Create service and delegate business logic
        let mut service = AuthService::new()?;
        let result = service.logout(ctx.catalog.as_deref(), args.all)?;

        // Format output based on result variant
        match result {
            LogoutResult::All { count } => {
                println!(
                    "{} Logged out from {} catalog(s)",
                    "✓".green(),
                    count.to_string().cyan()
                );
            }
            LogoutResult::Single { catalog_name } => {
                println!(
                    "{} Logged out from {}",
                    "✓".green(),
                    catalog_name.cyan().bold()
                );
            }
            LogoutResult::NotFound { catalog_name } => {
                println!(
                    "{} No stored credentials for {}",
                    "!".yellow(),
                    catalog_name.cyan()
                );
            }
        }

        Ok(())
    }

    /// Show authentication status for catalog(s)
    async fn auth_status(args: AuthStatusArgs, ctx: &CliTableContext) -> Result<()> {
        // Create service for business logic
        let service = AuthService::new()?;

        if args.all {
            // Get status for all catalogs from service
            let statuses = service.status_all();

            if args.output == "json" {
                let json_status: Vec<_> = statuses
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "catalog": s.catalog_name,
                            "auth_type": s.auth.as_ref().map(|a| a.describe()),
                            "configured": s.catalog_exists,
                        })
                    })
                    .collect();
                print_json(&serde_json::json!({ "catalogs": json_status }))?;
            } else if !service.has_any_credentials() {
                println!("{}", "No stored credentials".dimmed());
            } else {
                println!("{}", "Stored credentials:".dimmed());
                println!();

                let mut table = create_styled_table();
                table.set_header(vec![
                    Cell::new("Catalog".cyan().to_string()).set_alignment(CellAlignment::Left),
                    Cell::new("Auth Type".cyan().to_string()).set_alignment(CellAlignment::Left),
                    Cell::new("Status".cyan().to_string()).set_alignment(CellAlignment::Left),
                ]);

                for status in &statuses {
                    let config_status = if status.catalog_exists {
                        "configured".green().to_string()
                    } else {
                        "orphaned".yellow().to_string()
                    };
                    table.add_row(vec![
                        Cell::new(&status.catalog_name).set_alignment(CellAlignment::Left),
                        Cell::new(status.auth.as_ref().map(|a| a.describe()).unwrap_or("none"))
                            .set_alignment(CellAlignment::Left),
                        Cell::new(config_status).set_alignment(CellAlignment::Left),
                    ]);
                }

                println!("{}", table);
            }
        } else {
            // Get status for specific catalog
            let catalog_name = service.resolve_catalog_name(ctx.catalog.as_deref())?;
            let status = service.status(&catalog_name);

            if args.output == "json" {
                print_json(&serde_json::json!({
                    "catalog": status.catalog_name,
                    "configured": status.catalog_exists,
                    "authenticated": status.has_credentials,
                    "auth_type": status.auth.as_ref().map(|a| a.describe()),
                }))?;
            } else {
                println!(
                    "{} {}",
                    "Catalog:".dimmed(),
                    status.catalog_name.cyan().bold()
                );

                if !status.catalog_exists {
                    println!(
                        "  {} {}",
                        "Warning:".yellow(),
                        "Catalog not found in config"
                    );
                }

                if let Some(auth) = &status.auth {
                    println!("  {} {}", "Auth type:".dimmed(), auth.describe().green());
                    // Show additional info based on auth type (output formatting stays in CLI)
                    Self::print_auth_details(auth);
                } else {
                    println!(
                        "  {} {}",
                        "Status:".dimmed(),
                        "not authenticated".yellow()
                    );
                }
            }
        }

        Ok(())
    }

    /// Print auth details for status display (output formatting helper)
    fn print_auth_details(auth: &CatalogAuth) {
        match auth {
            CatalogAuth::OAuth2 {
                client_id,
                token_endpoint,
                scope,
                ..
            } => {
                println!("  {} {}", "Client ID:".dimmed(), client_id);
                if let Some(endpoint) = token_endpoint {
                    println!("  {} {}", "Token endpoint:".dimmed(), endpoint);
                }
                if let Some(s) = scope {
                    println!("  {} {}", "Scope:".dimmed(), s);
                }
            }
            CatalogAuth::Bearer { token } => {
                println!("  {} {}", "Token source:".dimmed(), token.describe());
            }
            CatalogAuth::SigV4 {
                region,
                signing_name,
            } => {
                println!("  {} {}", "Region:".dimmed(), region);
                println!("  {} {}", "Signing name:".dimmed(), signing_name);
            }
            CatalogAuth::None => {}
        }
    }

    // =========================================================================
    // Warehouse Commands
    // =========================================================================

    /// Handle warehouse subcommands
    async fn warehouse(args: WarehouseArgs, ctx: &CliTableContext) -> Result<()> {
        match args.command {
            WarehouseCommands::Ls(args) => Self::warehouse_ls(args, ctx).await,
            WarehouseCommands::Create(args) => Self::warehouse_create(args, ctx).await,
            WarehouseCommands::Delete(args) => Self::warehouse_delete(args, ctx).await,
        }
    }

    /// List warehouses
    async fn warehouse_ls(args: WarehouseLsArgs, ctx: &CliTableContext) -> Result<()> {
        let (catalog_name, client) = Self::get_management_client(ctx).await?;

        if !client.supports_management() {
            return Err(Error::UnsupportedFeature {
                feature: format!(
                    "Warehouse management not supported for {} catalogs",
                    client.catalog_type()
                ),
            });
        }

        let warehouses = client.list_warehouses().await?;

        if args.output == "json" {
            let json = serde_json::json!({
                "catalog": catalog_name,
                "warehouses": warehouses,
            });
            print_json(&json)?;
        } else if warehouses.is_empty() {
            println!(
                "{} {}",
                "No warehouses in".dimmed(),
                catalog_name.cyan().bold()
            );
            println!();
            println!("See: icetable admin warehouse create --help");
        } else {
            println!(
                "{} {}",
                "Warehouses in".dimmed(),
                catalog_name.cyan().bold()
            );
            println!();

            let mut table = create_styled_table();

            table.set_header(vec![
                Cell::new("Name".cyan().to_string()).set_alignment(CellAlignment::Left),
                Cell::new("Type".cyan().to_string()).set_alignment(CellAlignment::Left),
                Cell::new("Storage".cyan().to_string()).set_alignment(CellAlignment::Left),
                Cell::new("Location".cyan().to_string()).set_alignment(CellAlignment::Left),
            ]);

            for wh in &warehouses {
                table.add_row(vec![
                    Cell::new(&wh.name).set_alignment(CellAlignment::Left),
                    Cell::new(wh.warehouse_type.to_string()).set_alignment(CellAlignment::Left),
                    Cell::new(wh.storage_type.to_string()).set_alignment(CellAlignment::Left),
                    Cell::new(&wh.default_base_location).set_alignment(CellAlignment::Left),
                ]);
            }

            println!("{}", table);
        }

        Ok(())
    }

    /// Create a warehouse
    async fn warehouse_create(args: WarehouseCreateArgs, ctx: &CliTableContext) -> Result<()> {
        // Handle --examples flag
        if args.examples {
            let provider = Self::get_current_provider(ctx)?;
            Self::print_warehouse_examples(provider);
            return Ok(());
        }

        // At this point, name and location are guaranteed by clap's required_unless_present
        let name = args.name.as_ref().expect("name required");
        let location = args.location.as_ref().expect("location required");

        let (catalog_name, client) = Self::get_management_client(ctx).await?;

        if !client.supports_management() {
            return Err(Error::UnsupportedFeature {
                feature: format!(
                    "Warehouse management not supported for {} catalogs",
                    client.catalog_type()
                ),
            });
        }

        // Parse storage config from --config and --config-set
        let storage_config = Self::parse_storage_config(&args.config, &args.config_set)?;

        // Build request (storage type is inferred from location if not in config)
        let request = CreateWarehouseRequest::new(name, location)
            .with_storage_config_map(storage_config);

        let warehouse = client.create_warehouse(request).await?;

        println!(
            "{} Created warehouse '{}' in {}",
            "✓".green(),
            warehouse.name.cyan(),
            catalog_name.cyan()
        );
        println!(
            "  {} {}",
            "Location:".dimmed(),
            warehouse.default_base_location
        );

        Ok(())
    }

    /// Parse storage configuration from --config and --config-set
    ///
    /// --config can be:
    /// - Inline JSON: '{"s3.endpoint": "http://minio:9000"}'
    /// - File path: ./storage-config.json or /path/to/config.json
    ///
    /// --config-set entries override --config values
    fn parse_storage_config(
        config: &Option<String>,
        config_set: &[(String, String)],
    ) -> Result<HashMap<String, String>> {
        let mut result = HashMap::new();

        // Parse --config if provided
        if let Some(config_str) = config {
            let config_str = config_str.trim();

            if config_str.starts_with('{') {
                // Inline JSON
                let parsed: HashMap<String, serde_json::Value> =
                    serde_json::from_str(config_str).map_err(|e| Error::Parse {
                        message: format!("Invalid JSON in --config: {}", e),
                        source: Some(Box::new(e)),
                    })?;

                // Convert all values to strings
                for (key, value) in parsed {
                    let str_value = match value {
                        serde_json::Value::String(s) => s,
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Number(n) => n.to_string(),
                        _ => value.to_string(),
                    };
                    result.insert(key, str_value);
                }
            } else {
                // File path
                let path = Path::new(config_str);
                let content = std::fs::read_to_string(path).map_err(|e| Error::Parse {
                    message: format!("Failed to read config file '{}': {}", config_str, e),
                    source: Some(Box::new(e)),
                })?;

                let parsed: HashMap<String, serde_json::Value> =
                    serde_json::from_str(&content).map_err(|e| Error::Parse {
                        message: format!("Invalid JSON in config file '{}': {}", config_str, e),
                        source: Some(Box::new(e)),
                    })?;

                // Convert all values to strings
                for (key, value) in parsed {
                    let str_value = match value {
                        serde_json::Value::String(s) => s,
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Number(n) => n.to_string(),
                        _ => value.to_string(),
                    };
                    result.insert(key, str_value);
                }
            }
        }

        // Apply --config-set overrides (higher priority)
        for (key, value) in config_set {
            result.insert(key.clone(), value.clone());
        }

        Ok(result)
    }

    /// Delete a warehouse
    async fn warehouse_delete(args: WarehouseDeleteArgs, ctx: &CliTableContext) -> Result<()> {
        let (catalog_name, client) = Self::get_management_client(ctx).await?;

        if !client.supports_management() {
            return Err(Error::UnsupportedFeature {
                feature: format!(
                    "Warehouse management not supported for {} catalogs",
                    client.catalog_type()
                ),
            });
        }

        client.delete_warehouse(&args.name).await?;

        println!(
            "{} Deleted warehouse '{}' from {}",
            "✓".green(),
            args.name.cyan(),
            catalog_name.cyan()
        );

        Ok(())
    }

    /// Get the provider for the current catalog (from context or config)
    fn get_current_provider(ctx: &CliTableContext) -> Result<CatalogProvider> {
        let config = Config::load()?;

        let catalog_name = ctx
            .catalog
            .clone()
            .or_else(|| config.get_current_catalog().map(String::from))
            .ok_or(Error::NoCatalog)?;

        let catalog_config =
            config
                .catalogs
                .get(&catalog_name)
                .ok_or_else(|| Error::CatalogNotFound {
                    name: catalog_name.clone(),
                })?;

        Ok(catalog_config.provider())
    }

    /// Print example commands for creating warehouses with different storage backends
    fn print_warehouse_examples(provider: CatalogProvider) {
        println!("Examples for {}:\n", provider);

        match provider {
            CatalogProvider::Polaris => {
                println!("# MinIO (local development)");
                println!("icetable admin warehouse create mywarehouse \\");
                println!("  --location s3://bucket/warehouse \\");
                println!("  --config-set s3.endpoint=http://localhost:9000 \\");
                println!("  --config-set s3.path-style-access=true");
                println!();

                println!("# AWS S3");
                println!("icetable admin warehouse create mywarehouse \\");
                println!("  --location s3://bucket/warehouse \\");
                println!("  --config-set s3.region=eu-west-1");
                println!();

                println!("# GCS");
                println!("icetable admin warehouse create mywarehouse \\");
                println!("  --location gs://bucket/warehouse");
                println!();

                println!("# Azure (ADLS Gen2)");
                println!("icetable admin warehouse create mywarehouse \\");
                println!("  --location abfss://container@account.dfs.core.windows.net/warehouse");
            }
            CatalogProvider::Nessie => {
                println!("# Nessie does not require warehouse creation.");
                println!("# Tables are organized by branches and namespaces.");
                println!();
                println!("# List branches:");
                println!("icetable branch list");
            }
            CatalogProvider::Tabular => {
                println!("# Tabular warehouses are managed via the Tabular UI.");
                println!("# See: https://tabular.io/docs");
            }
            CatalogProvider::Unity => {
                println!("# Unity Catalog warehouses are managed via Databricks.");
                println!("# See: https://docs.databricks.com/en/data-governance/unity-catalog");
            }
            CatalogProvider::Generic => {
                println!("# Generic REST catalog - check your provider's documentation.");
                println!();
                println!("# Common pattern:");
                println!("icetable admin warehouse create mywarehouse \\");
                println!("  --location s3://bucket/warehouse");
            }
        }
    }

    /// Get management client using global catalog option or current context
    ///
    /// Uses credentials from credentials.yaml if available.
    async fn get_management_client(
        ctx: &CliTableContext,
    ) -> Result<(String, Box<dyn CatalogManagement>)> {
        let config = Config::load()?;

        // Resolve catalog name: -c > config context
        let catalog_name = ctx
            .catalog
            .clone()
            .or_else(|| config.get_current_catalog().map(String::from))
            .ok_or(Error::NoCatalog)?;

        // Get catalog config
        let catalog_config =
            config
                .catalogs
                .get(&catalog_name)
                .cloned()
                .ok_or_else(|| Error::CatalogNotFound {
                    name: catalog_name.clone(),
                })?;

        // Create management client with catalog name for credentials lookup
        let client =
            create_management_client_with_name(&catalog_config, Some(&catalog_name)).await?;

        Ok((catalog_name, client))
    }
}

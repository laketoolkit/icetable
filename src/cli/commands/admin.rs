//! Admin command implementation
//!
//! Handles catalog-specific management operations like warehouse CRUD and auth.

use std::collections::HashMap;
use std::path::Path;

use colored::Colorize;

use super::ConfigCommand;
use crate::cli::output::{AdminFormatter, AuthStatusInfo, WarehouseInfo};
use crate::cli::parser::{
    AdminArgs, AdminCommands, AuthArgs, AuthCommands, AuthLoginArgs, AuthLogoutArgs,
    AuthStatusArgs, CatalogContext, WarehouseArgs, WarehouseCommands, WarehouseCreateArgs,
    WarehouseDeleteArgs, WarehouseLsArgs,
};
use crate::config::{
    AuthService, CatalogAuth, CatalogProvider, Config, CredentialSource, LogoutResult,
};
use crate::core::catalog::{
    CatalogManagement, CreateWarehouseRequest, create_management_client_with_name,
};
use crate::error::{Error, Result};

/// Handler for admin commands
pub struct AdminCommand;

impl AdminCommand {
    /// Execute admin command
    pub async fn execute(args: AdminArgs, ctx: &CatalogContext) -> Result<()> {
        match args.command {
            AdminCommands::Config(args) => ConfigCommand::execute(args).await,
            AdminCommands::Warehouse(args) => Self::warehouse(args, ctx).await,
            AdminCommands::Auth(args) => Self::auth(args, ctx).await,
        }
    }

    // =========================================================================
    // Auth Commands
    // =========================================================================

    /// Handle auth subcommands
    async fn auth(args: AuthArgs, ctx: &CatalogContext) -> Result<()> {
        match args.command {
            AuthCommands::Login(args) => Self::auth_login(args, ctx).await,
            AuthCommands::Logout(args) => Self::auth_logout(args, ctx).await,
            AuthCommands::Status(args) => Self::auth_status(args, ctx).await,
        }
    }

    /// Login to a catalog (store credentials)
    async fn auth_login(args: AuthLoginArgs, ctx: &CatalogContext) -> Result<()> {
        // Create service and resolve catalog name
        let mut service = AuthService::new()?;
        let catalog_name = service.resolve_catalog_name(ctx.catalog.as_deref())?;

        // Build auth from CLI args (remains in CLI - it's arg parsing)
        let auth = Self::build_auth_from_args(&args)?;

        // Delegate business logic to service
        let result = service.login(&catalog_name, auth)?;

        // Format output using formatter
        println!(
            "{}",
            AdminFormatter::format_login_success(
                &result.catalog_name,
                result.auth_type,
                &result.credentials_path.display().to_string()
            )
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
                    message:
                        "OAuth2 login requires --client-secret, --secret-env, or --secret-file"
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
    async fn auth_logout(args: AuthLogoutArgs, ctx: &CatalogContext) -> Result<()> {
        // Create service and delegate business logic
        let mut service = AuthService::new()?;
        let result = service.logout(ctx.catalog.as_deref(), args.all)?;

        // Format output using formatter
        let output = match result {
            LogoutResult::All { count } => AdminFormatter::format_logout_all(count),
            LogoutResult::Single { catalog_name } => {
                AdminFormatter::format_logout_single(&catalog_name)
            }
            LogoutResult::NotFound { catalog_name } => {
                AdminFormatter::format_logout_not_found(&catalog_name)
            }
        };
        println!("{}", output);

        Ok(())
    }

    /// Show authentication status for catalog(s)
    async fn auth_status(args: AuthStatusArgs, ctx: &CatalogContext) -> Result<()> {
        // Create service for business logic
        let service = AuthService::new()?;

        if args.all {
            // Get status for all catalogs from service
            let statuses = service.status_all();

            // Convert to formatter types
            let formatter_statuses: Vec<AuthStatusInfo> = statuses
                .iter()
                .map(|s| AuthStatusInfo {
                    catalog_name: s.catalog_name.clone(),
                    auth_type: s.auth.as_ref().map(|a| a.describe().to_string()),
                    catalog_exists: s.catalog_exists,
                    auth_details: None,
                })
                .collect();

            if args.output == "json" {
                let json_str = AdminFormatter::format_auth_status_json(&formatter_statuses)
                    .map_err(|e| crate::error::Error::Serialization {
                        message: e.to_string(),
                    })?;
                println!("{}", json_str);
            } else {
                println!(
                    "{}",
                    AdminFormatter::format_auth_status_table(&formatter_statuses)
                );
            }
        } else {
            // Get status for specific catalog
            let catalog_name = service.resolve_catalog_name(ctx.catalog.as_deref())?;
            let status = service.status(&catalog_name);

            // Build auth details for formatter
            let auth_details = status.auth.as_ref().map(Self::get_auth_details);

            let formatter_status = AuthStatusInfo {
                catalog_name: status.catalog_name.clone(),
                auth_type: status.auth.as_ref().map(|a| a.describe().to_string()),
                catalog_exists: status.catalog_exists,
                auth_details,
            };

            if args.output == "json" {
                let json_str = AdminFormatter::format_auth_status_single_json(&formatter_status)
                    .map_err(|e| crate::error::Error::Serialization {
                        message: e.to_string(),
                    })?;
                println!("{}", json_str);
            } else {
                println!(
                    "{}",
                    AdminFormatter::format_auth_status_single(&formatter_status)
                );
            }
        }

        Ok(())
    }

    /// Extract auth details as key-value pairs for formatter
    fn get_auth_details(auth: &CatalogAuth) -> Vec<(String, String)> {
        let mut details = Vec::new();
        match auth {
            CatalogAuth::OAuth2 {
                client_id,
                token_endpoint,
                scope,
                ..
            } => {
                details.push(("Client ID:".to_string(), client_id.clone()));
                if let Some(endpoint) = token_endpoint {
                    details.push(("Token endpoint:".to_string(), endpoint.clone()));
                }
                if let Some(s) = scope {
                    details.push(("Scope:".to_string(), s.clone()));
                }
            }
            CatalogAuth::Bearer { token } => {
                details.push(("Token source:".to_string(), token.describe().to_string()));
            }
            CatalogAuth::SigV4 {
                region,
                signing_name,
            } => {
                details.push(("Region:".to_string(), region.clone()));
                details.push(("Signing name:".to_string(), signing_name.clone()));
            }
            CatalogAuth::None => {}
        }
        details
    }

    // =========================================================================
    // Warehouse Commands
    // =========================================================================

    /// Handle warehouse subcommands
    async fn warehouse(args: WarehouseArgs, ctx: &CatalogContext) -> Result<()> {
        match args.command {
            WarehouseCommands::Ls(args) => Self::warehouse_ls(args, ctx).await,
            WarehouseCommands::Create(args) => Self::warehouse_create(args, ctx).await,
            WarehouseCommands::Delete(args) => Self::warehouse_delete(args, ctx).await,
        }
    }

    /// List warehouses
    async fn warehouse_ls(args: WarehouseLsArgs, ctx: &CatalogContext) -> Result<()> {
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

        // Convert to formatter types
        let formatter_warehouses: Vec<WarehouseInfo> = warehouses
            .iter()
            .map(|wh| WarehouseInfo {
                name: wh.name.clone(),
                warehouse_type: wh.warehouse_type.to_string(),
                storage_type: wh.storage_type.to_string(),
                location: wh.default_base_location.clone(),
            })
            .collect();

        if args.output == "json" {
            let json_str =
                AdminFormatter::format_warehouse_list_json(&catalog_name, &formatter_warehouses)
                    .map_err(|e| crate::error::Error::Serialization {
                        message: e.to_string(),
                    })?;
            println!("{}", json_str);
        } else {
            println!(
                "{}",
                AdminFormatter::format_warehouse_list_table(&catalog_name, &formatter_warehouses)
            );
        }

        Ok(())
    }

    /// Create a warehouse
    async fn warehouse_create(args: WarehouseCreateArgs, ctx: &CatalogContext) -> Result<()> {
        // Handle --examples flag
        if args.examples {
            let provider = Self::get_current_provider(ctx)?;
            println!(
                "{}",
                AdminFormatter::format_warehouse_examples(&provider.to_string().to_lowercase())
            );
            return Ok(());
        }

        // At this point, name and location are guaranteed by clap's required_unless_present
        let name = args.name.as_ref().ok_or_else(|| Error::Configuration {
            message: "warehouse name is required (use --name or --examples)".to_string(),
        })?;
        let location = args.location.as_ref().ok_or_else(|| Error::Configuration {
            message: "warehouse location is required (use --location or --examples)".to_string(),
        })?;

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
        let request =
            CreateWarehouseRequest::new(name, location).with_storage_config_map(storage_config);

        let warehouse = client.create_warehouse(request).await?;

        println!(
            "{}",
            AdminFormatter::format_warehouse_create_success(
                &warehouse.name,
                &catalog_name,
                &warehouse.default_base_location
            )
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
    /// Returns HashMap<String, serde_json::Value> to preserve original types (bool, number, string)
    fn parse_storage_config(
        config: &Option<String>,
        config_set: &[(String, String)],
    ) -> Result<HashMap<String, serde_json::Value>> {
        let mut result = HashMap::new();

        // Parse --config if provided
        if let Some(config_str) = config {
            let config_str = config_str.trim();

            if config_str.starts_with('{') {
                // Inline JSON - preserve original types
                let parsed: HashMap<String, serde_json::Value> = serde_json::from_str(config_str)
                    .map_err(|e| Error::Parse {
                    message: format!("Invalid JSON in --config: {}", e),
                    source: Some(Box::new(e)),
                })?;
                result = parsed;
            } else {
                // File path
                let path = Path::new(config_str);
                let content = std::fs::read_to_string(path).map_err(|e| Error::Parse {
                    message: format!("Failed to read config file '{}': {}", config_str, e),
                    source: Some(Box::new(e)),
                })?;

                let parsed: HashMap<String, serde_json::Value> = serde_json::from_str(&content)
                    .map_err(|e| Error::Parse {
                        message: format!("Invalid JSON in config file '{}': {}", config_str, e),
                        source: Some(Box::new(e)),
                    })?;
                result = parsed;
            }
        }

        // Apply --config-set overrides (higher priority)
        // These are always strings since they come from CLI key=value pairs
        for (key, value) in config_set {
            result.insert(key.clone(), serde_json::Value::String(value.clone()));
        }

        Ok(result)
    }

    /// Delete a warehouse
    async fn warehouse_delete(args: WarehouseDeleteArgs, ctx: &CatalogContext) -> Result<()> {
        let (catalog_name, client) = Self::get_management_client(ctx).await?;

        if !client.supports_management() {
            return Err(Error::UnsupportedFeature {
                feature: format!(
                    "Warehouse management not supported for {} catalogs",
                    client.catalog_type()
                ),
            });
        }

        // If --force, delete all contents first
        if args.force {
            Self::force_delete_warehouse_contents(&catalog_name, &args.name, ctx).await?;
        }

        client.delete_warehouse(&args.name).await?;

        println!(
            "{}",
            AdminFormatter::format_warehouse_delete_success(&args.name, &catalog_name)
        );

        Ok(())
    }

    /// Force delete all contents of a warehouse (namespaces and tables)
    async fn force_delete_warehouse_contents(
        catalog_name: &str,
        warehouse_name: &str,
        _ctx: &CatalogContext,
    ) -> Result<()> {
        use crate::core::catalog::RestCatalogClient;

        // Load config and create a catalog client with the warehouse
        let config = Config::load()?;
        let mut catalog_config =
            config
                .catalogs
                .get(catalog_name)
                .cloned()
                .ok_or_else(|| Error::CatalogNotFound {
                    name: catalog_name.to_string(),
                })?;
        catalog_config.warehouse = Some(warehouse_name.to_string());

        let rest_client = RestCatalogClient::with_name(&catalog_config, Some(catalog_name)).await?;

        // List all namespaces
        let namespaces = match rest_client.list_namespaces(None).await {
            Ok(ns) => ns,
            Err(_) => return Ok(()), // No namespaces or error, continue with delete
        };

        // Delete contents of each namespace
        for ns in &namespaces {
            let ns_name = ns.join(".");

            // List and delete tables in namespace
            if let Ok(tables) = rest_client.list_tables(ns).await {
                for table in &tables {
                    print!("  {} {}.{} ... ", "Deleting".dimmed(), ns_name, table);
                    match rest_client.delete_table(ns, table, true).await {
                        Ok(_) => println!("{}", "ok".green()),
                        Err(e) => println!("{} ({})", "failed".red(), e),
                    }
                }
            }

            // Delete namespace
            print!("  {} {} ... ", "Deleting namespace".dimmed(), ns_name);
            match rest_client.delete_namespace(ns).await {
                Ok(_) => println!("{}", "ok".green()),
                Err(e) => println!("{} ({})", "failed".red(), e),
            }
        }

        Ok(())
    }

    /// Get the provider for the current catalog (from context or config)
    fn get_current_provider(ctx: &CatalogContext) -> Result<CatalogProvider> {
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

    /// Get management client using global catalog option or current context
    ///
    /// Uses credentials from credentials.yaml if available.
    async fn get_management_client(
        ctx: &CatalogContext,
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

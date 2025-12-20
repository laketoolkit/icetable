//! Auth command implementation
//!
//! Manages authentication for catalogs.

use crate::cli::output::{AdminFormatter, AuthStatusInfo};
use crate::cli::parser::{
    AuthArgs, AuthCommands, AuthLoginArgs, AuthLogoutArgs, AuthStatusArgs, CatalogContext,
};
use crate::config::{AuthService, CatalogAuth, CredentialSource, LogoutResult};
use crate::error::{Error, Result};

/// Handler for auth command
pub struct AuthCommand;

impl AuthCommand {
    /// Execute auth command
    pub async fn execute(args: AuthArgs, ctx: &CatalogContext) -> Result<()> {
        match args.command {
            AuthCommands::Login(args) => Self::login(args, ctx).await,
            AuthCommands::Logout(args) => Self::logout(args, ctx).await,
            AuthCommands::Status(args) => Self::status(args, ctx).await,
        }
    }

    /// Login to a catalog (store credentials)
    async fn login(args: AuthLoginArgs, ctx: &CatalogContext) -> Result<()> {
        // Create service and resolve catalog name
        let mut service = AuthService::new()?;
        let catalog_name = service.resolve_catalog_name(ctx.catalog.as_deref())?;

        // Build auth from CLI args
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
    async fn logout(args: AuthLogoutArgs, ctx: &CatalogContext) -> Result<()> {
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
    async fn status(args: AuthStatusArgs, ctx: &CatalogContext) -> Result<()> {
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
}

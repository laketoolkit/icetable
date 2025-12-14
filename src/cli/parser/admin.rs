//! Parser definitions for admin commands
//!
//! Commands:
//! - `icetable admin warehouse ls`
//! - `icetable admin warehouse create <name> --location <loc>`
//! - `icetable admin warehouse delete <name>`
//! - `icetable admin auth login [--client-id <id>] [--client-secret <secret>]`
//! - `icetable admin auth logout`
//! - `icetable admin auth status`
//!
//! Note: --catalog is a global option (use -c)

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Arguments for admin command
#[derive(Parser, Debug)]
pub struct AdminArgs {
    /// Admin subcommand
    #[command(subcommand)]
    pub command: AdminCommands,
}

/// Admin subcommands
#[derive(Subcommand, Debug)]
pub enum AdminCommands {
    /// Manage warehouses
    Warehouse(WarehouseArgs),

    /// Manage authentication
    Auth(AuthArgs),
}

/// Arguments for warehouse subcommand
#[derive(Parser, Debug)]
pub struct WarehouseArgs {
    /// Warehouse operation
    #[command(subcommand)]
    pub command: WarehouseCommands,
}

/// Warehouse operations
#[derive(Subcommand, Debug)]
pub enum WarehouseCommands {
    /// List warehouses
    Ls(WarehouseLsArgs),

    /// Create warehouse
    Create(WarehouseCreateArgs),

    /// Delete warehouse
    Delete(WarehouseDeleteArgs),
}

/// Arguments for listing warehouses
#[derive(Parser, Debug)]
pub struct WarehouseLsArgs {
    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for creating a warehouse
#[derive(Parser, Debug)]
pub struct WarehouseCreateArgs {
    /// Warehouse name
    #[arg(required_unless_present = "examples")]
    pub name: Option<String>,

    /// Storage location (s3://, gs://, az://)
    #[arg(short, long, required_unless_present = "examples")]
    pub location: Option<String>,

    /// Storage config as JSON (inline or file)
    #[arg(long)]
    pub config: Option<String>,

    /// Storage config key=value (repeatable)
    #[arg(long = "config-set", value_parser = super::parse_key_value)]
    pub config_set: Vec<(String, String)>,

    /// Show example commands
    #[arg(long)]
    pub examples: bool,
}

/// Arguments for deleting a warehouse
#[derive(Parser, Debug)]
pub struct WarehouseDeleteArgs {
    /// Warehouse name
    pub name: String,
}

// =============================================================================
// Auth Commands
// =============================================================================

/// Arguments for auth subcommand
#[derive(Parser, Debug)]
pub struct AuthArgs {
    /// Auth operation
    #[command(subcommand)]
    pub command: AuthCommands,
}

/// Auth operations
#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// Login to catalog (store credentials)
    Login(AuthLoginArgs),

    /// Logout from catalog (remove credentials)
    Logout(AuthLogoutArgs),

    /// Show authentication status
    Status(AuthStatusArgs),
}

/// Arguments for login command
#[derive(Parser, Debug)]
pub struct AuthLoginArgs {
    /// OAuth2 client ID
    #[arg(long)]
    pub client_id: Option<String>,

    /// OAuth2 client secret (plain text)
    #[arg(long, conflicts_with_all = ["secret_env", "secret_file"])]
    pub client_secret: Option<String>,

    /// Read secret from env var
    #[arg(long = "secret-env", conflicts_with_all = ["client_secret", "secret_file"])]
    pub secret_env: Option<String>,

    /// Read secret from file
    #[arg(long = "secret-file", conflicts_with_all = ["client_secret", "secret_env"])]
    pub secret_file: Option<PathBuf>,

    /// OAuth2 token endpoint
    #[arg(long)]
    pub token_endpoint: Option<String>,

    /// OAuth2 scope
    #[arg(long, default_value = "PRINCIPAL_ROLE:ALL")]
    pub scope: String,

    /// Bearer token (non-OAuth2)
    #[arg(long, conflicts_with_all = ["client_id", "client_secret", "secret_env", "secret_file"])]
    pub token: Option<String>,

    /// Read token from env var
    #[arg(long = "token-env", conflicts_with_all = ["client_id", "client_secret", "secret_env", "secret_file", "token"])]
    pub token_env: Option<String>,

    /// Read token from file
    #[arg(long = "token-file", conflicts_with_all = ["client_id", "client_secret", "secret_env", "secret_file", "token", "token_env"])]
    pub token_file: Option<PathBuf>,
}

/// Arguments for logout command
#[derive(Parser, Debug)]
pub struct AuthLogoutArgs {
    /// Remove all stored credentials
    #[arg(long)]
    pub all: bool,
}

/// Arguments for status command
#[derive(Parser, Debug)]
pub struct AuthStatusArgs {
    /// Show status for all catalogs
    #[arg(long)]
    pub all: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

//! Config command arguments

use clap::{Parser, Subcommand};

use crate::config::CatalogProvider;

/// Arguments for config command
#[derive(Parser, Debug)]
pub struct ConfigArgs {
    /// Config subcommand
    #[command(subcommand)]
    pub command: ConfigCommands,
}

/// Config subcommands
#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Set current context (catalog, namespace, table)
    Use(ConfigUseArgs),

    /// Add table alias or catalog (inferred from URI)
    Add(Box<ConfigAddArgs>),

    /// Delete table alias or catalog
    Delete(ConfigDeleteArgs),

    /// List configured tables and catalogs
    Ls(ConfigLsArgs),
}

/// Arguments for config use
#[derive(Parser, Debug)]
pub struct ConfigUseArgs {
    /// Catalog or table name
    pub name: Option<String>,

    /// Warehouse within catalog
    #[arg(short, long)]
    pub warehouse: Option<String>,

    /// Namespace (catalogs only)
    #[arg(short, long)]
    pub namespace: Option<String>,

    /// Table within namespace (catalogs only)
    #[arg(short, long)]
    pub table: Option<String>,
}

/// Arguments for config add (table alias or catalog, inferred from URI)
#[derive(Parser, Debug)]
pub struct ConfigAddArgs {
    /// Alias name (table) or catalog name
    pub name: String,

    /// URI (http/https=catalog, s3/gs/az/file=table)
    pub uri: String,

    // --- Catalog options (only used if URI is http/https) ---
    /// Provider (polaris, nessie, tabular, unity, generic)
    #[arg(short, long, value_enum, hide_possible_values = true)]
    pub provider: Option<CatalogProvider>,

    /// Warehouse location
    #[arg(short, long)]
    pub warehouse: Option<String>,

    /// Bearer token
    #[arg(long, conflicts_with_all = ["token_env", "client_id", "aws_region"])]
    pub token: Option<String>,

    /// Bearer token from env var
    #[arg(long, conflicts_with_all = ["token", "client_id", "aws_region"])]
    pub token_env: Option<String>,

    /// OAuth2 client ID
    #[arg(long, conflicts_with_all = ["token", "token_env", "aws_region"])]
    pub client_id: Option<String>,

    /// OAuth2 client secret
    #[arg(long, requires = "client_id")]
    pub client_secret: Option<String>,

    /// OAuth2 secret from env var
    #[arg(long, requires = "client_id")]
    pub client_secret_env: Option<String>,

    /// OAuth2 token endpoint
    #[arg(long, requires = "client_id")]
    pub oauth2_endpoint: Option<String>,

    /// OAuth2 scope
    #[arg(long, requires = "client_id")]
    pub oauth2_scope: Option<String>,

    /// AWS region for SigV4 auth
    #[arg(long, conflicts_with_all = ["token", "token_env", "client_id"])]
    pub aws_region: Option<String>,

    /// AWS signing service name
    #[arg(long, requires = "aws_region")]
    pub aws_signing_name: Option<String>,
}

/// Arguments for config delete
#[derive(Parser, Debug)]
pub struct ConfigDeleteArgs {
    /// Name to delete (tables searched first)
    pub name: String,
}

/// Arguments for config ls
#[derive(Parser, Debug)]
pub struct ConfigLsArgs {
    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

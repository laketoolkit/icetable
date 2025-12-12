//! Config command arguments

use clap::{Parser, Subcommand};

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
    /// Set the current context (catalog, namespace, table)
    Use(ConfigUseArgs),

    /// Add a table alias or catalog (inferred from URI scheme)
    Add(ConfigAddArgs),

    /// Delete a table alias or catalog
    Delete(ConfigDeleteArgs),

    /// List all configured tables and catalogs
    Ls(ConfigLsArgs),
}

/// Arguments for config use
#[derive(Parser, Debug)]
pub struct ConfigUseArgs {
    /// Catalog or table name to use
    pub name: Option<String>,

    /// Namespace (only for catalogs)
    #[arg(short, long)]
    pub namespace: Option<String>,

    /// Table within namespace (only for catalogs)
    #[arg(short, long)]
    pub table: Option<String>,
}

/// Arguments for config add (table alias or catalog, inferred from URI)
#[derive(Parser, Debug)]
pub struct ConfigAddArgs {
    /// Name (alias for table, or catalog name)
    pub name: String,

    /// URI (http/https = catalog, s3/gs/az/file = table alias)
    pub uri: String,

    // --- Catalog options (only used if URI is http/https) ---
    /// Warehouse location (catalog only)
    #[arg(short, long)]
    pub warehouse: Option<String>,

    /// Bearer token (catalog auth)
    #[arg(long, conflicts_with_all = ["token_env", "client_id", "aws_region"])]
    pub token: Option<String>,

    /// Bearer token from env var (catalog auth)
    #[arg(long, conflicts_with_all = ["token", "client_id", "aws_region"])]
    pub token_env: Option<String>,

    /// OAuth2 client ID (catalog auth)
    #[arg(long, conflicts_with_all = ["token", "token_env", "aws_region"])]
    pub client_id: Option<String>,

    /// OAuth2 client secret (catalog auth)
    #[arg(long, requires = "client_id")]
    pub client_secret: Option<String>,

    /// OAuth2 client secret from env var (catalog auth)
    #[arg(long, requires = "client_id")]
    pub client_secret_env: Option<String>,

    /// OAuth2 token endpoint (catalog auth)
    #[arg(long, requires = "client_id")]
    pub oauth2_endpoint: Option<String>,

    /// OAuth2 scope (catalog auth)
    #[arg(long, requires = "client_id")]
    pub oauth2_scope: Option<String>,

    /// AWS region for SigV4 auth (catalog auth)
    #[arg(long, conflicts_with_all = ["token", "token_env", "client_id"])]
    pub aws_region: Option<String>,

    /// AWS signing service name (catalog auth)
    #[arg(long, requires = "aws_region")]
    pub aws_signing_name: Option<String>,
}

/// Arguments for config delete
#[derive(Parser, Debug)]
pub struct ConfigDeleteArgs {
    /// Name to delete (searches tables first, then catalogs)
    pub name: String,
}

/// Arguments for config ls
#[derive(Parser, Debug)]
pub struct ConfigLsArgs {
    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

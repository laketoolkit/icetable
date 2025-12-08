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
    /// Set the current table context
    Use(ConfigUseArgs),

    /// Show the current table context
    Current(ConfigCurrentArgs),

    /// Unset the current table context
    Unset(ConfigUnsetArgs),

    /// Add a named table alias
    Add(ConfigAddArgs),

    /// Remove a named table alias
    Remove(ConfigRemoveArgs),

    /// Add a catalog configuration
    #[command(name = "add-catalog")]
    AddCatalog(ConfigAddCatalogArgs),

    /// Remove a catalog configuration
    #[command(name = "remove-catalog")]
    RemoveCatalog(ConfigRemoveCatalogArgs),

    /// List all configured tables and catalogs
    List(ConfigListArgs),
}

/// Arguments for config use
#[derive(Parser, Debug)]
pub struct ConfigUseArgs {
    /// Table path or alias name to use as default
    pub table: String,
}

/// Arguments for config current
#[derive(Parser, Debug)]
pub struct ConfigCurrentArgs {
    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for config unset
#[derive(Parser, Debug)]
pub struct ConfigUnsetArgs {}

/// Arguments for config add
#[derive(Parser, Debug)]
pub struct ConfigAddArgs {
    /// Alias name for the table
    pub name: String,

    /// Table path (local or s3://, gs://, etc.)
    pub path: String,

    /// Optional description
    #[arg(short, long)]
    pub description: Option<String>,
}

/// Arguments for config remove
#[derive(Parser, Debug)]
pub struct ConfigRemoveArgs {
    /// Alias name to remove
    pub name: String,
}

/// Arguments for config add-catalog
#[derive(Parser, Debug)]
pub struct ConfigAddCatalogArgs {
    /// Catalog name (used as prefix for tables, e.g., nessie.analytics.events)
    pub name: String,

    /// Catalog URI (e.g., http://nessie:19120/iceberg/)
    pub uri: String,

    /// Catalog type (rest, hive, glue)
    #[arg(short = 't', long, value_enum, default_value = "rest")]
    pub catalog_type: crate::core::CatalogType,

    /// Warehouse location (optional, some catalogs provide this)
    #[arg(short, long)]
    pub warehouse: Option<String>,

    /// Credential (optional, format depends on catalog type)
    #[arg(short, long)]
    pub credential: Option<String>,

    /// Credential from environment variable
    #[arg(long)]
    pub credential_env: Option<String>,

    /// Credential from file
    #[arg(long)]
    pub credential_file: Option<String>,

    /// Use IAM Role for authentication (AWS/GCP/Azure)
    #[arg(long)]
    pub use_iam_role: bool,

    /// Use OAuth2 for authentication
    #[arg(long)]
    pub use_oauth2: bool,
}

/// Arguments for config remove-catalog
#[derive(Parser, Debug)]
pub struct ConfigRemoveCatalogArgs {
    /// Catalog name to remove
    pub name: String,
}

/// Arguments for config list
#[derive(Parser, Debug)]
pub struct ConfigListArgs {
    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

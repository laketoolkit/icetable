//! Catalog command arguments

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Arguments for catalog command
#[derive(Parser, Debug)]
pub struct CatalogArgs {
    /// Catalog name (from config)
    #[arg(short, long, global = true)]
    pub catalog: Option<String>,

    /// Namespace (for commands that need it)
    #[arg(short, long, global = true)]
    pub namespace: Option<String>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text", global = true)]
    pub output: String,

    /// Catalog subcommand
    #[command(subcommand)]
    pub command: CatalogCommands,
}

/// Catalog subcommands
#[derive(Subcommand, Debug)]
pub enum CatalogCommands {
    /// List namespaces in a catalog
    Namespaces(CatalogNamespacesArgs),

    /// List tables in a namespace
    Tables(CatalogTablesArgs),

    /// Show catalog information
    Info,

    /// Create a namespace
    #[command(name = "create-namespace")]
    CreateNamespace(CatalogCreateNamespaceArgs),

    /// Drop a namespace
    #[command(name = "drop-namespace")]
    DropNamespace(CatalogDropNamespaceArgs),

    /// Create a table
    #[command(name = "create-table")]
    CreateTable(CatalogCreateTableArgs),

    /// Drop a table
    #[command(name = "drop-table")]
    DropTable(CatalogDropTableArgs),
}

/// Arguments for catalog namespaces
#[derive(Parser, Debug)]
pub struct CatalogNamespacesArgs {
    /// Parent namespace (for nested namespaces)
    #[arg(short, long)]
    pub parent: Option<String>,
}

/// Arguments for catalog tables
#[derive(Parser, Debug)]
pub struct CatalogTablesArgs {
    // Namespace is now a global flag in CatalogArgs
}

/// Arguments for catalog create-namespace
#[derive(Parser, Debug)]
pub struct CatalogCreateNamespaceArgs {
    /// Namespace name to create
    pub namespace: String,

    /// Namespace properties (key=value, can be specified multiple times)
    #[arg(short, long, value_parser = super::parse_key_value)]
    pub property: Vec<(String, String)>,
}

/// Arguments for catalog drop-namespace
#[derive(Parser, Debug)]
pub struct CatalogDropNamespaceArgs {
    /// Namespace name to drop
    pub namespace: String,

    /// Force drop even if namespace is not empty
    #[arg(long)]
    pub force: bool,
}

/// Arguments for catalog create-table
#[derive(Parser, Debug)]
pub struct CatalogCreateTableArgs {
    /// Namespace for the table
    #[arg(short, long)]
    pub namespace: String,

    /// Table name
    pub name: String,

    /// Schema file (JSON format)
    #[arg(long)]
    pub schema: PathBuf,

    /// Partition columns (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Table location (optional, catalog may provide default)
    #[arg(long)]
    pub location: Option<String>,

    /// Table properties (key=value, can be specified multiple times)
    #[arg(short, long, value_parser = super::parse_key_value)]
    pub property: Vec<(String, String)>,
}

/// Arguments for catalog drop-table
#[derive(Parser, Debug)]
pub struct CatalogDropTableArgs {
    /// Namespace of the table
    #[arg(short, long)]
    pub namespace: String,

    /// Table name to drop
    pub name: String,

    /// Also delete data files (purge)
    #[arg(long)]
    pub purge: bool,
}

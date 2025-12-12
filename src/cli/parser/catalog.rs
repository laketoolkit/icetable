//! Catalog command arguments
//!
//! Simplified catalog operations: ls, create, delete

use clap::Parser;
use std::path::PathBuf;

/// Arguments for ls command (list namespaces or tables)
#[derive(Parser, Debug)]
pub struct LsArgs {
    /// Catalog name (uses current if not specified)
    #[arg(short, long)]
    pub catalog: Option<String>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for create command (create namespace or table)
#[derive(Parser, Debug)]
pub struct CreateArgs {
    /// Catalog name (uses current if not specified)
    #[arg(short, long)]
    pub catalog: Option<String>,

    /// Schema file for table creation (JSON format)
    #[arg(long)]
    pub schema: Option<PathBuf>,

    /// Partition columns for table (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Table location (optional)
    #[arg(long)]
    pub location: Option<String>,

    /// Properties (key=value, can be specified multiple times)
    #[arg(short, long, value_parser = super::parse_key_value)]
    pub property: Vec<(String, String)>,
}

/// Arguments for delete command (delete namespace or table)
#[derive(Parser, Debug)]
pub struct DeleteArgs {
    /// Tables to delete (uses context namespace if -n not specified)
    #[arg(value_name = "TABLE")]
    pub tables: Vec<String>,

    /// Catalog name (uses current if not specified)
    #[arg(short, long)]
    pub catalog: Option<String>,

    /// Force delete namespace even if not empty
    #[arg(long)]
    pub force: bool,

    /// Purge table data files when deleting tables
    #[arg(long)]
    pub purge: bool,
}

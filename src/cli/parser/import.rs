//! Import command arguments

use clap::{Parser, Subcommand};

/// Import subcommands
#[derive(Subcommand, Debug)]
pub enum ImportCommands {
    /// Import from Delta Lake
    Delta(ImportDeltaArgs),

    /// Import from Parquet files
    Parquet(ImportParquetArgs),
}

/// Arguments for import delta command
#[derive(Parser, Debug)]
pub struct ImportDeltaArgs {
    /// Source Delta Lake table path
    pub source: String,

    /// Target Iceberg table path
    pub target: String,

    /// Table name (new tables only)
    #[arg(long)]
    pub name: Option<String>,

    /// Partition columns (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Preview without importing
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for import parquet command
#[derive(Parser, Debug)]
pub struct ImportParquetArgs {
    /// Source parquet file or directory
    pub source: String,

    /// Target Iceberg table path
    pub target: String,

    /// Table name (new tables only)
    #[arg(long)]
    pub name: Option<String>,

    /// Partition columns (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Glob pattern for files
    #[arg(long, default_value = "*.parquet")]
    pub pattern: String,

    /// Preview without importing
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

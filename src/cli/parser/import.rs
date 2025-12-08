//! Import command arguments

use clap::{Parser, Subcommand};

/// Import subcommands
#[derive(Subcommand, Debug)]
pub enum ImportCommands {
    /// Import from a Delta Lake table
    #[cfg(feature = "delta")]
    Delta(ImportDeltaArgs),

    /// Import from Parquet files
    Parquet(ImportParquetArgs),
}

/// Arguments for import delta command
#[cfg(feature = "delta")]
#[derive(Parser, Debug)]
pub struct ImportDeltaArgs {
    /// Path to source Delta Lake table
    pub source: String,

    /// Path to target Iceberg table
    pub target: String,

    /// Table name (for new tables)
    #[arg(long)]
    pub name: Option<String>,

    /// Partition columns (comma-separated, for new tables)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Dry run - show what would be imported without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for import parquet command
#[derive(Parser, Debug)]
pub struct ImportParquetArgs {
    /// Path to Parquet file or directory of Parquet files
    pub source: String,

    /// Path to target Iceberg table
    pub target: String,

    /// Table name (for new tables)
    #[arg(long)]
    pub name: Option<String>,

    /// Partition columns (comma-separated, for new tables)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Glob pattern for selecting files (e.g., "*.parquet", "**/*.parquet")
    #[arg(long, default_value = "*.parquet")]
    pub pattern: String,

    /// Dry run - show what would be imported without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

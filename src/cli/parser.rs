//! Command-line argument parsing

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// TableTools - Universal CLI for tabular data
#[derive(Parser, Debug)]
#[command(name = "tabletools")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, global = true, default_value = "info")]
    pub log_level: String,

    /// Log to file
    #[arg(long, global = true)]
    pub log_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Inspect table contents and metadata
    Inspect(InspectArgs),

    /// Validate file integrity and quality
    Validate(ValidateArgs),

    /// Compare two tables
    Diff(DiffArgs),

    /// Convert between formats
    Convert(ConvertArgs),

    /// Compute statistics
    Stats(StatsArgs),

    /// Execute SQL queries
    Query(QueryArgs),

    /// Interactive Terminal UI
    #[cfg(feature = "tui")]
    Tui(TuiArgs),
}

/// Arguments for inspect command
#[derive(Parser, Debug)]
pub struct InspectArgs {
    /// Path to table file or directory
    pub path: String,

    /// Number of rows to show
    #[arg(short = 'n', long, default_value = "10")]
    pub rows: usize,

    /// Only show schema
    #[arg(short, long)]
    pub schema: bool,

    /// Show metadata
    #[arg(short, long)]
    pub metadata: bool,

    /// Show statistics
    #[arg(long)]
    pub stats: bool,

    /// Force specific format
    #[arg(long)]
    pub format: Option<String>,

    /// Output format (table, json, yaml)
    #[arg(short, long, default_value = "table")]
    pub output: String,

    /// Only show specific columns
    #[arg(long, value_delimiter = ',')]
    pub columns: Option<Vec<String>>,

    /// Use random sampling instead of first rows
    #[arg(long)]
    pub sample: bool,
}

/// Arguments for validate command
#[derive(Parser, Debug)]
pub struct ValidateArgs {
    /// Path to table file or directory
    pub path: String,

    /// Validate against schema file
    #[arg(long)]
    pub schema: Option<PathBuf>,

    /// Validation rules file
    #[arg(long)]
    pub rules: Option<PathBuf>,

    /// Quick validation (structure only)
    #[arg(long)]
    pub quick: bool,

    /// Strict mode (warnings as errors)
    #[arg(long, conflicts_with = "relax")]
    pub strict: bool,

    /// Relax mode (never fail, always exit 0)
    #[arg(long, conflicts_with = "strict")]
    pub relax: bool,

    /// Output format (text, json, quiet)
    #[arg(short, long, default_value = "text")]
    pub output: String,

    /// Attempt to fix issues automatically
    #[arg(long)]
    pub fix: bool,
}

/// Arguments for diff command
#[derive(Parser, Debug)]
pub struct DiffArgs {
    /// First table path
    pub left: String,

    /// Second table path
    pub right: String,

    /// Only compare schemas
    #[arg(long)]
    pub schema_only: bool,

    /// Only compare data
    #[arg(long)]
    pub data_only: bool,

    /// Sample N rows for comparison
    #[arg(long)]
    pub sample: Option<usize>,

    /// Ignore row order
    #[arg(long)]
    pub ignore_order: bool,

    /// Ignore specific columns
    #[arg(long, value_delimiter = ',')]
    pub ignore_columns: Option<Vec<String>>,

    /// Report if more than N differences
    #[arg(long, default_value = "100")]
    pub threshold: usize,

    /// Output format (text, json, html)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for convert command
#[derive(Parser, Debug)]
pub struct ConvertArgs {
    /// Input file path
    pub input: String,

    /// Output file path
    #[arg(short, long)]
    pub output: String,

    /// Output format (parquet, arrow, csv, json)
    #[arg(short, long)]
    pub format: Option<String>,

    /// Compression algorithm (none, snappy, gzip, zstd, lz4)
    #[arg(long)]
    pub compression: Option<String>,

    /// Row group size for Parquet
    #[arg(long)]
    pub row_group_size: Option<usize>,

    /// Partition by columns
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Overwrite existing file
    #[arg(long)]
    pub overwrite: bool,

    /// Validate output after conversion
    #[arg(long)]
    pub validate: bool,
}

/// Arguments for stats command
#[derive(Parser, Debug)]
pub struct StatsArgs {
    /// Path to table file
    pub path: String,

    /// Only compute stats for specific columns
    #[arg(long, value_delimiter = ',')]
    pub columns: Option<Vec<String>>,

    /// Generate histograms
    #[arg(long)]
    pub histogram: bool,

    /// Compute percentiles (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub percentiles: Option<Vec<f64>>,

    /// Full profiling (slower)
    #[arg(long)]
    pub profile: bool,

    /// Output format (text, json, html)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for query command
#[derive(Parser, Debug)]
pub struct QueryArgs {
    /// SQL query to execute
    pub sql: String,

    /// Data source path
    #[arg(long)]
    pub data: Option<String>,

    /// Output file path
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format (table, csv, json, parquet)
    #[arg(long, default_value = "table")]
    pub format: String,

    /// Limit number of result rows
    #[arg(long)]
    pub limit: Option<usize>,
}

/// Arguments for tui command
#[cfg(feature = "tui")]
#[derive(Parser, Debug)]
pub struct TuiArgs {
    /// Path to table file or directory
    pub path: String,

    /// Read-only mode
    #[arg(long)]
    pub readonly: bool,

    /// Refresh interval in seconds
    #[arg(short, long, default_value = "5")]
    pub refresh: u64,
}

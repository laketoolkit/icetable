//! Command-line argument parsing

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// TableTools - Universal CLI for tabular data
#[derive(Parser, Debug)]
#[command(name = "tablectl")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Log level (off, error, warn, info, debug, trace)
    #[arg(long, global = true, default_value = "off", value_parser = parse_log_level)]
    pub log_level: crate::utils::LogLevel,

    /// Log to file
    #[arg(long, global = true)]
    pub log_file: Option<PathBuf>,

    /// The command to execute
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

    /// Show data preview
    #[arg(short, long)]
    pub preview: bool,

    /// Force specific format
    #[arg(short, long)]
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

    /// Force specific format (arrow, parquet, csv, json)
    #[arg(short, long)]
    pub format: Option<String>,

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

    /// Quiet mode (no output, just exit code)
    #[arg(short, long)]
    pub quiet: bool,

    /// Output format (text, json)
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

    /// Show detailed column statistics
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for convert command
#[derive(Parser, Debug)]
pub struct ConvertArgs {
    /// Input file path
    pub input: String,

    /// Output file path
    #[arg(short = 'F', long)]
    pub file: String,

    /// Output format (parquet, arrow, csv, json)
    #[arg(short, long)]
    pub format: Option<String>,

    /// Select specific columns (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub columns: Option<Vec<String>>,

    /// Filter rows with SQL-like WHERE clause (e.g., "age > 18 AND city = 'NYC'")
    #[arg(long = "where")]
    pub where_clause: Option<String>,

    /// Rename columns (format: old_name:new_name, comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub rename: Option<Vec<String>>,

    /// Cast column types (format: column:type, comma-separated, e.g., "age:Int64,price:Float64")
    #[arg(long, value_delimiter = ',')]
    pub cast: Option<Vec<String>>,

    /// Compression algorithm (none, snappy, gzip, zstd, lz4)
    #[arg(long)]
    pub compression: Option<String>,

    /// Row group size for Parquet
    #[arg(long)]
    pub row_group_size: Option<usize>,

    /// Partition by columns (comma-separated)
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
    /// SQL query to execute (file paths in query should be quoted, e.g., "SELECT * FROM 'data.parquet'")
    pub sql: String,

    /// Output file path (requires --format to be specified)
    #[arg(short = 'F', long, requires = "format")]
    pub file: Option<PathBuf>,

    /// Output format when saving to file (parquet, arrow, csv, json)
    #[arg(short, long)]
    pub format: Option<String>,

    /// Display format for stdout (table, json)
    #[arg(short, long, default_value = "table")]
    pub output: String,

    /// Limit number of result rows to display or save
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

/// Parse log level from string
fn parse_log_level(s: &str) -> Result<crate::utils::LogLevel, String> {
    s.parse()
}

//! Command-line argument parsing

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// TableTools - Universal CLI for tabular data
#[derive(Parser, Debug)]
#[command(name = "tablectl")]
#[command(version, about, long_about = None)]
pub struct Cli {
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
    /// Create a new empty table
    Init(InitArgs),

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

    /// View table version history
    History(HistoryArgs),

    /// Remove old files no longer referenced by the table
    Vacuum(VacuumArgs),

    /// Compact small files into larger ones
    Optimize(OptimizeArgs),

    /// Restore table to a previous version
    Restore(RestoreArgs),

    /// Create a checkpoint/snapshot of the current state
    Snapshot(SnapshotArgs),

    /// Repair table metadata and fix inconsistencies
    Repair(RepairArgs),

    /// Interactive Terminal UI
    #[cfg(feature = "tui")]
    Tui(TuiArgs),
}

/// Arguments for init command
#[derive(Parser, Debug)]
pub struct InitArgs {
    /// Table format: delta or iceberg
    #[arg(value_parser = ["delta", "iceberg"])]
    pub format: String,

    /// Path where the table will be created
    pub path: String,

    /// Schema definition file (JSON)
    #[arg(long)]
    pub schema: Option<PathBuf>,

    /// Table name
    #[arg(long)]
    pub name: Option<String>,

    /// Table description
    #[arg(long)]
    pub description: Option<String>,

    /// Partition columns (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Table properties (key=value, comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub properties: Option<Vec<String>>,
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

    /// Only show physical layout
    #[arg(long)]
    pub layout: bool,

    /// Show statistics
    #[arg(long)]
    pub stats: bool,

    /// Show data preview
    #[arg(short, long)]
    pub preview: bool,

    /// Verbose mode (more detailed info)
    #[arg(short, long)]
    pub verbose: bool,

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

    /// Read table at specific version (Delta) or snapshot ID (Iceberg)
    #[arg(long)]
    pub version: Option<i64>,

    /// Read table as of a specific timestamp (format: "2024-01-15" or "2024-01-15T10:30:00")
    #[arg(long)]
    pub as_of: Option<String>,
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

    /// Target table format for table-to-table conversion (delta, iceberg)
    #[arg(long, value_parser = ["delta", "iceberg"])]
    pub target_format: Option<String>,

    /// Output format (parquet, arrow, csv, json) - for file formats only
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

/// Arguments for history command
#[derive(Parser, Debug)]
pub struct HistoryArgs {
    /// Path to table
    pub path: String,

    /// Maximum number of versions to show
    #[arg(short = 'n', long, default_value = "10")]
    pub limit: usize,

    /// Show all versions (no limit)
    #[arg(long)]
    pub all: bool,

    /// Output format (table, json)
    #[arg(short, long, default_value = "table")]
    pub output: String,
}

/// Arguments for vacuum command
#[derive(Parser, Debug)]
pub struct VacuumArgs {
    /// Path to table
    pub path: String,

    /// Retention period in hours (default: 168 = 7 days)
    #[arg(short, long, default_value = "168")]
    pub retention_hours: u64,

    /// Dry run - show what would be deleted without actually deleting
    #[arg(long)]
    pub dry_run: bool,

    /// Force vacuum even if retention is below safety threshold
    #[arg(long)]
    pub force: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for optimize command
#[derive(Parser, Debug)]
pub struct OptimizeArgs {
    /// Path to table
    pub path: String,

    /// Target file size in bytes (default: 256MB)
    #[arg(long, default_value = "268435456")]
    pub target_size: u64,

    /// Maximum number of concurrent tasks
    #[arg(long, default_value = "4")]
    pub max_concurrent_tasks: usize,

    /// Only optimize files smaller than this size (bytes)
    #[arg(long)]
    pub min_file_size: Option<u64>,

    /// Filter to specific partitions (key=value, comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub partitions: Option<Vec<String>>,

    /// Enable Z-ordering on specified columns
    #[arg(long, value_delimiter = ',')]
    pub zorder: Option<Vec<String>>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for restore command
#[derive(Parser, Debug)]
pub struct RestoreArgs {
    /// Path to table
    pub path: String,

    /// Version to restore to
    #[arg(long, conflicts_with = "as_of")]
    pub version: Option<i64>,

    /// Restore to state as of timestamp (format: "2024-01-15" or "2024-01-15T10:30:00")
    #[arg(long, conflicts_with = "version")]
    pub as_of: Option<String>,

    /// Dry run - show what would change without actually restoring
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot command
#[derive(Parser, Debug)]
pub struct SnapshotArgs {
    /// Path to table
    pub path: String,

    /// Force checkpoint creation even if not needed
    #[arg(long)]
    pub force: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for repair command
#[derive(Parser, Debug)]
pub struct RepairArgs {
    /// Path to table
    pub path: String,

    /// Dry run - show what would be repaired without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Sync metadata with actual files on disk
    #[arg(long)]
    pub sync_metadata: bool,

    /// Remove references to missing files
    #[arg(long)]
    pub remove_missing: bool,

    /// Add untracked parquet files to the table
    #[arg(long)]
    pub add_orphans: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Parse log level from string
fn parse_log_level(s: &str) -> Result<crate::utils::LogLevel, String> {
    s.parse()
}

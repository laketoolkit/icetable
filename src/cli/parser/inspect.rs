//! Inspection command arguments

use clap::Parser;

/// Arguments for inspect command
#[derive(Parser, Debug)]
pub struct InspectArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

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

    /// Deep scan: check all snapshots for orphan detection (slower but accurate)
    #[arg(long)]
    pub deep: bool,

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

    /// Read table as of a specific time (e.g., "7d", "24h", or "2024-01-15")
    #[arg(long)]
    pub as_of: Option<String>,
}

/// Arguments for validate command
#[derive(Parser, Debug)]
pub struct ValidateArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Force specific format (arrow, parquet, csv, json)
    #[arg(short, long)]
    pub format: Option<String>,

    /// Validate against schema file
    #[arg(long)]
    pub schema: Option<std::path::PathBuf>,

    /// Validation rules file
    #[arg(long)]
    pub rules: Option<std::path::PathBuf>,

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
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Reference to compare (snapshot ID, branch name, or tag name). Defaults to current.
    pub reference: Option<String>,

    /// Base reference to compare against (snapshot ID, branch name, or tag name)
    #[arg(long)]
    pub base: Option<String>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for stats command
#[derive(Parser, Debug)]
pub struct StatsArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Table format (delta, iceberg) - auto-detected if not specified
    #[arg(short, long, value_parser = ["delta", "iceberg"])]
    pub format: Option<String>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,

    /// Filter by partition (e.g., "date=2024-01-15/*" or "region=us-west-2")
    #[arg(short, long)]
    pub partition: Option<String>,
}

/// Arguments for analyze command
#[derive(Parser, Debug)]
pub struct AnalyzeArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Minimum file size to consider "small" (default: 16MB)
    #[arg(long, default_value = "16777216")]
    pub min_file_size: u64,

    /// Skip orphan and missing files check (faster, skips storage scan)
    #[arg(long)]
    pub skip_orphans: bool,

    /// Show detailed partition-level information
    #[arg(short, long)]
    pub verbose: bool,

    /// Output format (text, json)
    #[arg(short = 'o', long, default_value = "text")]
    pub output: String,
}

/// Arguments for history command
#[derive(Parser, Debug)]
pub struct HistoryArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Table format (delta, iceberg) - auto-detected if not specified
    #[arg(short, long, value_parser = ["delta", "iceberg"])]
    pub format: Option<String>,

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

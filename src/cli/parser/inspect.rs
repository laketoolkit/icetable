//! Inspection command arguments

use clap::Parser;

/// Arguments for inspect command
#[derive(Parser, Debug)]
pub struct InspectArgs {
    /// Show snapshot history and manifests
    #[arg(short, long)]
    pub verbose: bool,

    /// Output format (table, json)
    #[arg(short, long, default_value = "table")]
    pub output: String,

    /// Specific snapshot ID
    #[arg(long)]
    pub snapshot: Option<i64>,

    /// Time travel (e.g., 7d, 24h, 2024-01-15)
    #[arg(long)]
    pub as_of: Option<String>,
}

/// Arguments for validate command
#[derive(Parser, Debug)]
pub struct ValidateArgs {
    /// Force format (arrow, parquet, csv, json)
    #[arg(short, long)]
    pub format: Option<String>,

    /// Schema file to validate against
    #[arg(long)]
    pub schema: Option<std::path::PathBuf>,

    /// Validation rules file
    #[arg(long)]
    pub rules: Option<std::path::PathBuf>,

    /// Quick validation (structure only)
    #[arg(long)]
    pub quick: bool,

    /// Warnings as errors
    #[arg(long, conflicts_with = "relax")]
    pub strict: bool,

    /// Never fail (exit 0)
    #[arg(long, conflicts_with = "strict")]
    pub relax: bool,

    /// No output, just exit code
    #[arg(short = 'Q', long)]
    pub quiet: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,

    /// Auto-fix issues
    #[arg(long)]
    pub fix: bool,
}

/// Arguments for diff command
#[derive(Parser, Debug)]
pub struct DiffArgs {
    /// From reference (snapshot ID, branch, tag)
    #[arg(long)]
    pub from: Option<String>,

    /// To reference (snapshot ID, branch, tag)
    #[arg(long)]
    pub to: Option<String>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for stats command
#[derive(Parser, Debug)]
pub struct StatsArgs {
    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,

    /// Filter by partition
    #[arg(short, long)]
    pub partition: Option<String>,
}

/// Arguments for analyze command
#[derive(Parser, Debug)]
pub struct AnalyzeArgs {
    /// Min size to consider "small" (bytes)
    #[arg(long, default_value = "16777216")]
    pub min_file_size: u64,

    /// Skip orphan/missing files check
    #[arg(long)]
    pub skip_orphans: bool,

    /// Check all snapshots for orphans
    #[arg(long, default_value_t = true)]
    pub all_snapshots: bool,

    /// Show partition-level details
    #[arg(short, long)]
    pub verbose: bool,

    /// Output format (text, json)
    #[arg(short = 'o', long, default_value = "text")]
    pub output: String,
}

/// Arguments for history command
#[derive(Parser, Debug)]
pub struct HistoryArgs {
    /// Max versions to show
    #[arg(long, default_value = "10")]
    pub limit: usize,

    /// Show all versions
    #[arg(long)]
    pub all: bool,

    /// Output format (table, json)
    #[arg(short, long, default_value = "table")]
    pub output: String,
}

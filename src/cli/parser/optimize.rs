//! Optimize command arguments

use clap::{Parser, Subcommand};

/// Optimize subcommands
#[derive(Subcommand, Debug)]
pub enum OptimizeCommands {
    /// Compact small data files into larger ones
    Data(OptimizeDataArgs),

    /// Rewrite and compact manifest files
    Manifests(OptimizeManifestsArgs),
}

/// Arguments for optimize data command
#[derive(Parser, Debug)]
pub struct OptimizeDataArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Branch to optimize (defaults to main/current)
    #[arg(short, long)]
    pub branch: Option<String>,

    /// Target file size in bytes (default: 256MB)
    #[arg(long, default_value = "268435456")]
    pub target_size: u64,

    /// Maximum number of concurrent tasks
    #[arg(long, default_value = "4")]
    pub max_concurrent_tasks: usize,

    /// Only optimize files smaller than this size (bytes)
    #[arg(long)]
    pub min_file_size: Option<u64>,

    /// Optimize specific partition (e.g., "day=2024-01-01/currency=USD")
    #[arg(long)]
    pub partition: Option<String>,

    /// Optimize all partitions (required if --partition not specified)
    #[arg(long)]
    pub all_partitions: bool,

    /// Enable Z-ordering on specified columns
    #[arg(long, value_delimiter = ',')]
    pub zorder: Option<Vec<String>>,

    /// Maximum number of files to compact in this run (for incremental compaction)
    #[arg(long)]
    pub max_files: Option<usize>,

    /// Maximum bytes to process in this run (e.g., "10GB", "500MB")
    #[arg(long)]
    pub max_bytes: Option<String>,

    /// Dry run mode - show what would be done without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for optimize manifests command
#[derive(Parser, Debug)]
pub struct OptimizeManifestsArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Branch to optimize (defaults to main/current)
    #[arg(short, long)]
    pub branch: Option<String>,

    /// Target manifest size in bytes (default: 8MB)
    #[arg(long, default_value = "8388608")]
    pub target_size: u64,

    /// Minimum number of manifests to trigger rewrite
    #[arg(long, default_value = "5")]
    pub min_manifests: usize,

    /// Dry run mode - show what would be done without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

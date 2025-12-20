//! Optimize command arguments

use clap::{Parser, Subcommand};

use super::VacuumArgs;

/// Optimize subcommands
#[derive(Subcommand, Debug)]
pub enum OptimizeCommands {
    /// Compact small data files
    Compact(CompactArgs),

    /// Rewrite manifest files
    Manifests(OptimizeManifestsArgs),

    /// Clean up unreferenced files
    Vacuum(VacuumArgs),
}

/// Arguments for compact command
#[derive(Parser, Debug)]
pub struct CompactArgs {
    /// Branch to optimize
    #[arg(short, long)]
    pub branch: Option<String>,

    /// Target file size in bytes
    #[arg(long, default_value = "268435456")]
    pub target_size: u64,

    /// Max concurrent tasks
    #[arg(long, default_value = "4")]
    pub max_concurrent_tasks: usize,

    /// Min file size to optimize (bytes)
    #[arg(long)]
    pub min_file_size: Option<u64>,

    /// Specific partition to optimize
    #[arg(long)]
    pub partition: Option<String>,

    /// Optimize all partitions
    #[arg(long)]
    pub all_partitions: bool,

    /// Z-order columns (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub zorder: Option<Vec<String>>,

    /// Max files to compact
    #[arg(long)]
    pub max_files: Option<usize>,

    /// Max bytes to process (e.g., 10GB)
    #[arg(long)]
    pub max_bytes: Option<String>,

    /// Preview without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for optimize manifests command
#[derive(Parser, Debug)]
pub struct OptimizeManifestsArgs {
    /// Branch to optimize
    #[arg(short, long)]
    pub branch: Option<String>,

    /// Target manifest size in bytes
    #[arg(long, default_value = "8388608")]
    pub target_size: u64,

    /// Min manifests to trigger rewrite
    #[arg(long, default_value = "5")]
    pub min_manifests: usize,

    /// Preview without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

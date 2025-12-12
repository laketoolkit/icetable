//! Snapshot command arguments

use clap::{Parser, Subcommand};

/// Arguments for snapshot command
#[derive(Parser, Debug)]
pub struct SnapshotArgs {
    /// Snapshot subcommand
    #[command(subcommand)]
    pub command: SnapshotCommands,
}

/// Snapshot subcommands
#[derive(Subcommand, Debug)]
pub enum SnapshotCommands {
    /// List all snapshots
    #[command(name = "ls")]
    Ls(SnapshotListArgs),

    /// Create a new snapshot/checkpoint
    Create(SnapshotCreateArgs),

    /// Expire old snapshots
    Expire(SnapshotExpireArgs),

    /// Set current snapshot (time travel)
    Set(SnapshotSetArgs),

    /// Cherry-pick changes from another snapshot
    Cherrypick(SnapshotCherrypickArgs),

    /// Show snapshot lineage (parent chain)
    Lineage(SnapshotLineageArgs),
}

/// Arguments for snapshot list
#[derive(Parser, Debug)]
pub struct SnapshotListArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Maximum number of snapshots to show
    #[arg(short = 'n', long, default_value = "10")]
    pub limit: usize,

    /// Show all snapshots (no limit)
    #[arg(long)]
    pub all: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot create
#[derive(Parser, Debug)]
pub struct SnapshotCreateArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Force checkpoint creation even if not needed
    #[arg(long)]
    pub force: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot expire
#[derive(Parser, Debug)]
pub struct SnapshotExpireArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Branch to expire snapshots from (defaults to main/current)
    #[arg(short, long)]
    pub branch: Option<String>,

    /// Expire snapshots older than this time (e.g., "7d", "24h", "2w", or "2024-01-15")
    #[arg(long)]
    pub older_than: Option<String>,

    /// Keep the last N snapshots (minimum 1)
    #[arg(long)]
    pub retain_last: Option<usize>,

    /// Specific snapshot IDs to expire (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub ids: Option<Vec<i64>>,

    /// Dry run - show what would be expired without actually expiring
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot set
#[derive(Parser, Debug)]
pub struct SnapshotSetArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Snapshot ID to set as current
    #[arg(long, conflicts_with_all = ["as_of", "branch", "tag"])]
    pub id: Option<i64>,

    /// Set to snapshot as of time (e.g., "7d", "24h", or "2024-01-15")
    #[arg(long, conflicts_with_all = ["id", "branch", "tag"])]
    pub as_of: Option<String>,

    /// Set to snapshot referenced by branch name
    #[arg(long, conflicts_with_all = ["id", "as_of", "tag"])]
    pub branch: Option<String>,

    /// Set to snapshot referenced by tag name
    #[arg(long, conflicts_with_all = ["id", "as_of", "branch"])]
    pub tag: Option<String>,

    /// Dry run - show what would change without actually setting
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot cherrypick
#[derive(Parser, Debug)]
pub struct SnapshotCherrypickArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Source snapshot ID to cherry-pick from
    #[arg(long)]
    pub snapshot_id: i64,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot lineage
#[derive(Parser, Debug)]
pub struct SnapshotLineageArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Snapshot ID to show lineage for (defaults to current)
    pub snapshot_id: Option<i64>,

    /// Number of snapshots to show (default: 10)
    #[arg(short = 'n', long, default_value = "10")]
    pub limit: usize,

    /// Show all snapshots in lineage
    #[arg(short, long)]
    pub all: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot diff
#[derive(Parser, Debug)]
pub struct SnapshotDiffArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// First snapshot ID (older)
    #[arg(long)]
    pub from: i64,

    /// Second snapshot ID (newer, defaults to current)
    #[arg(long)]
    pub to: Option<i64>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

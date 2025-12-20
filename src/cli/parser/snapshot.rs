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
    /// List snapshots
    #[command(name = "ls")]
    Ls(SnapshotListArgs),

    /// Create snapshot/checkpoint
    Create(SnapshotCreateArgs),

    /// Expire old snapshots
    Expire(SnapshotExpireArgs),

    /// Set current snapshot (time travel)
    Set(SnapshotSetArgs),

    /// Cherry-pick from another snapshot
    Cherrypick(SnapshotCherrypickArgs),

    /// Show snapshot lineage
    Lineage(SnapshotLineageArgs),
}

/// Arguments for snapshot list
#[derive(Parser, Debug)]
pub struct SnapshotListArgs {
    /// Max snapshots to show
    #[arg(long, default_value = "10")]
    pub limit: usize,

    /// Show all snapshots
    #[arg(long)]
    pub all: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot create
#[derive(Parser, Debug)]
pub struct SnapshotCreateArgs {
    /// Force checkpoint even if not needed
    #[arg(long)]
    pub force: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot expire
#[derive(Parser, Debug)]
pub struct SnapshotExpireArgs {
    /// Branch to expire from
    #[arg(short, long)]
    pub branch: Option<String>,

    /// Expire all snapshots except current (or last N with --keep)
    #[arg(long)]
    pub all: bool,

    /// Keep last N snapshots (use with --all, default: 1)
    #[arg(long, default_value_if("all", "true", "1"))]
    pub keep: Option<usize>,

    /// Expire older than (e.g., 7d, 24h, 2w)
    #[arg(long)]
    pub older_than: Option<String>,

    /// Snapshot IDs to expire (space or comma-separated)
    #[arg(long, num_args = 1.., value_delimiter = ' ')]
    pub id: Option<Vec<i64>>,

    /// Preview without expiring
    #[arg(long)]
    pub dry_run: bool,

    /// Skip confirmation prompt
    #[arg(short, long)]
    pub force: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot set
#[derive(Parser, Debug)]
pub struct SnapshotSetArgs {
    /// Snapshot ID to set
    #[arg(long, conflicts_with_all = ["as_of", "ref_branch", "tag"])]
    pub id: Option<i64>,

    /// Set by time (e.g., 7d, 24h, 2024-01-15)
    #[arg(long, conflicts_with_all = ["id", "ref_branch", "tag"])]
    pub as_of: Option<String>,

    /// Set by branch name
    #[arg(long = "branch", conflicts_with_all = ["id", "as_of", "tag"])]
    pub ref_branch: Option<String>,

    /// Set by tag name
    #[arg(long, conflicts_with_all = ["id", "as_of", "ref_branch"])]
    pub tag: Option<String>,

    /// Preview without setting
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot cherrypick
#[derive(Parser, Debug)]
pub struct SnapshotCherrypickArgs {
    /// Source snapshot ID
    #[arg(long)]
    pub snapshot_id: i64,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot lineage
#[derive(Parser, Debug)]
pub struct SnapshotLineageArgs {
    /// Snapshot ID (defaults to current)
    pub snapshot_id: Option<i64>,

    /// Max snapshots to show
    #[arg(long, default_value = "10")]
    pub limit: usize,

    /// Show all in lineage
    #[arg(short, long)]
    pub all: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for snapshot diff
#[derive(Parser, Debug)]
pub struct SnapshotDiffArgs {
    /// First snapshot ID (older)
    #[arg(long)]
    pub from: i64,

    /// Second snapshot ID (newer)
    #[arg(long)]
    pub to: Option<i64>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

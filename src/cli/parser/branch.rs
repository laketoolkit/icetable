//! Branch and tag command arguments

use clap::{Parser, Subcommand};

/// Arguments for branch command
#[derive(Parser, Debug)]
pub struct BranchArgs {
    /// Branch subcommand
    #[command(subcommand)]
    pub command: BranchCommands,
}

/// Branch subcommands
#[derive(Subcommand, Debug)]
pub enum BranchCommands {
    /// List branches
    #[command(name = "ls")]
    Ls(BranchListArgs),

    /// Create branch
    Create(BranchCreateArgs),

    /// Delete branch
    Delete(BranchDeleteArgs),

    /// Fast-forward branch to ref
    FastForward(BranchFastForwardArgs),

    /// Rename branch
    Rename(BranchRenameArgs),
}

/// Arguments for branch list
#[derive(Parser, Debug)]
pub struct BranchListArgs {
    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for branch create
#[derive(Parser, Debug)]
pub struct BranchCreateArgs {
    /// Branch name
    pub name: String,

    /// Source snapshot ID
    #[arg(long)]
    pub from_snapshot: Option<i64>,

    /// Max reference age (ms)
    #[arg(long)]
    pub max_ref_age_ms: Option<i64>,

    /// Min snapshots to keep
    #[arg(long)]
    pub min_snapshots_to_keep: Option<i32>,

    /// Max snapshot age (ms)
    #[arg(long)]
    pub max_snapshot_age_ms: Option<i64>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for branch delete
#[derive(Parser, Debug)]
pub struct BranchDeleteArgs {
    /// Branch name
    pub name: String,

    /// Preview without deleting
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for branch fast-forward
#[derive(Parser, Debug)]
pub struct BranchFastForwardArgs {
    /// Branch name
    pub name: String,

    /// Target snapshot or branch
    #[arg(long)]
    pub to: String,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for branch rename
#[derive(Parser, Debug)]
pub struct BranchRenameArgs {
    /// Current name
    pub old_name: String,

    /// New name
    pub new_name: String,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for tag command
#[derive(Parser, Debug)]
pub struct TagArgs {
    /// Tag subcommand
    #[command(subcommand)]
    pub command: TagCommands,
}

/// Tag subcommands
#[derive(Subcommand, Debug)]
pub enum TagCommands {
    /// List tags
    #[command(name = "ls")]
    Ls(TagListArgs),

    /// Create tag
    Create(TagCreateArgs),

    /// Delete tag
    Delete(TagDeleteArgs),

    /// Rename tag
    Rename(TagRenameArgs),
}

/// Arguments for tag list
#[derive(Parser, Debug)]
pub struct TagListArgs {
    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for tag create
#[derive(Parser, Debug)]
pub struct TagCreateArgs {
    /// Tag name
    pub name: String,

    /// Snapshot ID to tag
    #[arg(long)]
    pub snapshot_id: Option<i64>,

    /// Max reference age (ms)
    #[arg(long)]
    pub max_ref_age_ms: Option<i64>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for tag delete
#[derive(Parser, Debug)]
pub struct TagDeleteArgs {
    /// Tag name
    pub name: String,

    /// Preview without deleting
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for tag rename
#[derive(Parser, Debug)]
pub struct TagRenameArgs {
    /// Current name
    pub old_name: String,

    /// New name
    pub new_name: String,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

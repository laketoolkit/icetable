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
    /// List all branches
    #[command(name = "ls")]
    Ls(BranchListArgs),

    /// Create a new branch
    Create(BranchCreateArgs),

    /// Delete a branch
    Delete(BranchDeleteArgs),

    /// Fast-forward a branch to another ref
    FastForward(BranchFastForwardArgs),

    /// Rename a branch
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
    /// Name for the new branch
    pub name: String,

    /// Snapshot ID to branch from (defaults to current)
    #[arg(long)]
    pub from_snapshot: Option<i64>,

    /// Maximum reference age in milliseconds for the branch
    #[arg(long)]
    pub max_ref_age_ms: Option<i64>,

    /// Minimum snapshots to keep on this branch
    #[arg(long)]
    pub min_snapshots_to_keep: Option<i32>,

    /// Maximum snapshot age in milliseconds
    #[arg(long)]
    pub max_snapshot_age_ms: Option<i64>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for branch delete
#[derive(Parser, Debug)]
pub struct BranchDeleteArgs {
    /// Name of branch to delete
    pub name: String,

    /// Dry run - show what would be deleted without actually deleting
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for branch fast-forward
#[derive(Parser, Debug)]
pub struct BranchFastForwardArgs {
    /// Name of branch to fast-forward
    pub name: String,

    /// Target snapshot ID or branch name
    #[arg(long)]
    pub to: String,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for branch rename
#[derive(Parser, Debug)]
pub struct BranchRenameArgs {
    /// Current branch name
    pub old_name: String,

    /// New branch name
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
    /// List all tags
    #[command(name = "ls")]
    Ls(TagListArgs),

    /// Create a new tag
    Create(TagCreateArgs),

    /// Delete a tag
    Delete(TagDeleteArgs),

    /// Rename a tag
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
    /// Name for the new tag
    pub name: String,

    /// Snapshot ID to tag (defaults to current)
    #[arg(long)]
    pub snapshot_id: Option<i64>,

    /// Maximum reference age in milliseconds
    #[arg(long)]
    pub max_ref_age_ms: Option<i64>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for tag delete
#[derive(Parser, Debug)]
pub struct TagDeleteArgs {
    /// Name of tag to delete
    pub name: String,

    /// Dry run - show what would be deleted without actually deleting
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for tag rename
#[derive(Parser, Debug)]
pub struct TagRenameArgs {
    /// Current tag name
    pub old_name: String,

    /// New tag name
    pub new_name: String,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

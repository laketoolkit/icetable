//! Command-line argument parsing

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// icectl - CLI for Apache Iceberg table management
#[derive(Parser, Debug)]
#[command(name = "icectl")]
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
    /// Analyze table health and get optimization recommendations
    Analyze(AnalyzeArgs),

    /// Create a new empty table
    Init(InitArgs),

    /// Inspect table contents and metadata
    Inspect(InspectArgs),

    /// Validate file integrity and quality
    Validate(ValidateArgs),

    /// Compare two tables
    Diff(DiffArgs),

    /// Compute statistics
    Stats(StatsArgs),

    /// View table version history
    History(HistoryArgs),

    /// Remove old files no longer referenced by the table
    Vacuum(VacuumArgs),

    /// Compact and optimize table data and metadata
    #[command(subcommand)]
    Optimize(OptimizeCommands),

    /// Manage table snapshots (list, create, expire, set)
    Snapshot(SnapshotArgs),

    /// Repair table metadata and fix inconsistencies
    Repair(RepairArgs),

    /// Import data into Iceberg from external sources
    #[command(subcommand)]
    Import(ImportCommands),

    /// Manage table branches
    Branch(BranchArgs),

    /// Manage table tags
    Tag(TagArgs),

    /// Manage configuration (default table context)
    Config(ConfigArgs),

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

/// Arguments for vacuum command
#[derive(Parser, Debug)]
pub struct VacuumArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Retention period (e.g., "7d", "168h", or hours as number)
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
    List(SnapshotListArgs),

    /// Create a new snapshot/checkpoint
    Create(SnapshotCreateArgs),

    /// Expire old snapshots
    Expire(SnapshotExpireArgs),

    /// Set current snapshot (time travel)
    Set(SnapshotSetArgs),

    /// Cherry-pick changes from another snapshot
    Cherrypick(SnapshotCherrypickArgs),
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
    #[arg(long, conflicts_with = "as_of")]
    pub id: Option<i64>,

    /// Set to snapshot as of time (e.g., "7d", "24h", or "2024-01-15")
    #[arg(long, conflicts_with = "id")]
    pub as_of: Option<String>,

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

/// Arguments for repair command
#[derive(Parser, Debug)]
pub struct RepairArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Table format (delta, iceberg) - auto-detected if not specified
    #[arg(short, long, value_parser = ["delta", "iceberg"])]
    pub format: Option<String>,

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
    #[arg(short = 'o', long, default_value = "text")]
    pub output: String,
}

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
    List(BranchListArgs),

    /// Create a new branch
    Create(BranchCreateArgs),

    /// Delete a branch
    Delete(BranchDeleteArgs),

    /// Fast-forward a branch to another ref
    FastForward(BranchFastForwardArgs),
}

/// Arguments for branch list
#[derive(Parser, Debug)]
pub struct BranchListArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for branch create
#[derive(Parser, Debug)]
pub struct BranchCreateArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

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
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

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
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Name of branch to fast-forward
    pub name: String,

    /// Target snapshot ID or branch name
    #[arg(long)]
    pub to: String,

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
    List(TagListArgs),

    /// Create a new tag
    Create(TagCreateArgs),

    /// Delete a tag
    Delete(TagDeleteArgs),
}

/// Arguments for tag list
#[derive(Parser, Debug)]
pub struct TagListArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for tag create
#[derive(Parser, Debug)]
pub struct TagCreateArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

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
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Name of tag to delete
    pub name: String,

    /// Dry run - show what would be deleted without actually deleting
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Import subcommands
#[derive(Subcommand, Debug)]
pub enum ImportCommands {
    /// Import from a Delta Lake table
    #[cfg(feature = "delta")]
    Delta(ImportDeltaArgs),

    /// Import from Parquet files
    Parquet(ImportParquetArgs),
}

/// Arguments for import delta command
#[cfg(feature = "delta")]
#[derive(Parser, Debug)]
pub struct ImportDeltaArgs {
    /// Path to source Delta Lake table
    pub source: String,

    /// Path to target Iceberg table
    pub target: String,

    /// Table name (for new tables)
    #[arg(long)]
    pub name: Option<String>,

    /// Partition columns (comma-separated, for new tables)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Dry run - show what would be imported without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for import parquet command
#[derive(Parser, Debug)]
pub struct ImportParquetArgs {
    /// Path to Parquet file or directory of Parquet files
    pub source: String,

    /// Path to target Iceberg table
    pub target: String,

    /// Table name (for new tables)
    #[arg(long)]
    pub name: Option<String>,

    /// Partition columns (comma-separated, for new tables)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Glob pattern for selecting files (e.g., "*.parquet", "**/*.parquet")
    #[arg(long, default_value = "*.parquet")]
    pub pattern: String,

    /// Dry run - show what would be imported without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for config command
#[derive(Parser, Debug)]
pub struct ConfigArgs {
    /// Config subcommand
    #[command(subcommand)]
    pub command: ConfigCommands,
}

/// Config subcommands
#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Set the current table context
    Use(ConfigUseArgs),

    /// Show the current table context
    Current(ConfigCurrentArgs),

    /// Unset the current table context
    Unset(ConfigUnsetArgs),

    /// Add a named table alias
    Add(ConfigAddArgs),

    /// Remove a named table alias
    Remove(ConfigRemoveArgs),

    /// List all configured tables
    List(ConfigListArgs),
}

/// Arguments for config use
#[derive(Parser, Debug)]
pub struct ConfigUseArgs {
    /// Table path or alias name to use as default
    pub table: String,
}

/// Arguments for config current
#[derive(Parser, Debug)]
pub struct ConfigCurrentArgs {
    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for config unset
#[derive(Parser, Debug)]
pub struct ConfigUnsetArgs {}

/// Arguments for config add
#[derive(Parser, Debug)]
pub struct ConfigAddArgs {
    /// Alias name for the table
    pub name: String,

    /// Table path (local or s3://, gs://, etc.)
    pub path: String,

    /// Optional description
    #[arg(short, long)]
    pub description: Option<String>,
}

/// Arguments for config remove
#[derive(Parser, Debug)]
pub struct ConfigRemoveArgs {
    /// Alias name to remove
    pub name: String,
}

/// Arguments for config list
#[derive(Parser, Debug)]
pub struct ConfigListArgs {
    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Parse log level from string
fn parse_log_level(s: &str) -> Result<crate::utils::LogLevel, String> {
    s.parse()
}

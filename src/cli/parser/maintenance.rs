//! Maintenance command arguments (vacuum, repair, doctor)

use clap::Parser;
use std::path::PathBuf;

/// Arguments for vacuum command
#[derive(Parser, Debug)]
pub struct VacuumArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Branch to vacuum (defaults to scanning all branches)
    #[arg(short, long)]
    pub branch: Option<String>,

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

/// Arguments for doctor command
#[derive(Parser, Debug)]
pub struct DoctorArgs {
    /// Path to table (uses default from config if not provided)
    #[arg(short = 't', long = "table")]
    pub path: Option<String>,

    /// Also verify that all data files referenced in manifests exist (slow)
    #[arg(long)]
    pub check_files: bool,

    /// Validate specific catalog by name
    #[arg(long)]
    pub catalog: Option<String>,

    /// Validate storage connectivity
    #[arg(long)]
    pub storage: bool,

    /// Output format: human or json
    #[arg(short, long, default_value = "human")]
    pub output: String,
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

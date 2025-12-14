//! Maintenance command arguments (vacuum, repair, doctor)

use clap::Parser;
use std::path::PathBuf;

/// Arguments for vacuum command
#[derive(Parser, Debug)]
pub struct VacuumArgs {
    /// Branch to vacuum
    #[arg(short, long)]
    pub branch: Option<String>,

    /// Retention period (e.g., 7d, 168h)
    #[arg(short, long, default_value = "168")]
    pub retention_hours: u64,

    /// Preview without deleting
    #[arg(long)]
    pub dry_run: bool,

    /// Force even if below safety threshold
    #[arg(long)]
    pub force: bool,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: String,
}

/// Arguments for repair command
#[derive(Parser, Debug)]
pub struct RepairArgs {
    /// Preview without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Sync metadata with files on disk
    #[arg(long)]
    pub sync_metadata: bool,

    /// Remove refs to missing files
    #[arg(long)]
    pub remove_missing: bool,

    /// Add untracked parquet files
    #[arg(long)]
    pub add_orphans: bool,

    /// Output format (text, json)
    #[arg(short = 'o', long, default_value = "text")]
    pub output: String,
}

/// Arguments for doctor command
#[derive(Parser, Debug)]
pub struct DoctorArgs {
    /// Verify all data files exist (slow)
    #[arg(long)]
    pub check_files: bool,

    /// Check specific catalog
    #[arg(long)]
    pub catalog: Option<String>,

    /// Check storage connectivity
    #[arg(long)]
    pub storage: bool,

    /// Output format (human, json)
    #[arg(short, long, default_value = "human")]
    pub output: String,
}

/// Arguments for init command
#[derive(Parser, Debug)]
pub struct InitArgs {
    /// Table location path
    pub path: String,

    /// Schema file (JSON)
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

    /// Properties (key=value, comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub properties: Option<Vec<String>>,
}

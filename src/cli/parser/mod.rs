//! Command-line argument parsing
//!
//! This module defines all CLI arguments and commands for icetable.
//! Each submodule contains arguments for a specific command group.
//!
//! # Module Structure
//!
//! - `inspect` - Table inspection commands (inspect, validate, diff, stats, analyze, history)
//! - `optimize` - Optimization commands (data compaction, manifest rewrite)
//! - `snapshot` - Snapshot management (list, create, expire, set, cherrypick, lineage)
//! - `branch` - Branch and tag management
//! - `catalog` - Catalog operations (namespaces, tables)
//! - `config` - Configuration management
//! - `import` - Data import from external sources
//! - `maintenance` - Table maintenance (vacuum, repair, doctor, init)
//! - `generate` - Test data generation and utilities

pub mod branch;
pub mod catalog;
pub mod config;
pub mod generate;
pub mod import;
pub mod inspect;
pub mod maintenance;
pub mod optimize;
pub mod snapshot;

// Re-export all argument types
pub use branch::*;
pub use catalog::*;
pub use config::*;
pub use generate::*;
pub use import::*;
pub use inspect::*;
pub use maintenance::*;
pub use optimize::*;
pub use snapshot::*;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::core::config::CredentialSource;

/// CLI for managing Apache Iceberg tables - inspect, optimize, vacuum, and more
#[derive(Parser, Debug)]
#[command(name = "icetable")]
#[command(version, about, long_about = None)]
#[command(disable_help_flag = true)]
pub struct Cli {
    /// Table name or path (e.g., "namespace.table" or "s3://bucket/path")
    #[arg(
        short = 't',
        long = "table",
        global = true,
        help_heading = "Global Options"
    )]
    pub table: Option<String>,

    /// Namespace (e.g., "db.schema")
    #[arg(
        short = 'n',
        long = "namespace",
        global = true,
        help_heading = "Global Options"
    )]
    pub namespace: Option<String>,

    /// Suppress non-error output
    #[arg(short = 'q', long, global = true, help_heading = "Global Options")]
    pub quiet: bool,

    /// Log level [default: off]
    #[arg(long, global = true, default_value = "off", value_parser = parse_log_level, hide_default_value = true, help_heading = "Global Options")]
    pub log_level: crate::utils::LogLevel,

    /// Log to file
    #[arg(long, global = true, help_heading = "Global Options")]
    pub log_file: Option<PathBuf>,

    // ═══════════════════════════════════════════════════════════════════════════
    // Hidden global options - use `icetable options` to see all
    // ═══════════════════════════════════════════════════════════════════════════
    /// REST Catalog URI for ad-hoc catalog access
    #[arg(long, global = true, env = "ICETABLE_CATALOG_URI", hide = true)]
    pub catalog_uri: Option<String>,

    /// Catalog warehouse location
    #[arg(long, global = true, env = "ICETABLE_CATALOG_WAREHOUSE", hide = true)]
    pub catalog_warehouse: Option<String>,

    /// Catalog credential (client_id:client_secret)
    #[arg(long, global = true, env = "ICETABLE_CATALOG_CREDENTIAL", hide = true)]
    pub catalog_credential: Option<String>,

    /// Catalog credential from environment variable
    #[arg(
        long,
        global = true,
        env = "ICETABLE_CATALOG_CREDENTIAL_ENV",
        hide = true
    )]
    pub catalog_credential_env: Option<String>,

    /// Catalog credential from file
    #[arg(
        long,
        global = true,
        env = "ICETABLE_CATALOG_CREDENTIAL_FILE",
        hide = true
    )]
    pub catalog_credential_file: Option<std::path::PathBuf>,

    /// Use IAM role for authentication (AWS, GCP, Azure)
    #[arg(long, global = true, hide = true)]
    pub catalog_use_iam_role: bool,

    /// Use OAuth2 for authentication
    #[arg(long, global = true, hide = true)]
    pub catalog_use_oauth2: bool,

    /// Maximum memory usage (e.g., 2GB, 512MB). 0 = unlimited
    #[arg(
        long,
        global = true,
        default_value = "0",
        env = "ICETABLE_MAX_MEMORY",
        hide = true
    )]
    pub max_memory: String,

    /// Operation timeout in seconds. 0 = no timeout
    #[arg(
        long,
        global = true,
        default_value = "0",
        env = "ICETABLE_TIMEOUT",
        hide = true
    )]
    pub timeout: u64,

    /// Maximum concurrent operations
    #[arg(
        long,
        global = true,
        default_value = "0",
        env = "ICETABLE_MAX_CONCURRENCY",
        hide = true
    )]
    pub max_concurrency: u32,

    /// Maximum worker threads for runtime. 0 = use system default
    #[arg(
        long,
        global = true,
        default_value = "0",
        env = "ICETABLE_MAX_THREADS",
        hide = true
    )]
    pub max_threads: usize,

    /// Print help
    #[arg(short, long, action = clap::ArgAction::Help, global = true)]
    pub help: Option<bool>,

    /// The subcommand to execute
    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List namespaces or tables in a catalog
    Ls(LsArgs),

    /// Create a namespace or table in a catalog
    Create(CreateArgs),

    /// Delete a namespace or table from a catalog
    Delete(DeleteArgs),

    /// Analyze table health and get optimization recommendations
    Analyze(AnalyzeArgs),

    /// Create a new empty table (local path)
    Init(InitArgs),

    /// Inspect table contents and metadata
    Inspect(InspectArgs),

    /// Validate file integrity and quality
    Validate(ValidateArgs),

    /// Compare snapshots, branches, or tags
    Diff(DiffArgs),

    /// Compute statistics
    Stats(StatsArgs),

    /// View table version history
    History(HistoryArgs),

    /// Clean up old files no longer referenced by the table
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

    /// Manage configuration (aliases, catalogs)
    Config(ConfigArgs),

    /// Generate synthetic test data for benchmarking and testing
    Generate(GenerateArgs),

    /// Generate shell completions
    Completions(CompletionsArgs),

    /// Diagnose environment health (credentials, connectivity)
    Doctor(DoctorArgs),

    /// Print all global options (kubectl style)
    Options,

    /// Interactive Terminal UI
    #[cfg(feature = "tui")]
    Tui(TuiArgs),
}

/// Global context for table operations
///
/// Contains the global options from CLI that are relevant to table operations.
/// This is passed to commands instead of individual parameters.
#[derive(Debug, Clone)]
pub struct TableContext {
    /// Table name or path (e.g., "namespace.table" or "s3://bucket/path")
    pub table: Option<String>,
    /// Namespace (e.g., "db.schema")
    pub namespace: Option<String>,
    /// Catalog configuration from CLI
    pub catalog_config: Option<crate::core::CatalogConfig>,
}

impl TableContext {
    /// Get the full table reference, combining namespace and table if both are present
    ///
    /// If both namespace and table are specified, returns "namespace.table".
    /// If only table is specified, returns the table as-is.
    /// If neither is specified, returns None.
    pub fn table_ref(&self) -> Option<String> {
        match (&self.namespace, &self.table) {
            (Some(ns), Some(t)) => {
                // If table already contains namespace (has '.'), use it as-is
                if t.contains('.')
                    || t.starts_with("s3://")
                    || t.starts_with("gs://")
                    || t.starts_with("az://")
                    || t.starts_with("file://")
                    || t.starts_with("/")
                {
                    Some(t.clone())
                } else {
                    Some(format!("{}.{}", ns, t))
                }
            }
            (None, Some(t)) => Some(t.clone()),
            _ => None,
        }
    }
}

impl Cli {
    /// Build catalog configuration from CLI options
    pub fn catalog_config(&self) -> Option<crate::core::CatalogConfig> {
        self.catalog_uri.as_ref().map(|uri| {
            let mut config = crate::core::CatalogConfig::rest(uri);
            if let Some(ref warehouse) = self.catalog_warehouse {
                config = config.with_warehouse(warehouse);
            }

            // Build credential source from CLI options
            let credential_source = CredentialSource::from_cli_options(
                self.catalog_credential.clone(),
                self.catalog_credential_env.clone(),
                self.catalog_credential_file.clone(),
                self.catalog_use_iam_role,
                self.catalog_use_oauth2,
            );

            // Set credential source if present
            if let Some(source) = credential_source {
                config = config.with_credential(source);
            }

            config
        })
    }

    /// Build table context from CLI global options
    pub fn table_context(&self) -> TableContext {
        TableContext {
            table: self.table.clone(),
            namespace: self.namespace.clone(),
            catalog_config: self.catalog_config(),
        }
    }
}

/// Parse key=value pairs
pub fn parse_key_value(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=VALUE: no `=` found in `{s}`"))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}

/// Parse log level from string
fn parse_log_level(s: &str) -> Result<crate::utils::LogLevel, String> {
    s.parse()
}

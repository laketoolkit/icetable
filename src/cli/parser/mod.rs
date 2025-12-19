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

pub mod admin;
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
pub use admin::*;
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

/// CLI for managing Apache Iceberg tables - inspect, optimize, and more
#[derive(Parser, Debug)]
#[command(name = "icetable")]
#[command(version, about, long_about = None)]
#[command(disable_help_flag = true)]
#[command(max_term_width = 100, next_line_help = false)]
pub struct Cli {
    /// Table name or path
    #[arg(
        short = 't',
        long = "table",
        global = true,
        help_heading = "Global Options"
    )]
    pub table: Option<String>,

    /// Namespace
    #[arg(
        short = 'n',
        long = "namespace",
        global = true,
        help_heading = "Global Options"
    )]
    pub namespace: Option<String>,

    /// Catalog name (from config)
    #[arg(
        short = 'c',
        long = "catalog",
        global = true,
        help_heading = "Global Options"
    )]
    pub catalog: Option<String>,

    /// Warehouse within catalog
    #[arg(
        short = 'w',
        long = "warehouse",
        global = true,
        help_heading = "Global Options"
    )]
    pub warehouse: Option<String>,

    /// Suppress non-error output
    #[arg(short = 'q', long, global = true, help_heading = "Global Options")]
    pub quiet: bool,

    /// Log level
    #[arg(long, global = true, default_value = "off", value_parser = parse_log_level, hide_default_value = true, help_heading = "Global Options")]
    pub log_level: crate::utils::LogLevel,

    /// Log to file
    #[arg(long, global = true, help_heading = "Global Options")]
    pub log_file: Option<PathBuf>,

    // ═══════════════════════════════════════════════════════════════════════════
    // Hidden global options - use `icetable options` to see all
    // ═══════════════════════════════════════════════════════════════════════════
    /// REST Catalog URI
    #[arg(long, global = true, env = "ICETABLE_CATALOG_URI", hide = true)]
    pub catalog_uri: Option<String>,

    /// Catalog warehouse location
    #[arg(long, global = true, env = "ICETABLE_CATALOG_WAREHOUSE", hide = true)]
    pub catalog_warehouse: Option<String>,

    /// Catalog credential (id:secret)
    #[arg(long, global = true, env = "ICETABLE_CATALOG_CREDENTIAL", hide = true)]
    pub catalog_credential: Option<String>,

    /// Credential from env var
    #[arg(
        long,
        global = true,
        env = "ICETABLE_CATALOG_CREDENTIAL_ENV",
        hide = true
    )]
    pub catalog_credential_env: Option<String>,

    /// Credential from file
    #[arg(
        long,
        global = true,
        env = "ICETABLE_CATALOG_CREDENTIAL_FILE",
        hide = true
    )]
    pub catalog_credential_file: Option<std::path::PathBuf>,

    /// Use IAM role for auth
    #[arg(long, global = true, hide = true)]
    pub catalog_use_iam_role: bool,

    /// Use OAuth2 for auth
    #[arg(long, global = true, hide = true)]
    pub catalog_use_oauth2: bool,

    /// Max memory (e.g., 2GB). 0=unlimited
    #[arg(
        long,
        global = true,
        default_value = "0",
        env = "ICETABLE_MAX_MEMORY",
        hide = true
    )]
    pub max_memory: String,

    /// Timeout in seconds. 0=none
    #[arg(
        long,
        global = true,
        default_value = "0",
        env = "ICETABLE_TIMEOUT",
        hide = true
    )]
    pub timeout: u64,

    /// Max concurrent operations
    #[arg(
        long,
        global = true,
        default_value = "0",
        env = "ICETABLE_MAX_CONCURRENCY",
        hide = true
    )]
    pub max_concurrency: u32,

    /// Max worker threads. 0=default
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
#[command(subcommand_value_name = "COMMAND", infer_subcommands = true)]
pub enum Commands {
    // ─────────────────────────────────────────────────────────────────────────
    // Data Operations
    // ─────────────────────────────────────────────────────────────────────────
    /// List namespaces or tables
    Ls(LsArgs),

    /// Create namespace or table
    Create(CreateArgs),

    /// Delete namespace or table
    Delete(DeleteArgs),

    /// Inspect table metadata and schema
    #[command(visible_alias = "i")]
    Inspect(InspectArgs),

    /// Analyze table health and get recommendations
    #[command(visible_alias = "a")]
    Analyze(AnalyzeArgs),

    /// Compute partition statistics
    Stats(StatsArgs),

    /// Validate file integrity
    Validate(ValidateArgs),

    /// Compare snapshots/branches/tags
    Diff(DiffArgs),

    /// Import from external sources
    #[command(subcommand)]
    Import(ImportCommands),

    // ─────────────────────────────────────────────────────────────────────────
    // Maintenance
    // ─────────────────────────────────────────────────────────────────────────
    /// Optimize table (compact, vacuum, manifests)
    #[command(subcommand, visible_alias = "o")]
    Optimize(OptimizeCommands),

    /// Repair table metadata
    Repair(RepairArgs),

    // ─────────────────────────────────────────────────────────────────────────
    // Version Control
    // ─────────────────────────────────────────────────────────────────────────
    /// View version history
    #[command(visible_alias = "h")]
    History(HistoryArgs),

    /// Manage snapshots
    #[command(visible_alias = "s")]
    Snapshot(SnapshotArgs),

    /// Manage branches
    Branch(BranchArgs),

    /// Manage tags
    Tag(TagArgs),

    // ─────────────────────────────────────────────────────────────────────────
    // Administration
    // ─────────────────────────────────────────────────────────────────────────
    /// Administrative operations (config, warehouses, auth)
    Admin(AdminArgs),

    /// Diagnose environment health
    Doctor(DoctorArgs),

    // ─────────────────────────────────────────────────────────────────────────
    // Utilities
    // ─────────────────────────────────────────────────────────────────────────
    /// Generate synthetic test data
    Generate(GenerateArgs),

    /// Generate shell completions
    Completions(CompletionsArgs),

    /// Print all global options
    Options,

    /// Interactive Terminal UI
    #[cfg(feature = "tui")]
    Tui(TuiArgs),
}

// Re-export CatalogContext from core for CLI commands
pub use crate::core::resolution::CatalogContext;

impl Cli {
    /// Build catalog configuration from CLI options
    pub fn catalog_config(&self) -> Option<crate::core::CatalogConfig> {
        self.catalog_uri.as_ref().map(|uri| {
            let mut config = crate::core::CatalogConfig::rest(uri);
            if let Some(ref warehouse) = self.catalog_warehouse {
                config = config.with_warehouse(warehouse);
            }

            // Build auth from CLI options
            let credential_source = CredentialSource::from_cli_options(
                self.catalog_credential.clone(),
                self.catalog_credential_env.clone(),
                self.catalog_credential_file.clone(),
                self.catalog_use_iam_role,
                self.catalog_use_oauth2,
            );

            // Set auth if credential source is present
            if let Some(source) = credential_source {
                config = config.with_auth(crate::core::CatalogAuth::bearer(source));
            }

            config
        })
    }

    /// Build table context from CLI global options
    pub fn table_context(&self) -> CatalogContext {
        CatalogContext {
            table: self.table.clone(),
            namespace: self.namespace.clone(),
            catalog: self.catalog.clone(),
            warehouse: self.warehouse.clone(),
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

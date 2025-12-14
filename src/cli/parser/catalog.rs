//! Catalog command arguments
//!
//! Explicit subcommands for catalog operations:
//! - `ls namespaces` / `ls tables` / `ls` (auto-detect)
//! - `create namespace <name>` / `create table <name> --schema ...`
//! - `delete namespace <name>` / `delete table <name> [<name>...]`
//!
//! Note: --catalog and --warehouse are global options (use -c and -w)

use clap::{Parser, Subcommand};
use std::path::PathBuf;

// ============================================================================
// LS COMMAND
// ============================================================================

/// Arguments for ls command (list namespaces or tables)
#[derive(Parser, Debug)]
pub struct LsArgs {
    /// What to list (auto-detects if omitted)
    #[command(subcommand)]
    pub command: Option<LsCommands>,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text", global = true)]
    pub output: String,
}

/// What to list
#[derive(Subcommand, Debug)]
pub enum LsCommands {
    /// List namespaces
    Namespaces,
    /// List tables
    Tables,
}

// ============================================================================
// CREATE COMMAND
// ============================================================================

/// Arguments for create command
#[derive(Parser, Debug)]
pub struct CreateArgs {
    /// What to create
    #[command(subcommand)]
    pub command: CreateCommands,
}

/// Create subcommands
#[derive(Subcommand, Debug)]
pub enum CreateCommands {
    /// Create a namespace
    Namespace(NamespaceCreateArgs),
    /// Create a table
    Table(TableCreateArgs),
}

/// Arguments for creating a namespace
#[derive(Parser, Debug)]
pub struct NamespaceCreateArgs {
    /// Namespace name
    pub name: String,

    /// Properties (key=value, repeatable)
    #[arg(short, long, value_parser = super::parse_key_value)]
    pub property: Vec<(String, String)>,
}

/// Arguments for creating a table
#[derive(Parser, Debug)]
pub struct TableCreateArgs {
    /// Table name
    pub name: String,

    /// Schema file (JSON)
    #[arg(long)]
    pub schema: PathBuf,

    /// Partition columns (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub partition_by: Option<Vec<String>>,

    /// Table location (optional)
    #[arg(long)]
    pub location: Option<String>,

    /// Properties (key=value, repeatable)
    #[arg(short, long, value_parser = super::parse_key_value)]
    pub property: Vec<(String, String)>,
}

// ============================================================================
// DELETE COMMAND
// ============================================================================

/// Arguments for delete command
#[derive(Parser, Debug)]
pub struct DeleteArgs {
    /// What to delete
    #[command(subcommand)]
    pub command: DeleteCommands,
}

/// Delete subcommands
#[derive(Subcommand, Debug)]
pub enum DeleteCommands {
    /// Delete a namespace
    Namespace(NamespaceDeleteArgs),
    /// Delete one or more tables
    Table(TableDeleteArgs),
}

/// Arguments for deleting a namespace
#[derive(Parser, Debug)]
pub struct NamespaceDeleteArgs {
    /// Namespace name
    pub name: String,

    /// Force delete even if not empty
    #[arg(long)]
    pub force: bool,
}

/// Arguments for deleting tables
#[derive(Parser, Debug)]
pub struct TableDeleteArgs {
    /// Table names
    #[arg(required = true)]
    pub names: Vec<String>,

    /// Purge data files when deleting
    #[arg(long)]
    pub purge: bool,
}

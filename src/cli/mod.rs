//! CLI layer for TableTools
//!
//! This module handles command-line argument parsing, output formatting,
//! and user interaction.

pub mod parser;
pub mod commands;
pub mod output;

pub use parser::{Cli, Commands};
pub use output::OutputFormatter;

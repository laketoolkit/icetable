//! CLI layer for TableTools
//!
//! This module handles command-line argument parsing, output formatting,
//! and user interaction.

pub mod commands;
pub mod output;
pub mod parser;

pub use output::OutputFormatter;
pub use parser::{Cli, Commands};

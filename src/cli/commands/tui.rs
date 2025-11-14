//! TUI command implementation

use crate::cli::parser::TuiArgs;
use crate::error::{Error, Result};

/// Handler for tui command
pub struct TuiCommand;

impl TuiCommand {
    /// Execute tui command
    pub async fn execute(_args: TuiArgs) -> Result<()> {
        // Phase 4 implementation using ratatui
        Err(Error::UnsupportedFeature {
            feature: "tui command not yet implemented (Phase 4)".to_string(),
        })
    }
}

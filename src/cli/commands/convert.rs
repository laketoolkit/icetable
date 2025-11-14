//! Convert command implementation

use crate::cli::parser::ConvertArgs;
use crate::error::{Error, Result};

/// Handler for convert command
pub struct ConvertCommand;

impl ConvertCommand {
    /// Execute convert command
    pub async fn execute(_args: ConvertArgs) -> Result<()> {
        // Phase 2 implementation
        Err(Error::UnsupportedFeature {
            feature: "convert command not yet implemented (Phase 2)".to_string(),
        })
    }
}

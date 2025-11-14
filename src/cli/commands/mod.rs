//! CLI command implementations

pub mod convert;
pub mod diff;
pub mod inspect;
pub mod query;
pub mod stats;
pub mod validate;

#[cfg(feature = "tui")]
pub mod tui;

// Re-export command handlers
pub use convert::ConvertCommand;
pub use diff::DiffCommand;
pub use inspect::InspectCommand;
pub use query::QueryCommand;
pub use stats::StatsCommand;
pub use validate::ValidateCommand;

#[cfg(feature = "tui")]
pub use tui::TuiCommand;

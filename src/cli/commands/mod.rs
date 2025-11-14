//! CLI command implementations

pub mod inspect;
pub mod validate;
pub mod diff;
pub mod convert;
pub mod stats;
pub mod query;

#[cfg(feature = "serve")]
pub mod serve;

// Re-export command handlers
pub use inspect::InspectCommand;
pub use validate::ValidateCommand;
pub use diff::DiffCommand;
pub use convert::ConvertCommand;
pub use stats::StatsCommand;
pub use query::QueryCommand;

#[cfg(feature = "serve")]
pub use serve::ServeCommand;

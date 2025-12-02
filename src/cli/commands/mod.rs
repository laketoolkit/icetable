//! CLI command implementations

pub mod convert;
pub mod diff;
pub mod history;
pub mod init;
pub mod inspect;
pub mod optimize;
pub mod repair;
pub mod restore;
pub mod snapshot;
pub mod stats;
pub mod vacuum;
pub mod validate;

#[cfg(feature = "tui")]
pub mod tui;

// Re-export command handlers
pub use convert::ConvertCommand;
pub use diff::DiffCommand;
pub use history::HistoryCommand;
pub use init::InitCommand;
pub use inspect::InspectCommand;
pub use optimize::OptimizeCommand;
pub use repair::RepairCommand;
pub use restore::RestoreCommand;
pub use snapshot::SnapshotCommand;
pub use stats::StatsCommand;
pub use vacuum::VacuumCommand;
pub use validate::ValidateCommand;

#[cfg(feature = "tui")]
pub use tui::TuiCommand;

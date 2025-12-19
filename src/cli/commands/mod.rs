//! CLI command implementations

pub mod admin;
pub mod analyze;
pub mod common;
pub mod config;
pub mod constants;
pub mod create;
pub mod delete;
pub mod diff;
pub mod doctor;
pub mod generate;
pub mod history;
pub mod import;
pub mod init;
pub mod inspect;
pub mod ls;
pub mod optimize;
pub mod refs;
pub mod repair;
pub mod snapshot;
pub mod stats;
pub mod vacuum;
pub mod validate;

#[cfg(feature = "tui")]
pub mod tui;

#[cfg(test)]
mod tests;

// Re-export command handlers
pub use admin::AdminCommand;
pub use analyze::AnalyzeCommand;
pub use config::ConfigCommand;
pub use create::CreateCommand;
pub use delete::DeleteCommand;
pub use diff::DiffCommand;
pub use doctor::DoctorCommand;
pub use generate::GenerateCommand;
pub use history::HistoryCommand;
pub use import::ImportCommand;
pub use init::InitCommand;
pub use inspect::InspectCommand;
pub use ls::LsCommand;
pub use optimize::OptimizeCommand;
pub use refs::{BranchCommand, TagCommand};
pub use repair::RepairCommand;
pub use snapshot::SnapshotCommand;
pub use stats::StatsCommand;
pub use vacuum::VacuumCommand;
pub use validate::ValidateCommand;

#[cfg(feature = "tui")]
pub use tui::TuiCommand;

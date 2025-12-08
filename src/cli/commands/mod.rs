//! CLI command implementations

pub mod analyze;
pub mod branch;
pub mod catalog;
pub mod common;
pub mod config;
pub mod diff;
pub mod doctor;
pub mod generate;
pub mod history;
pub mod import;
pub mod init;
pub mod inspect;
pub mod optimize;
pub mod repair;
pub mod snapshot;
pub mod stats;
pub mod tag;
pub mod vacuum;
pub mod validate;

#[cfg(feature = "tui")]
pub mod tui;

// Re-export command handlers
pub use analyze::AnalyzeCommand;
pub use branch::BranchCommand;
pub use catalog::CatalogCommand;
pub use config::ConfigCommand;
pub use diff::DiffCommand;
pub use doctor::DoctorCommand;
pub use generate::GenerateCommand;
pub use history::HistoryCommand;
pub use import::ImportCommand;
pub use init::InitCommand;
pub use inspect::InspectCommand;
pub use optimize::OptimizeCommand;
pub use repair::RepairCommand;
pub use snapshot::SnapshotCommand;
pub use stats::StatsCommand;
pub use tag::TagCommand;
pub use vacuum::VacuumCommand;
pub use validate::ValidateCommand;

#[cfg(feature = "tui")]
pub use tui::TuiCommand;

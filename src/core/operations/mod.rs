//! Core operations for table manipulation
//!
//! This module contains the business logic for operations like inspect, validate,
//! convert, generate, import, history, init, diff, and statistics. These operations
//! use the FormatHandler and StorageBackend abstractions to work with any supported
//! format and storage.

pub mod convert;
pub mod diff;
pub mod generate;
pub mod history;
pub mod import;
pub mod init;
pub mod inspect;
pub mod stats;
pub mod transform;
pub mod validate;

// Re-export operation types
pub use convert::ConvertOperation;
pub use diff::{DiffConfig, DiffService, SnapshotDiffResult, SnapshotRef};
pub use generate::{
    GenerateConfig, GenerateOperation, GenerateResult, SchemaTemplate, parse_schema_string,
};
pub use history::{HistoryConfig, HistoryEntry, HistoryService};
pub use import::{ImportConfig, ImportResult, ImportService};
pub use init::{ColumnDefinition, InitConfig, InitResult, InitService, SchemaDefinition};
pub use inspect::{
    IcebergInspectOptions, IcebergInspectResult, IcebergTableInspector, InspectOperation,
};
pub use stats::{PartitionStats, StatsConfig, StatsResult, StatsService, TableStats};
pub use transform::{TransformConfig, apply_transforms};
pub use validate::ValidateOperation;

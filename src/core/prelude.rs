//! Prelude for CLI commands
//!
//! This module re-exports commonly used types for CLI command implementations.
//! Using a single wildcard import simplifies command files.
//!
//! # Usage
//!
//! ```ignore
//! use crate::core::prelude::*;
//!
//! // Now you have access to:
//! // - CatalogConfig, CatalogContext, TableResolution
//! // - IcebergMetadataService, TableServiceReader, TableServiceWriter
//! // - format_bytes, format_count, format_number, extract_filename
//! // - Various service types
//! ```

// Configuration
pub use super::config::{CatalogAuth, CatalogConfig, CatalogType, CredentialSource};

// Resolution (table/catalog lookup)
pub use super::resolution::{
    CatalogContext, CatalogResolution, TableResolution, no_catalog_error, no_namespace_error,
    no_table_error, resolve_catalog_from_context, resolve_table,
};

// Metadata services
pub use super::metadata::{
    DataFileChanges, DataFileInfo, IcebergMetadataService, MaintenanceResult, OperationType,
    TableServiceReader, TableServiceWriter,
};

// Table loading
pub use super::table_loader::{TableExt, TableLoader};

// Storage
pub use super::storage::{ObjectStoreExt, Storage, create_object_store};

// Formatting utilities
pub use super::inspection::formatters::{
    extract_filename, format_bytes, format_count, format_number, format_percentage,
};

// Maintenance services
pub use super::maintenance::{
    MaintenanceConfig, ManifestConfig, ManifestService, OptimizeService, RefConfig, RefService,
    RepairService, SnapshotConfig, SnapshotService, VacuumConfig, VacuumResult, VacuumService,
};

// Operations services
pub use super::operations::{
    DiffConfig, DiffService, HistoryConfig, HistoryService, InitConfig, InitService, StatsConfig,
    StatsService,
};

// Analysis
pub use super::analysis::{AnalysisConfig, AnalyzeService, TableAnalysis};

// Formats
pub use super::formats::{FormatHandler, FormatHandlerRegistry};

// Progress reporting
pub use super::progress::{NoopReporter, OptionalProgress, ProgressReporter};

// Iceberg re-exports
pub use iceberg::spec::{Schema as IcebergSchema, Snapshot, TableMetadata};
pub use iceberg::table::Table as IcebergTable;

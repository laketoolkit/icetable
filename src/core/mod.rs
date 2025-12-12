//! Core functionality for TableTools
//!
//! This module contains the core abstractions and implementations for working
//! with tabular data across different formats and storage systems.

pub mod analysis;
pub mod arrow_compat;
pub mod catalog;
pub mod commit;
pub mod config;
pub mod context;
pub mod formats;
pub mod inspection;
pub mod maintenance;
pub mod metadata;
pub mod operations;
pub mod resolution;
pub mod storage;
pub mod table_loader;
pub mod validation;

// Re-export commonly used types
pub use crate::utils::core::{
    TableFormat, detect_table_format, detect_table_format_async, format_bytes, generate_unique_id,
};
pub use catalog::{CatalogClient, RestCatalogClient, TableCommitter, TableRef};
pub use commit::{CommitResult, DirectCommitter, SnapshotCommitter};
pub use config::{
    CatalogAuth, CatalogConfig, CatalogType, Config, CredentialSource, ResolvePath,
    ResolveTableRef, ResolvedTable,
};
pub use context::{TableContext, TableContextBuilder};
pub use formats::{FormatHandler, FormatHandlerFactory};
pub use inspection::{PhysicalInspectionService, PhysicalInspector, PhysicalMetadata};
pub use storage::{ObjectStoreExt, Storage, create_object_store};
pub use table_loader::{TableExt, TableLoader};

// Re-export resolution types for table/catalog resolution
pub use resolution::{
    CatalogContext, CatalogResolution, TableResolution, no_catalog_error, no_namespace_error,
    no_table_error, resolve_catalog_from_context, resolve_table, resolve_table_path,
};

// Re-export formatting utilities for consistent access across CLI
pub use inspection::formatters::{
    extract_filename, format_count, format_number, format_percentage, format_size,
};

// Re-export iceberg types used by CLI to avoid direct iceberg:: dependency
// This provides a stable interface if iceberg crate changes
pub use iceberg::spec::{Schema as IcebergSchema, Snapshot, TableMetadata};
pub use iceberg::table::Table as IcebergTable;

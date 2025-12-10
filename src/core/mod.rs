//! Core functionality for TableTools
//!
//! This module contains the core abstractions and implementations for working
//! with tabular data across different formats and storage systems.

pub mod analysis;
pub mod arrow_compat;
pub mod catalog;
pub mod commit;
pub mod context;
pub mod formats;
pub mod inspection;
pub mod maintenance;
pub mod metadata;
pub mod operations;
pub mod storage;
pub mod table_loader;
pub mod validation;

// Re-export commonly used types
pub use catalog::{
    CatalogClient, CatalogConfig, CatalogType, RestCatalogClient, TableCommitter, TableRef,
};
pub use context::{TableContext, TableContextBuilder};
pub use formats::{FormatHandler, FormatHandlerFactory};
pub use inspection::{PhysicalInspectionService, PhysicalInspector, PhysicalMetadata};
pub use storage::{ObjectStoreExt, Storage, create_object_store};
pub use table_loader::{TableExt, TableLoader};
pub use crate::utils::core::{
    TableFormat, detect_table_format, detect_table_format_async, format_bytes, generate_unique_id,
};
pub use commit::{CommitResult, SnapshotCommitter, DirectCommitter};

// Re-export iceberg types used by CLI to avoid direct iceberg:: dependency
// This provides a stable interface if iceberg crate changes
pub use iceberg::table::Table as IcebergTable;
pub use iceberg::spec::{TableMetadata, Snapshot, Schema as IcebergSchema};

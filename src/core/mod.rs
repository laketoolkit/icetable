//! Core functionality for TableTools
//!
//! This module contains the core abstractions and implementations for working
//! with tabular data across different formats and storage systems.

pub mod analysis;
pub mod arrow_compat;
pub mod catalog;
pub mod context;
pub mod formats;
pub mod inspection;
pub mod maintenance;
pub mod metadata;
pub mod operations;
pub mod storage;
pub mod utils;
pub mod validation;

// Re-export commonly used types
pub use catalog::{CatalogClient, CatalogConfig, CatalogType, RestCatalogClient, TableCommitter, TableRef};
pub use context::{TableContext, TableContextBuilder};
pub use formats::{FormatHandler, FormatHandlerFactory};
pub use inspection::{PhysicalInspectionService, PhysicalInspector, PhysicalMetadata};
pub use storage::{StorageBackend, StorageBackendFactory};
pub use utils::{
    TableFormat, detect_table_format, detect_table_format_async, format_bytes, generate_unique_id,
};

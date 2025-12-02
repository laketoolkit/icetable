//! Core functionality for TableTools
//!
//! This module contains the core abstractions and implementations for working
//! with tabular data across different formats and storage systems.

pub mod arrow_compat;
pub mod formats;
pub mod inspection;
pub mod maintenance;
pub mod metadata;
pub mod operations;
pub mod storage;
pub mod utils;
pub mod validation;

// Re-export commonly used types
pub use formats::{FormatHandler, FormatHandlerFactory};
pub use inspection::{PhysicalInspectionService, PhysicalInspector, PhysicalMetadata};
pub use storage::{StorageBackend, StorageBackendFactory};
pub use utils::{detect_table_format, format_bytes, generate_unique_id, TableFormat};

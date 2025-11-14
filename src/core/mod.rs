//! Core functionality for TableTools
//!
//! This module contains the core abstractions and implementations for working
//! with tabular data across different formats and storage systems.

pub mod formats;
pub mod storage;
pub mod operations;

// Re-export commonly used types
pub use formats::{FormatHandler, FormatHandlerFactory};
pub use storage::{StorageBackend, StorageBackendFactory};

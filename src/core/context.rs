//! Table context for unified table access
//!
//! Provides a single entry point for loading tables, handling:
//! - Path resolution (via config)
//! - Storage backend creation
//! - Format detection
//! - Metadata loading

use std::sync::Arc;

use crate::config::ResolvePath;
use crate::core::TableFormat;
use crate::core::storage::{Storage, create_object_store};
use crate::utils::core::detect_format;
use crate::error::{Error, Result};

use crate::core::metadata::IcebergMetadataService;

/// Table context - unified access to a table
///
/// Usage:
/// ```ignore
/// let ctx = TableContext::from_path(args.path).await?;
/// let metadata = ctx.iceberg_metadata()?;
/// ```
pub struct TableContext {
    /// Resolved table path
    pub path: String,
    /// Storage backend for the table (object_store)
    pub storage: Storage,
    /// Detected table format
    pub format: TableFormat,
}

impl TableContext {
    /// Create context from an optional path (resolves via config if None)
    pub async fn from_path(path: Option<String>) -> Result<Self> {
        let resolved_path = path.resolve()?;
        Self::new(&resolved_path).await
    }

    /// Create context from an explicit path string
    pub async fn new(path: &str) -> Result<Self> {
        let storage = create_object_store(path).await?;
        let format = detect_format(path, &storage).await;

        Ok(Self {
            path: path.to_string(),
            storage,
            format,
        })
    }

    /// Check if the table is Iceberg format
    pub fn is_iceberg(&self) -> bool {
        matches!(self.format, TableFormat::Iceberg)
    }

    /// Require Iceberg format, return error if not
    pub fn require_iceberg(&self) -> Result<()> {
        if !self.is_iceberg() {
            return Err(Error::General(format!(
                "Path '{}' is not an Iceberg table",
                self.path
            )));
        }
        Ok(())
    }

    /// Get Iceberg metadata service for this table
    pub async fn iceberg_service(&self) -> Result<IcebergMetadataService> {
        self.require_iceberg()?;
        IcebergMetadataService::new_async(self.path.clone()).await
    }

    /// Load Iceberg metadata (convenience method)
    pub async fn iceberg_metadata(&self) -> Result<(Arc<iceberg::spec::TableMetadata>, i32)> {
        let service = self.iceberg_service().await?;
        service.load_metadata().await
    }
}

/// Builder for TableContext with optional format override
pub struct TableContextBuilder {
    path: Option<String>,
    format_override: Option<TableFormat>,
}

impl TableContextBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            path: None,
            format_override: None,
        }
    }

    /// Set the path (optional, will use config default if not set)
    pub fn path(mut self, path: Option<String>) -> Self {
        self.path = path;
        self
    }

    /// Override the detected format
    pub fn format(mut self, format: &str) -> Self {
        self.format_override = Some(match format.to_lowercase().as_str() {
            "iceberg" => TableFormat::Iceberg,
            _ => TableFormat::Unknown,
        });
        self
    }

    /// Build the context
    pub async fn build(self) -> Result<TableContext> {
        let mut ctx = TableContext::from_path(self.path).await?;

        if let Some(format) = self.format_override {
            ctx.format = format;
        }

        Ok(ctx)
    }
}

impl Default for TableContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

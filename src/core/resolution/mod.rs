//! Table and catalog resolution logic
//!
//! This module provides the core logic for resolving table references
//! from various sources (paths, catalogs, config context).
//!
//! # Resolution Flow
//!
//! ```text
//!                          CLI Options
//!                              │
//!                              ▼
//!     ┌────────────────────────────────────────────────┐
//!     │               CatalogContext                    │
//!     │  (table, namespace, catalog, warehouse options) │
//!     └────────────────────────────────────────────────┘
//!                              │
//!           ┌──────────────────┼──────────────────┐
//!           │                  │                  │
//!           ▼                  ▼                  ▼
//!   resolve_table_     resolve_catalog    (direct path)
//!   from_context()                          s3://...
//!           │                  │                  │
//!           ▼                  ▼                  ▼
//!     ┌──────────┐      ┌──────────────┐    ┌──────────┐
//!     │ Table    │      │   Catalog    │    │   Path   │
//!     │Resolution│      │  Resolution  │    │          │
//!     └──────────┘      └──────────────┘    └──────────┘
//!           │                  │
//!           │    ┌─────────────┘
//!           │    │
//!           ▼    ▼
//!     ┌─────────────────────────────────────┐
//!     │         Factory Methods             │
//!     ├─────────────────────────────────────┤
//!     │ to_table()         → IcebergTable   │
//!     │ to_readonly_service() → Reader      │
//!     │ to_writable_service() → Writer      │
//!     └─────────────────────────────────────┘
//! ```
//!
//! # Key Types
//!
//! - [`CatalogContext`] - Input from CLI options, holds unresolved references
//! - [`TableResolution`] - Result of resolving a table (Path or CatalogTable)
//! - [`CatalogResolution`] - Resolved catalog client with namespace context
//!
//! # Usage Example
//!
//! ```ignore
//! // From CLI command handler:
//! let resolution = resolve_table_from_context(&ctx).await?;
//!
//! // For read-only operations:
//! let service = resolution.to_readonly_service().await?;
//! let snapshots = service.list_snapshots(None).await?;
//!
//! // For write operations (requires catalog):
//! let service = resolution.to_writable_service(catalog_config, branch).await?;
//! service.write_snapshot(changes, operation, summary).await?;
//! ```

mod catalog;
mod context;
mod errors;
mod management;
mod table;

// Re-export all public items
pub use catalog::{resolve_catalog_from_context, CatalogResolution};
pub use context::CatalogContext;
pub use errors::{no_catalog_error, no_namespace_error, no_table_error};
pub use management::{
    get_catalog_provider_from_context, resolve_management_from_context, ManagementResolution,
};
pub use table::{resolve_table, resolve_table_path, resolve_table_with_catalog, TableResolution};

#[cfg(test)]
mod tests;

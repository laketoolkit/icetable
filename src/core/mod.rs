//! Core functionality for icetable
//!
//! This module contains the core business logic for working with Apache Iceberg
//! tables. It is designed to be independent of the CLI layer and can be used
//! as a library.
//!
//! # Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                              CLI Layer                                  │
//! │  cli/commands/     - Command handlers (thin orchestration layer)        │
//! │  cli/parser/       - Argument parsing (clap)                            │
//! │  cli/output/       - Formatting and display (CliOutput trait)           │
//! └─────────────────────────────────────────────────────────────────────────┘
//!                                    │
//!                                    ▼
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                              Core Layer                                 │
//! │                                                                         │
//! │  resolution.rs     - Table/catalog resolution (CatalogContext,          │
//! │                      TableResolution, CatalogResolution)                │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                      Operations                                  │   │
//! │  │  operations/     - High-level table operations                   │   │
//! │  │    inspect.rs    - Table inspection                              │   │
//! │  │    generate.rs   - Synthetic data generation                     │   │
//! │  │    diff.rs       - Snapshot comparison                           │   │
//! │  │    import.rs     - Data import (Delta, Parquet)                  │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                      Maintenance                                 │   │
//! │  │  maintenance/    - Table maintenance operations                  │   │
//! │  │    optimize.rs   - Data file compaction                          │   │
//! │  │    manifest.rs   - Manifest rewriting                            │   │
//! │  │    vacuum.rs     - Orphan file cleanup                           │   │
//! │  │    snapshot.rs   - Snapshot expiration                           │   │
//! │  │    refs.rs       - Branch/tag management                         │   │
//! │  │    repair.rs     - Metadata repair                               │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                      Metadata Services                           │   │
//! │  │  metadata/       - Metadata reading and writing                  │   │
//! │  │    traits.rs     - TableServiceReader, TableServiceWriter traits │   │
//! │  │    iceberg.rs    - IcebergMetadataService implementation         │   │
//! │  │    writer.rs     - SnapshotWriter for creating snapshots         │   │
//! │  │    reader.rs     - MetadataReader for loading metadata           │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                      Catalog                                     │   │
//! │  │  catalog/        - Catalog abstractions                          │   │
//! │  │    rest.rs       - REST catalog client (Polaris, Nessie)         │   │
//! │  │    committer.rs  - TableCommitter for atomic commits             │   │
//! │  │    management/   - Warehouse and namespace management            │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                      Storage                                     │   │
//! │  │  storage/        - Storage backend abstractions                  │   │
//! │  │    mod.rs        - ObjectStore creation and utilities            │   │
//! │  │    file_io.rs    - FileIO creation for iceberg operations        │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! │  config/           - Configuration management (catalogs, auth)          │
//! │  analysis/         - Table analysis and statistics                      │
//! │  validation/       - Schema and data validation                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Abstractions
//!
//! ## Resolution (`resolution.rs`)
//!
//! - [`CatalogContext`] - CLI options for table/catalog resolution
//! - [`TableResolution`] - Resolved table (path or catalog reference)
//! - [`CatalogResolution`] - Resolved catalog with client
//!
//! ## Metadata Traits (`metadata/traits.rs`)
//!
//! - [`TableServiceReader`] - Read-only operations (list files, snapshots)
//! - [`TableServiceWriter`] - Write operations (create snapshots)
//!
//! ## Catalog (`catalog/`)
//!
//! - [`CatalogClient`] - Abstract catalog client
//! - [`RestCatalogClient`] - REST catalog implementation
//! - [`TableCommitter`] - Atomic commit to catalog
//!
//! # Usage from CLI
//!
//! ```ignore
//! // 1. Resolve table from CLI context
//! let resolution = resolve_table_from_context(&ctx).await?;
//!
//! // 2. Get appropriate service
//! let service = resolution.to_readonly_service().await?;  // for reads
//! let service = resolution.to_writable_service(catalog, branch).await?;  // for writes
//!
//! // 3. Use service methods
//! let files = service.list_data_files().await?;
//! ```
//!
//! # Usage as Library
//!
//! ```ignore
//! use icetable::core::{IcebergMetadataService, TableServiceReader};
//!
//! let service = IcebergMetadataService::new_async("s3://bucket/table").await?;
//! let files = service.list_data_files().await?;
//! ```

pub mod analysis;
pub mod arrow_compat;
pub mod catalog;
pub mod commit;
pub mod config;
pub mod formats;
pub mod inspection;
pub mod maintenance;
pub mod metadata;
pub mod operations;
pub mod prelude;
pub mod progress;
pub mod resolution;
pub mod storage;
pub mod table_loader;
pub mod utils;
pub mod validation;

// Re-export commonly used types
pub use utils::{
    TableFormat, detect_table_format, detect_table_format_async, format_bytes, generate_unique_id,
};
pub use catalog::{CatalogClient, RestCatalogClient, TableCommitter, TableRef};
pub use commit::{CommitResult, DirectCommitter, SnapshotCommitter};
pub use config::{
    CatalogAuth, CatalogConfig, CatalogType, Config, CredentialSource, ResolvePath,
    ResolveTableRef, ResolvedTable,
};
pub use formats::{FormatHandler, FormatHandlerFactory};
pub use inspection::{PhysicalInspectionService, PhysicalInspector, PhysicalMetadata};
pub use storage::{ObjectStoreExt, Storage, create_object_store};
pub use table_loader::{TableExt, TableLoader};

// Re-export metadata service types and traits
pub use metadata::{
    IcebergMetadataService, TableServiceReader, TableServiceWriter,
};

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

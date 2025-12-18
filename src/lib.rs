//! icetable - CLI for Apache Iceberg table management
//!
//! This library provides a unified interface for inspecting, validating,
//! converting, and managing Apache Iceberg tables across storage systems
//! (local, S3, GCS, Azure).
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use icetable::{create_object_store, ObjectStoreExt, Result};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create object store from URL or path
//!     let store = create_object_store("/path/to/table").await?;
//!
//!     // Use extension methods for convenient access
//!     let exists = store.exists_str("metadata/v1.metadata.json").await?;
//!     println!("Table exists: {}", exists);
//!
//!     Ok(())
//! }
//! ```
//!
//! # Extending with Custom Formats
//!
//! ```rust,ignore
//! use icetable::{FormatHandler, FormatHandlerRegistry};
//!
//! // Register a custom format handler
//! FormatHandlerRegistry::global().register("xml", 75, |path, storage| {
//!     Ok(Box::new(XmlHandler::new(path, storage)?))
//! });
//! ```
//!
//! # Custom Transformations
//!
//! ```rust,ignore
//! use icetable::transform::{TransformPipeline, FilterStep, CustomTransformStep};
//!
//! let pipeline = TransformPipeline::new()
//!     .add_step(FilterStep::new("age > 18"))
//!     .add_step(CustomTransformStep::new("deduplicate", |batch| {
//!         // Your custom logic here
//!         Ok(batch)
//!     }));
//!
//! let transformed = pipeline.apply(batch)?;
//! ```
//!
//! # Extensibility Guide
//!
//! ## Adding a New CLI Command
//!
//! 1. **Create parser** in `src/cli/parser/foo.rs`:
//!    ```rust,ignore
//!    #[derive(Args)]
//!    pub struct FooArgs {
//!        /// Table path or name
//!        pub table: String,
//!        /// Enable verbose output
//!        #[arg(long)]
//!        pub verbose: bool,
//!    }
//!    ```
//!
//! 2. **Create command** in `src/cli/commands/foo.rs`:
//!    ```rust,ignore
//!    pub struct FooCommand;
//!
//!    impl FooCommand {
//!        pub async fn execute(args: FooArgs, ctx: &CatalogContext) -> Result<()> {
//!            // Use resolution module for path/catalog handling
//!            let resolution = resolve_table_from_context(ctx).await?;
//!            let service = resolution.to_readonly_service().await?;
//!
//!            // Use service traits for operations
//!            let files = service.list_data_files().await?;
//!            // ...
//!            Ok(())
//!        }
//!    }
//!    ```
//!
//! 3. **Register** in `src/main.rs`:
//!    ```rust,ignore
//!    Commands::Foo(args) => FooCommand::execute(args, &ctx).await,
//!    ```
//!
//! ## Adding a New Storage Backend
//!
//! The `create_object_store` function auto-detects storage from URL prefix.
//! To add a new backend:
//!
//! 1. **Add detection** in `src/core/storage.rs`:
//!    ```rust,ignore
//!    fn create_object_store_inner(path: &str) -> Result<Storage> {
//!        if path.starts_with("mycloud://") {
//!            return create_mycloud_store(path);
//!        }
//!        // ... existing backends
//!    }
//!    ```
//!
//! 2. **Implement builder** with environment-based config:
//!    ```rust,ignore
//!    fn create_mycloud_store(path: &str) -> Result<Storage> {
//!        let bucket = extract_bucket(path)?;
//!        let store = MyCloudBuilder::from_env()
//!            .with_bucket(bucket)
//!            .build()?;
//!        Ok(Arc::new(store))
//!    }
//!    ```
//!
//! ## Adding Validation Rules
//!
//! Custom validation rules can be added via YAML:
//!
//! ```yaml
//! # rules/custom.yaml
//! rules:
//!   - name: "timestamp_column_exists"
//!     enabled: true
//!     rule_type:
//!       column_exists:
//!         column: "event_time"
//!
//!   - name: "correct_timestamp_type"
//!     enabled: true
//!     rule_type:
//!       column_type:
//!         column: "event_time"
//!         expected_type: "Timestamp"
//! ```
//!
//! Load custom rules:
//! ```rust,ignore
//! let engine = ValidationEngine::new()
//!     .load_rules_from_file("rules/custom.yaml")?;
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

// Internal implementation modules
// CLI module is hidden from docs - it's only for the icetable binary, not for library users
#[doc(hidden)]
pub mod cli;

pub mod core;
pub mod error;

// Utils module is hidden from docs - internal CLI utilities
#[doc(hidden)]
pub mod utils;

// =============================================================================
// Public API - Direct exports from root
// =============================================================================

// Error handling
pub use error::{Error, Result, ResultExt};

// Configuration
pub use core::config;

// Storage
pub use core::storage::{
    ObjectMeta, ObjectStoreExt, Storage, create_object_store, detect_storage_type,
};

// Format handling
pub use core::formats::{
    ColumnStats, FileMetadata, FormatHandler, FormatHandlerFactory, FormatHandlerRegistry,
    ReadOptions, ReadOptionsBuilder, ValidationReport, WriteOptions, WriteOptionsBuilder,
};

// Metadata service traits and types
// These allow implementing custom metadata backends or extending functionality
pub use core::metadata::{
    DataFileChanges, DataFileInfo, IcebergMetadataService, MaintenanceResult,
    MetadataServiceReader, MetadataServiceWriter, OperationType, SnapshotInfo, TableServiceReader,
    TableServiceWriter,
};

// Table loading
pub use core::table_loader::TableLoader;

// Validation
pub use core::validation::{RuleResult, Severity, ValidationEngine, ValidationRule};

// Maintenance services
// These provide high-level operations for table optimization, vacuum, snapshots, etc.
pub mod maintenance {
    //! Maintenance services for table operations
    //!
    //! Provides high-level services for optimizing, vacuuming, and managing
    //! Apache Iceberg tables.
    //!
    //! # Available Services
    //!
    //! - [`VacuumService`] - Remove orphan files and expired snapshots
    //! - [`OptimizeService`] - Compact small files into larger ones
    //! - [`SnapshotService`] - Manage table snapshots (list, expire, restore)
    //! - [`RefService`] - Manage branches and tags
    //! - [`RepairService`] - Repair table metadata
    //! - [`ManifestService`] - Rewrite manifest files
    //! - [`DoctorService`] - Health checks for tables

    pub use crate::core::maintenance::{
        // Refs (branches/tags)
        BranchRetention,
        // Doctor
        CheckResult,
        CheckStatus,
        CheckSummary,
        // Snapshots
        CreateBackupResult,
        DoctorConfig,
        DoctorService,
        ExpireSnapshotsResult,
        FileGroup,
        LineageEntry,
        LineageResult,
        ListSnapshotsResult,
        // Shared
        MaintenanceConfig,
        // Manifest
        ManifestAnalysis,
        ManifestConfig,
        ManifestRewriteResult,
        ManifestService,
        // Optimize
        OptimizeService,
        // Vacuum
        OrphanFile,
        RefConfig,
        RefResult,
        RefService,
        // Repair
        RepairAnalysis,
        RepairService,
        SetSnapshotResult,
        SnapshotConfig,
        SnapshotDetails,
        SnapshotService,
        VacuumAnalysis,
        VacuumConfig,
        VacuumResult,
        VacuumService,
    };
}

// Transformations
pub mod transform {
    //! Data transformation pipeline
    //!
    //! Provides composable transformations for Arrow RecordBatches.
    //!
    //! # Built-in Steps
    //!
    //! - [`FilterStep`] - Filter rows based on expressions
    //! - [`ProjectStep`] - Select specific columns
    //! - [`RenameStep`] - Rename columns
    //! - [`CastStep`] - Cast column types
    //! - [`CustomTransformStep`] - Custom transformation using closures

    pub use crate::core::operations::transform::{
        CastStep, CustomTransformStep, FilterStep, ProjectStep, RenameStep, TransformConfig,
        TransformPipeline, TransformStep,
    };
}

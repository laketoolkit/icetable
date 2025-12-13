# Architecture

This document describes the high-level architecture of icetable, a CLI tool for managing Apache Iceberg tables (with Delta Lake import support).

## Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                           CLI Layer                             │
│  src/cli/                                                       │
│  ├── commands/     Command implementations (inspect, vacuum...) │
│  └── output/       Formatters and display logic                 │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                          Core Layer                             │
│  src/core/                                                      │
│  ├── inspection/   Physical table inspection                    │
│  ├── metadata/     Metadata services (Iceberg, Delta)           │
│  ├── maintenance/  Vacuum, optimize, snapshot management        │
│  ├── operations/   Data operations (generate, transform)        │
│  ├── storage/      Storage backends (local, S3, GCS)            │
│  └── catalog/      Catalog integration (REST, Hive, Glue)       │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Storage Layer                            │
│  Local filesystem, Amazon S3, Google Cloud Storage, Azure Blob  │
└─────────────────────────────────────────────────────────────────┘
```

## Design Principles

### 1. Service vs Operation Naming Convention

The codebase uses two naming patterns for core logic:

**`*Service`** - Stateful, configurable components:
- Have configuration structs (e.g., `DoctorConfig`, `VacuumConfig`)
- Created with `::new()` or `::with_config()`
- Maintain internal state between method calls
- Examples: `DoctorService`, `VacuumService`, `OptimizeService`, `SnapshotService`

**`*Operation`** - Stateless, one-shot functions:
- Pure functions or simple structs without configuration
- Execute immediately with all parameters passed to the method
- No internal state to manage
- Examples: `GenerateOperation`, `ValidateOperation`, `ConvertOperation`

```rust
// Service pattern: configured, then executed
let service = VacuumService::with_config(config);
let result = service.analyze(&metadata_service).await?;

// Operation pattern: executed directly
let result = GenerateOperation::execute(schema, output_path, options).await?;
```

This distinction helps developers understand whether a component needs configuration
and state management (Service) or is a simple stateless transformation (Operation).

### 2. Thin CLI, Fat Core

The CLI layer (`src/cli/`) is intentionally thin. It handles:
- Argument parsing (clap)
- Progress indicators (indicatif)
- Output formatting (colored, comfy-table)
- User interaction

All business logic lives in `src/core/`. This enables:
- Easy testing of core logic without CLI
- Potential future use as a library
- Clear separation of concerns

### 3. Trait-Based Abstractions

Key abstractions use traits for extensibility:

```rust
// Storage abstraction
trait StorageBackend {
    async fn get(&self, path: &str, options: &GetOptions) -> Result<Bytes>;
    async fn put(&self, path: &str, data: Bytes) -> Result<()>;
    async fn list(&self, options: &ListOptions) -> Result<ListResult>;
    async fn delete(&self, path: &str) -> Result<()>;
}

// Inspector abstraction
trait PhysicalInspector {
    async fn extract_metadata(&self, options: &PhysicalInspectOptions) -> Result<PhysicalMetadata>;
    fn format_name(&self) -> &str;
    fn can_inspect(&self, path: &Path) -> bool;
}

// Metadata service abstraction
trait MetadataService {
    async fn load_metadata(&self) -> Result<(Arc<TableMetadata>, String)>;
    async fn list_snapshots(&self) -> Result<Vec<SnapshotInfo>>;
    async fn list_data_files(&self) -> Result<Vec<DataFileInfo>>;
}
```

### 4. Format-Agnostic Design

The inspection system uses a format-agnostic intermediate representation:

```
┌──────────────┐     ┌──────────────────┐     ┌──────────────┐
│ IcebergInsp. │────▶│ PhysicalMetadata │────▶│ InspectView  │
└──────────────┘     └──────────────────┘     └──────────────┘
                              ▲
┌──────────────┐              │
│  DeltaInsp.  │──────────────┘
└──────────────┘
```

This allows:
- Consistent output across formats
- Format-specific details preserved in `details` maps
- Easy addition of new formats

## Module Structure

### `src/cli/`

```
cli/
├── commands/
│   ├── analyze.rs      # Table health analysis
│   ├── common.rs       # Shared CLI utilities
│   ├── config.rs       # Configuration management
│   ├── create.rs       # Create tables/namespaces
│   ├── delete.rs       # Delete tables/namespaces
│   ├── diff.rs         # Compare snapshots
│   ├── doctor.rs       # Environment diagnostics
│   ├── generate.rs     # Test data generation
│   ├── history.rs      # Snapshot history
│   ├── import.rs       # Delta Lake import
│   ├── init.rs         # Initialize new table
│   ├── inspect.rs      # Table inspection
│   ├── ls.rs           # List catalog contents
│   ├── optimize.rs     # Compact small files
│   ├── refs.rs         # Branch/tag management
│   ├── repair.rs       # Fix table metadata
│   ├── snapshot.rs     # Snapshot management
│   ├── stats.rs        # Partition statistics
│   ├── tui.rs          # Terminal UI (experimental)
│   ├── vacuum.rs       # Remove orphan files
│   └── validate.rs     # Schema/data validation
└── output/
    ├── box_section.rs  # Box-frame rendering
    ├── inspect_formatter.rs  # Inspection output
    └── mod.rs
```

### `src/core/`

```
core/
├── analysis/           # Table health analysis
│   └── service.rs      # AnalysisService
├── arrow_compat.rs     # Arrow version compatibility
├── catalog/            # Catalog integration (REST, etc.)
│   ├── client.rs       # CatalogClient
│   ├── rest.rs         # REST catalog implementation
│   └── traits.rs       # Catalog traits
├── commit/             # Table commit operations
│   └── mod.rs          # TableCommitter
├── config/             # Configuration and resolution
│   ├── mod.rs          # CatalogConfig, CatalogAuth
│   └── resolver.rs     # Config-based table resolution
├── context.rs          # TableContext for execution
├── formats/            # Format detection and conversion
│   └── mod.rs          # Format utilities
├── inspection/
│   ├── iceberg/        # Iceberg inspector (modularized)
│   │   ├── mod.rs      # IcebergInspector
│   │   ├── manifest.rs # Manifest reading
│   │   ├── orphan.rs   # Orphan detection
│   │   ├── layout.rs   # Layout extraction
│   │   └── factory.rs  # Inspector factory
│   ├── delta.rs        # Delta Lake inspector
│   ├── traits.rs       # PhysicalInspector trait
│   ├── registry.rs     # Inspector discovery
│   └── view_builder.rs # View construction
├── maintenance/
│   ├── doctor/         # DoctorService (env diagnostics)
│   ├── vacuum.rs       # VacuumService
│   ├── optimize.rs     # OptimizeService
│   ├── manifest.rs     # Manifest operations
│   └── snapshot.rs     # Snapshot management
├── metadata/
│   ├── iceberg.rs      # IcebergMetadataService
│   └── traits.rs       # MetadataService trait
├── operations/
│   ├── generate.rs     # Data generation
│   └── transform/      # Data transformations
│       ├── pipeline.rs # Transform pipeline
│       ├── filter.rs   # Row filtering
│       └── project.rs  # Column projection
├── resolution.rs       # TableResolution, CatalogResolution
├── storage/
│   ├── traits.rs       # StorageBackend trait
│   ├── local.rs        # Local filesystem
│   ├── s3.rs           # Amazon S3
│   ├── gcs.rs          # Google Cloud Storage
│   └── factory.rs      # Backend factory
├── table_loader.rs     # TableLoader for Iceberg tables
└── validation/         # Data validation
    └── mod.rs          # Validation utilities
```

### `src/utils/`

Shared utilities used across the codebase:

```
utils/
├── box_frame.rs        # Box-frame rendering utilities
├── cancellation.rs     # Graceful cancellation (Ctrl+C)
├── logging.rs          # Log configuration
├── progress.rs         # Progress indicators
├── resources.rs        # Memory/concurrency limits
├── telemetry.rs        # Usage telemetry (opt-in)
├── text.rs             # Text formatting utilities
├── time.rs             # Timestamp parsing
├── types.rs            # Shared type definitions
└── core/
    ├── format_detection.rs  # Auto-detect table format
    ├── fs.rs                # Filesystem utilities
    ├── iceberg.rs           # Iceberg-specific utilities
    ├── parquet.rs           # Parquet utilities
    └── snapshot.rs          # Snapshot utilities
```

## Table Resolution

The resolution system (`core/resolution.rs`) provides a unified way to resolve table references
from various sources. This is central to the "Thin CLI, Fat Core" principle.

### Resolution Types

```rust
// Path-based or catalog-based table resolution
pub enum TableResolution {
    Path(String),                    // Direct storage path
    CatalogTable {                   // Loaded from catalog
        table: Box<IcebergTable>,
        namespace: Vec<String>,
        name: String,
        catalog_config: Box<CatalogConfig>,
    },
}

// Catalog-level resolution for ls, create, delete
pub struct CatalogResolution {
    catalog_name: String,
    catalog_config: CatalogConfig,
    namespace: Option<String>,
    table: Option<String>,
    client: RestCatalogClient,
}
```

### Resolution Priority

1. CLI `--catalog-uri` argument (explicit catalog)
2. Config context (`icetable config use`)
3. Direct path (if provided)

### Factory Methods

`TableResolution` provides factory methods to create services:

```rust
// Read-only operations (analyze, inspect, list)
let service = resolution.to_readonly_service().await?;

// Write operations (optimize, repair, expire)
let service = resolution.to_writable_service(catalog, branch).await?;

// Direct table access
let table = resolution.to_table().await?;
```

This ensures:
- Catalog tables use proper commit semantics
- Direct paths write to storage without catalog tracking
- Consistent behavior across all commands

## Key Data Flows

### Inspection Flow

```
1. CLI parses args → InspectCommand
2. Resolve table (path or catalog)
3. StorageBackendFactory creates backend
4. PhysicalInspectorRegistry selects inspector
5. Inspector extracts PhysicalMetadata
6. InspectionViewBuilder creates view
7. InspectionFormatter renders output
```

### Vacuum Flow

```
1. CLI parses args → VacuumCommand
2. VacuumService.execute(table_path)
3. Load metadata via IcebergMetadataService
4. Scan storage for all data files
5. Build reference set from all snapshots
6. Identify unreferenced files
7. Delete orphan files (if --execute)
```

### Optimize Flow

```
1. CLI parses args → OptimizeCommand
2. OptimizeService.execute(table_path)
3. Load partition information
4. Identify partitions with small files
5. For each partition:
   a. Read all small files
   b. Merge into target-size files
   c. Write new files
   d. Create new snapshot
6. Optionally expire old snapshots
```

## Error Handling

All fallible operations return `Result<T, Error>` where `Error` is defined in `src/error.rs`.
The error system uses **semantic error types** - each variant describes a specific failure mode
with appropriate context:

```rust
pub enum Error {
    // I/O and storage
    Io(std::io::Error),
    FileNotFound { path: PathBuf },
    Storage { message: String },
    ObjectStore(object_store::Error),

    // Format parsing
    Parse { message: String, source: Option<...> },
    InvalidFormat { message: String },
    CorruptedFile { path: PathBuf, reason: String },

    // Table operations
    TableNotFound { path: String },
    TableAlreadyExists { path: String },
    SnapshotNotFound { snapshot_id: i64 },
    BranchNotFound { name: String },
    TagNotFound { name: String },

    // Catalog operations
    NoCatalog,
    CatalogNotFound { name: String },
    NamespaceNotFound { name: String },

    // Data validation
    DataValidation { message: String },
    SchemaValidation { message: String },
    InvalidFilterExpression { expression: String, reason: String },

    // External libraries
    Arrow(arrow::error::ArrowError),
    Parquet(parquet::errors::ParquetError),

    // ... and more (33+ semantic variants)
}
```

Key design principles:
- **No generic catch-all errors** - each error type is semantically meaningful
- **Contextual information** - errors include relevant data (paths, IDs, names)
- **User-friendly messages** - `user_message()` method provides actionable suggestions
- **Recoverability hints** - `is_recoverable()` indicates if retry might succeed

Errors propagate up to the CLI layer, which formats them for display.

## Concurrency Model

- Async runtime: Tokio
- Manifest reading: Parallel with `buffer_unordered(32)`
- File operations: Configurable concurrency via `ResourceLimits`
- Cancellation: `tokio::sync::watch` for graceful shutdown

## Configuration

Configuration is loaded from (in order of precedence):
1. CLI arguments
2. Environment variables (`ICETABLE_*`)
3. Config file (`~/.config/icetable/config.yaml`)

## Testing Strategy

- Unit tests: In-module `#[cfg(test)]` blocks
- Integration tests: `tests/` directory (requires test fixtures)
- Test data: Generated via `generate` command

## Adding New Features

### Adding a New Command

1. Create `src/cli/commands/mycommand.rs`
2. Add to `src/cli/commands/mod.rs`
3. Register in `src/main.rs` CLI definition
4. Implement core logic in `src/core/` if needed

### Adding a New Table Format

1. Implement `PhysicalInspector` trait
2. Implement `PhysicalInspectorFactory` trait
3. Register in `PhysicalInspectorRegistry`
4. Add format detection in `utils/core/format_detection.rs`

### Adding a New Storage Backend

1. Implement `StorageBackend` trait
2. Add to `StorageBackendFactory::create_backend()`
3. Update URL parsing in `storage/traits.rs`

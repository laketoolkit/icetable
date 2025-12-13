# Architecture

This document describes the high-level architecture of icetable, a CLI tool for managing Apache Iceberg tables (with Delta Lake import support).

## Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                           CLI Layer                             │
│  src/cli/                                                       │
│  ├── commands/     Command implementations (inspect, vacuum...) │
│  ├── parser/       Argument parsing and validation              │
│  └── output/       Formatters and display logic                 │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                          Core Layer                             │
│  src/core/                                                      │
│  ├── analysis/     Table health analysis                        │
│  ├── metadata/     Metadata services and traits                 │
│  ├── maintenance/  Vacuum, optimize, snapshot management        │
│  ├── operations/   Inspect, generate, transform                 │
│  ├── storage/      Object store abstraction                     │
│  └── catalog/      REST catalog integration                     │
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
// Metadata service abstraction
pub trait MetadataService: Send + Sync {
    fn table(&self) -> &Table;
    fn file_io(&self) -> &FileIO;
    async fn commit_changes(&self, changes: DataFileChanges, op: OperationType) -> Result<i64>;
    async fn list_snapshots(&self) -> Result<Vec<SnapshotInfo>>;
    async fn list_data_files(&self, snapshot_id: Option<i64>) -> Result<Vec<DataFileInfo>>;
}

// Catalog context for REST catalog operations
pub trait CatalogContext: Send + Sync {
    fn catalog(&self) -> &Arc<dyn Catalog>;
    fn namespace(&self) -> Option<&str>;
    fn table(&self) -> Option<&str>;
    async fn load_table(&self, name: &str) -> Result<Table>;
    async fn table_exists(&self, name: &str) -> Result<bool>;
}
```

### 4. Format-Agnostic Design

The optimize command uses a parallel streaming architecture:

```
┌──────────┐      ┌──────────┐      ┌──────────────────┐      ┌──────────┐
│ Reader 1 │────▶│          │      │                  │────▶│ Writer 1 │
├──────────┤      │  Async   │      │  Bounded MPMC    │      ├──────────┤
│ Reader 2 │────▶│  Stream  │────▶│    Channel       │────▶│ Writer 2 │
├──────────┤      │          │      │  (backpressure)  │      ├──────────┤
│ Reader N │────▶│          │      │                  │────▶│ Writer M │
└──────────┘      └──────────┘      └──────────────────┘      └──────────┘
```

- **N readers** (8-32 based on file size) read parquet files concurrently
- **Bounded channel** provides backpressure and respects `--max-memory`
- **M writers** (up to 8, based on cores) write output files in parallel
- Schema coercion happens per-file, not per-batch (optimization)

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

Shared utilities:

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
2. resolve_table_from_context() → TableResolution
3. TableResolution.to_table() → iceberg::Table
4. IcebergTableInspector::inspect() → IcebergInspectResult
5. Format as text or JSON
```

### Optimize Flow

```
1. CLI parses args → OptimizeCommand
2. Load table via catalog or path
3. IcebergMetadataService wraps table
4. OptimizeService.execute():
   a. List data files, group by partition
   b. Split into subgroups (dynamic sizing)
   c. For each subgroup, run compact_group_pipeline():
      - Spawn N reader tasks (buffer_unordered)
      - Spawn M writer tasks (parallel consumers)
      - Stream batches through bounded channel
      - Writers create new compacted files
   d. Commit changes via MetadataService
5. Report results
```

### Vacuum Flow

```
1. CLI parses args → VacuumCommand
2. VacuumService.analyze():
   a. Load all snapshots
   b. Build referenced file set (with manifest caching)
   c. List all files in data directory
   d. Identify orphans older than retention period
3. VacuumService.execute():
   a. Delete orphan files (bulk delete API)
   b. Report freed space
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

- **Async runtime**: Tokio (multi-threaded)
- **File reading**: `buffer_unordered(N)` where N = 8-32 based on file size
- **File writing**: M parallel writers where M = min(cores, expected_files, 8)
- **Channel**: `async_channel` bounded MPMC, size respects `--max-memory`
- **Cancellation**: `tokio::sync::watch` for graceful Ctrl+C handling

### Memory Management

When `--max-memory` is set:
- Channel buffer size is limited to use ≤50% of memory limit
- Backpressure naturally limits memory usage
- Writers release batches immediately after writing

## Configuration

Configuration is loaded from (in order of precedence):
1. CLI arguments (`-t`, `-n`, etc.)
2. Environment variables (`AWS_*`, etc.)
3. Config file (`~/.config/icetable/config.toml`)

Config file stores:
- Catalog definitions (URL, warehouse, auth)
- Current context (catalog, namespace, table)
- Table aliases

## Testing Strategy

- **Unit tests**: In-module `#[cfg(test)]` blocks
- **Integration tests**: `tests/` directory
- **Test data**: Generated via `generate` command
- **CI**: GitHub Actions with MinIO for S3 testing

## Adding New Features

### Adding a New Command

1. Create `src/cli/parser/mycommand.rs` for arguments
2. Create `src/cli/commands/mycommand.rs` for implementation
3. Add to `src/cli/commands/mod.rs`
4. Register in `src/main.rs` CLI definition
5. Implement core logic in `src/core/` if needed

### Adding a New Catalog Type

1. Implement catalog client in `src/core/catalog/`
2. Add to catalog factory
3. Update config parsing

### Adding a New Storage Backend

Storage is handled by `object_store` crate. To add support:
1. Update `create_object_store()` in `src/core/storage/mod.rs`
2. Add URL scheme detection
3. Update credential resolution in `src/utils/credentials.rs`

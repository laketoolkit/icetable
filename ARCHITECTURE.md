# Architecture

This document describes the high-level architecture of icetable, a CLI tool for inspecting and maintaining Apache Iceberg and Delta Lake tables.

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
│   ├── inspect.rs      # Table inspection
│   ├── vacuum.rs       # Remove orphan files
│   ├── optimize.rs     # Compact small files
│   ├── analyze.rs      # Table health analysis
│   ├── history.rs      # Snapshot history
│   ├── stats.rs        # Partition statistics
│   ├── generate.rs     # Test data generation
│   └── validate.rs     # Schema/data validation
└── output/
    ├── box_section.rs  # Box-frame rendering
    ├── inspect_formatter.rs  # Inspection output
    └── mod.rs
```

### `src/core/`

```
core/
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
├── metadata/
│   ├── iceberg.rs      # IcebergMetadataService
│   └── traits.rs       # MetadataService trait
├── maintenance/
│   ├── vacuum.rs       # VacuumService
│   ├── optimize.rs     # OptimizeService
│   ├── manifest.rs     # Manifest operations
│   └── snapshot.rs     # Snapshot management
├── operations/
│   ├── inspect.rs      # High-level inspect
│   ├── generate.rs     # Data generation
│   └── transform/      # Data transformations
│       ├── pipeline.rs # Transform pipeline
│       ├── filter.rs   # Row filtering
│       └── project.rs  # Column projection
├── storage/
│   ├── traits.rs       # StorageBackend trait
│   ├── local.rs        # Local filesystem
│   ├── s3.rs           # Amazon S3
│   ├── gcs.rs          # Google Cloud Storage
│   └── factory.rs      # Backend factory
└── catalog/
    ├── config.rs       # Catalog configuration
    └── mod.rs          # Catalog integration
```

### `src/utils/`

Shared utilities used across the codebase:

```
utils/
├── time.rs             # Timestamp parsing
├── resources.rs        # Memory/concurrency limits
├── credentials.rs      # Credential resolution
├── cancellation.rs     # Graceful cancellation
└── core/
    ├── format_detection.rs  # Auto-detect table format
    └── snapshot.rs          # Snapshot utilities
```

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

All fallible operations return `Result<T, Error>` where `Error` is defined in `src/error.rs`:

```rust
pub enum Error {
    Io(std::io::Error),
    Storage(StorageError),
    Metadata(MetadataError),
    Iceberg(iceberg::Error),
    Delta(deltalake::DeltaTableError),
    Arrow(arrow::error::ArrowError),
    Parquet(parquet::errors::ParquetError),
    Config(String),
    Validation(String),
    General(String),
}
```

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

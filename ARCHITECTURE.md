# Architecture

This document describes the high-level architecture of icetable, a CLI tool for managing Apache Iceberg tables.

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

### 1. Thin CLI, Fat Core

The CLI layer (`src/cli/`) is intentionally thin. It handles:
- Argument parsing (clap)
- Progress indicators (indicatif)
- Output formatting (colored, comfy-table)
- User interaction

All business logic lives in `src/core/`. This enables:
- Easy testing of core logic without CLI
- Potential future use as a library
- Clear separation of concerns

### 2. Trait-Based Abstractions

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

### 3. Streaming Pipeline for Optimize

The optimize command uses a parallel streaming architecture:

```
┌──────────┐     ┌──────────┐     ┌──────────────────┐     ┌──────────┐
│ Reader 1 │────▶│          │     │                  │────▶│ Writer 1 │
├──────────┤     │  Async   │     │  Bounded MPMC    │     ├──────────┤
│ Reader 2 │────▶│  Stream  │────▶│    Channel       │────▶│ Writer 2 │
├──────────┤     │          │     │  (backpressure)  │     ├──────────┤
│ Reader N │────▶│          │     │                  │────▶│ Writer M │
└──────────┘     └──────────┘     └──────────────────┘     └──────────┘
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
│   ├── common.rs       # Shared utilities
│   ├── config.rs       # Configuration management
│   ├── generate.rs     # Test data generation
│   ├── inspect.rs      # Table inspection
│   ├── optimize.rs     # File compaction
│   ├── snapshot.rs     # Snapshot management
│   ├── vacuum.rs       # Orphan file cleanup
│   └── ...
├── parser/
│   ├── mod.rs          # CLI argument definitions
│   └── ...
└── output/
    ├── box_section.rs  # Box-frame rendering
    └── mod.rs
```

### `src/core/`

```
core/
├── analysis/
│   └── service.rs      # AnalysisService for table health
├── catalog/
│   ├── config.rs       # Catalog configuration
│   ├── context.rs      # CatalogContext trait
│   └── rest.rs         # REST catalog client
├── maintenance/
│   ├── mod.rs          # FileGroup, partitioning
│   ├── optimize.rs     # OptimizeService (parallel writers)
│   ├── vacuum.rs       # VacuumService
│   └── snapshot.rs     # Snapshot operations
├── metadata/
│   ├── traits.rs       # MetadataService trait
│   └── iceberg.rs      # IcebergMetadataService
├── operations/
│   ├── inspect.rs      # IcebergTableInspector
│   └── generate.rs     # GenerateOperation
└── storage/
    ├── ext.rs          # ObjectStoreExt trait
    └── mod.rs          # create_object_store factory
```

### `src/utils/`

Shared utilities:

```
utils/
├── core/
│   ├── mod.rs          # format_bytes, generate_unique_id, etc.
│   └── ...
├── resources.rs        # ResourceLimits (memory, timeout, concurrency)
├── cancellation.rs     # Graceful shutdown with Ctrl+C
├── credentials.rs      # AWS/GCS credential resolution
└── time.rs             # Timestamp parsing
```

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

All fallible operations return `Result<T, Error>` where `Error` is defined in `src/error.rs`:

```rust
pub enum Error {
    Io(std::io::Error),
    Iceberg(iceberg::Error),
    Arrow(arrow::error::ArrowError),
    Parquet(parquet::errors::ParquetError),
    ObjectStore(object_store::Error),
    TableNotFound { path: String },
    SnapshotNotFound { id: i64 },
    Configuration { message: String },
    Metadata { message: String },
    // ...
}
```

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

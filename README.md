# TableTools

Universal CLI for inspecting, validating, converting, and managing tabular data files.

## Status

🚧 **Project Status**: Architecture Complete, Implementation In Progress

This project is currently in the architectural phase. The complete module structure, traits, and scaffolding are in place. Implementation of core functionality is delegated to the Rust-Developer agent.

## Features (Planned)

- **Multi-Format Support**: Parquet, Arrow IPC, Delta Lake, Iceberg
- **Cloud-Native**: First-class support for S3, GCS, Azure Blob Storage
- **Fast**: Streaming operations, never loads entire files into memory
- **User-Friendly**: Actionable error messages, progress indicators
- **Zero Config**: Works out of the box for common operations

## Quick Start

```bash
# Build the project
cargo build --release

# Inspect a Parquet file
tabletools inspect data.parquet

# Validate a file
tabletools validate s3://bucket/data.parquet

# Compare two tables
tabletools diff old.parquet new.parquet

# Convert formats
tabletools convert data.csv -o data.parquet

# Compute statistics
tabletools stats data.parquet --histogram

# Query with SQL
tabletools query "SELECT * FROM data.parquet WHERE age > 30"
```

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for detailed architecture documentation.

Key architectural decisions:
- Trait-based design for extensibility
- Async-first for efficient I/O
- Streaming to minimize memory usage
- User-friendly error messages with suggestions

## Project Structure

```
tabletools/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library interface
│   ├── error.rs             # Error types
│   ├── core/                # Core abstractions
│   │   ├── formats/         # Format handlers
│   │   ├── storage/         # Storage backends
│   │   └── operations/      # Business logic
│   ├── cli/                 # CLI layer
│   │   ├── parser.rs        # Argument parsing
│   │   ├── output.rs        # Output formatting
│   │   └── commands/        # Command implementations
│   └── utils/               # Utilities
├── tests/                   # Integration tests
├── benches/                 # Benchmarks
├── examples/                # Usage examples
├── ARCHITECTURE.md          # Architecture documentation
└── DEV-GUIDE.md            # Development specifications
```

## Development

### Prerequisites

- Rust 1.70+ (2021 edition)
- Cargo

### Build

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# With all features
cargo build --all-features

# Minimal build (Parquet/Arrow only)
cargo build --no-default-features
```

### Test

```bash
# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo test

# Run benchmarks
cargo bench
```

### Features

- `delta`: Delta Lake support (default: enabled)
- `iceberg`: Iceberg support (default: disabled)
- `serve`: Web UI server (default: enabled)

## Documentation

- [DEV-GUIDE.md](DEV-GUIDE.md): Complete product specifications
- [ARCHITECTURE.md](ARCHITECTURE.md): System architecture and design decisions

## Contributing

Contributions are welcome! This project follows standard Rust conventions:

1. Run `cargo fmt` before committing
2. Run `cargo clippy` and fix warnings
3. Add tests for new functionality
4. Update documentation

## License

MIT OR Apache-2.0 (dual licensed)

## Roadmap

- [ ] v0.1 (MVP): Parquet inspect, validate, convert (local only)
- [ ] v0.5: Cloud storage support (S3, GCS, Azure)
- [ ] v1.0: Full feature set with Delta/Iceberg read support
- [ ] v1.5: Web UI, plugin system
- [ ] v2.0: Write support for table formats

---

**Note**: This project is currently in active development. The architecture is complete, but implementations are in progress. See [ARCHITECTURE.md](ARCHITECTURE.md) for details on extension points and how to contribute.

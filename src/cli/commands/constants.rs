//! Memory estimation constants for CLI commands
//!
//! These constants define the estimated memory requirements for various operations.
//! They are used with `with_resource_limits()` to enable memory tracking and limits.
//!
//! # Memory Tiers
//!
//! - **Small (8-16 MB)**: Config operations, init
//! - **Light (32 MB)**: Simple CRUD operations (create, delete, refs)
//! - **Medium (64 MB)**: Read operations (validate, history, stats, doctor, snapshot)
//! - **Heavy (128 MB)**: Analysis operations (inspect, analyze, diff, delta import)
//! - **Intensive (256 MB)**: Write-heavy operations (repair, vacuum, parquet import)

/// Memory for config operations (8 MB)
/// Used for: loading/saving config files, simple I/O
pub const MEMORY_CONFIG_OPS: u64 = 8 * 1024 * 1024;

/// Memory for init operations (16 MB)
/// Used for: initializing new table metadata
pub const MEMORY_INIT_OPS: u64 = 16 * 1024 * 1024;

/// Memory for light operations (32 MB)
/// Used for: create table, delete, branch/tag operations
pub const MEMORY_LIGHT_OPS: u64 = 32 * 1024 * 1024;

/// Memory for medium operations (64 MB)
/// Used for: validation, history, stats, doctor, snapshot listing
pub const MEMORY_MEDIUM_OPS: u64 = 64 * 1024 * 1024;

/// Memory for heavy operations (128 MB)
/// Used for: inspection, analysis, diff, Delta import
pub const MEMORY_HEAVY_OPS: u64 = 128 * 1024 * 1024;

/// Memory for intensive operations (256 MB)
/// Used for: repair, vacuum (manifest scanning), Parquet import
pub const MEMORY_INTENSIVE_OPS: u64 = 256 * 1024 * 1024;

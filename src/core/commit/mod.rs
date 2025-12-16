//! Commit abstractions for Iceberg tables
//!
//! This module provides a unified interface for committing changes to Iceberg tables,
//! whether they are managed by a catalog or accessed directly (static tables).
//!
//! # Architecture
//!
//! ```text
//! SnapshotCommitter (trait)
//! ├── DirectCommitter   - writes directly to storage (no concurrency control)
//! └── CatalogCommitter  - uses catalog API (with concurrency control) [future]
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use icetable::core::commit::{SnapshotCommitter, DirectCommitter};
//!
//! // Create committer
//! let committer = DirectCommitter::new(table_path, storage);
//!
//! // Commit a snapshot
//! let result = committer
//!     .commit_snapshot(current_metadata, snapshot, "main")
//!     .await?;
//! ```

mod direct_committer;
mod traits;

pub use direct_committer::DirectCommitter;
pub use traits::{CommitResult, SnapshotCommitter};

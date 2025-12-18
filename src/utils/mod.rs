//! CLI Utility modules
//!
//! This module contains **CLI-specific** utilities that depend on terminal/UI libraries
//! like indicatif. These should NOT be used by core library code.
//!
//! # Module Organization
//!
//! - `src/utils/` - CLI utilities (this module): progress bars, logging, telemetry, etc.
//! - `src/core/utils/` - Shared/core utilities: format detection, file operations, etc.
//!
//! # For Core Services
//!
//! Core services should use:
//! - `crate::core::progress::ProgressReporter` trait for progress reporting
//! - `crate::core::utils::*` for shared utilities
//!
//! # Re-exports
//!
//! This module re-exports `crate::core::utils::*` for convenience.

// CLI utilities
pub mod box_frame;
pub mod cancellation;
pub mod logging;
pub mod progress;
pub mod resources;
pub mod telemetry;
pub mod text;
pub mod time;
pub mod types;

// CLI re-exports
pub use box_frame::create_box_frame;
pub use cancellation::{
    CancellationToken, CancellationTokenSource, check_cancellation, is_cancelled,
    register_cleanup_handler, request_cancellation, setup_signal_handlers, temp_dir_with_cleanup,
    with_cancellation,
};
pub use logging::{LogLevel, init_logger};

// Re-export CredentialSource from core::config
pub use crate::core::config::CredentialSource;
pub use progress::{ProgressTracker, create_progress_bar, create_spinner};
pub use resources::{
    ResourceLimits, current_memory_usage, get_resource_limits, init_resource_limits,
    release_memory, track_memory_usage, with_resource_limits, with_timeout,
};
pub use telemetry::TelemetryCollector;
pub use text::{strip_ansi_codes, visual_width, wrap_line};
pub use time::{parse_relative_duration, parse_timestamp};
pub use types::parse_data_type;

// Core re-exports
pub use crate::core::utils as core;
pub use crate::core::utils::{
    ExpirationConfig, ScannedFile, SnapshotItem, TableFormat, WriteMetadataResult, detect_format,
    detect_table_format, detect_table_format_async, determine_cutoff_timestamp,
    determine_snapshots_to_expire, extract_version_from_path, find_latest_metadata, format_bytes,
    generate_unique_id, metadata_location_filename, new_metadata_location, next_metadata_location,
    normalize_path, normalize_relative_path, parse_bytes, read_parquet_record_count,
    scan_parquet_files, sizes, write_metadata_file,
};

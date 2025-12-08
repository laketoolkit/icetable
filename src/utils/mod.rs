//! Utility modules
//!
//! Unified utilities for both CLI and core operations.

// CLI utilities
pub mod box_frame;
pub mod cancellation;
pub mod credentials;
pub mod logging;
pub mod progress;
pub mod resources;
pub mod telemetry;
pub mod text;
pub mod time;
pub mod types;

// Core/domain utilities (previously in crate::core::utils)
pub mod core;

// CLI re-exports
pub use box_frame::create_box_frame;
pub use cancellation::{
    check_cancellation, is_cancelled, register_cleanup_handler, request_cancellation,
    setup_signal_handlers, temp_dir_with_cleanup, with_cancellation, CancellationToken,
    CancellationTokenSource,
};
pub use credentials::CredentialSource;
pub use logging::{LogLevel, init_logger};
pub use progress::ProgressTracker;
pub use resources::{ResourceLimits, get_resource_limits, init_resource_limits, with_timeout, track_memory_usage, release_memory, current_memory_usage};
pub use telemetry::TelemetryCollector;
pub use text::{strip_ansi_codes, visual_width, wrap_line};
pub use time::{parse_relative_duration, parse_timestamp};
pub use types::parse_data_type;

// Core re-exports (for backward compatibility with crate::core::utils paths)
pub use core::{
    format_bytes, generate_unique_id, parse_bytes, sizes,
    TableFormat, detect_table_format, detect_table_format_async, detect_table_format_with_storage,
    ScannedFile, normalize_path, normalize_relative_path, scan_parquet_files,
    iceberg_to_arrow_type, extract_version_from_path, find_latest_metadata,
    metadata_location_filename, new_metadata_location, next_metadata_location,
    read_parquet_record_count,
    ExpirationConfig, SnapshotItem, determine_cutoff_timestamp, determine_snapshots_to_expire,
};

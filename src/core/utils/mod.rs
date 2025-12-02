//! Shared utility modules
//!
//! Common utilities used across the codebase.

pub mod format_detection;
pub mod fs;
pub mod parquet;

pub use format_detection::{detect_table_format, TableFormat};
pub use fs::{normalize_path, scan_parquet_files, ScannedFile};
pub use parquet::read_parquet_record_count;

/// Default file sizes for maintenance operations (in bytes)
pub mod sizes {
    /// Default target file size: 256 MB
    pub const DEFAULT_TARGET_SIZE: u64 = 256 * 1024 * 1024;
    /// Default minimum file size: 16 MB
    pub const DEFAULT_MIN_SIZE: u64 = 16 * 1024 * 1024;
    /// Default maximum file size: 512 MB
    pub const DEFAULT_MAX_SIZE: u64 = 512 * 1024 * 1024;
    /// Default retention hours for vacuum: 168 (7 days)
    pub const DEFAULT_RETENTION_HOURS: u64 = 168;
}

/// Format bytes to human-readable string
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Generate a timestamp-based unique ID
pub fn generate_unique_id() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

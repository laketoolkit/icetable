//! Core utility modules
//!
//! Domain-specific utilities for table operations.

pub mod format_detection;
pub mod fs;
pub mod iceberg;
pub mod parquet;
pub mod snapshot;

pub use format_detection::{
    TableFormat, detect_format, detect_table_format, detect_table_format_async,
};
pub use fs::{ScannedFile, normalize_path, normalize_relative_path, scan_parquet_files};
pub use iceberg::{
    WriteMetadataResult, extract_version_from_path, find_latest_metadata,
    metadata_location_filename, new_metadata_location, next_metadata_location, write_metadata_file,
};
pub use parquet::read_parquet_record_count;
pub use snapshot::{
    ExpirationConfig, SnapshotItem, determine_cutoff_timestamp, determine_snapshots_to_expire,
};

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
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Parse human-readable size string to bytes
///
/// Supports formats like: "10GB", "500MB", "1TB", "100KB", or plain bytes "1234567890"
pub fn parse_bytes(s: &str) -> Result<u64, String> {
    let s = s.trim().to_uppercase();

    // Try to parse as plain number first
    if let Ok(bytes) = s.parse::<u64>() {
        return Ok(bytes);
    }

    // Parse with unit suffix
    let (num_str, multiplier) = if s.ends_with("TB") {
        (&s[..s.len() - 2], 1024u64 * 1024 * 1024 * 1024)
    } else if s.ends_with("GB") {
        (&s[..s.len() - 2], 1024u64 * 1024 * 1024)
    } else if s.ends_with("MB") {
        (&s[..s.len() - 2], 1024u64 * 1024)
    } else if s.ends_with("KB") {
        (&s[..s.len() - 2], 1024u64)
    } else if s.ends_with('T') {
        (&s[..s.len() - 1], 1024u64 * 1024 * 1024 * 1024)
    } else if s.ends_with('G') {
        (&s[..s.len() - 1], 1024u64 * 1024 * 1024)
    } else if s.ends_with('M') {
        (&s[..s.len() - 1], 1024u64 * 1024)
    } else if s.ends_with('K') {
        (&s[..s.len() - 1], 1024u64)
    } else if s.ends_with('B') {
        (&s[..s.len() - 1], 1u64)
    } else {
        return Err(format!(
            "Invalid size format: '{}'. Use formats like '10GB', '500MB', '1TB'",
            s
        ));
    };

    let num: f64 = num_str
        .trim()
        .parse()
        .map_err(|_| format!("Invalid number in size: '{}'", num_str))?;

    Ok((num * multiplier as f64) as u64)
}

/// Generate a timestamp-based unique ID
pub fn generate_unique_id() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

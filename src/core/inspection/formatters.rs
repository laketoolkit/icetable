//! Formatting utilities for physical inspection output

use std::path::Path;

// Re-export format_bytes from central location
pub use crate::utils::core::format_bytes;

/// Format number with thousands separators
pub fn format_number(n: i64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) && *c != '-' {
            result.push(',');
        }
        result.push(*c);
    }

    result
}

/// Format count in compact form (e.g., 1.5K, 2.3M)
pub fn format_count(count: usize) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Format percentage
pub fn format_percentage(value: f64) -> String {
    format!("{:.2}%", value * 100.0)
}

/// Format compression ratio
pub fn format_compression_ratio(compressed: u64, uncompressed: u64) -> String {
    if uncompressed == 0 {
        return "N/A".to_string();
    }
    let ratio = compressed as f64 / uncompressed as f64;
    format!("{:.2}x", 1.0 / ratio)
}

/// Extract filename from path
pub fn extract_filename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Get file name from path (alias for extract_filename)
pub fn get_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 bytes");
        assert_eq!(format_bytes(512), "512 bytes");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_bytes(1024u64 * 1024 * 1024 * 1024), "1.00 TB");
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1000000), "1,000,000");
        assert_eq!(format_number(-1000), "-1,000");
    }

    #[test]
    fn test_format_percentage() {
        assert_eq!(format_percentage(0.0), "0.00%");
        assert_eq!(format_percentage(0.5), "50.00%");
        assert_eq!(format_percentage(1.0), "100.00%");
    }

    #[test]
    fn test_format_compression_ratio() {
        assert_eq!(format_compression_ratio(50, 100), "2.00x");
        assert_eq!(format_compression_ratio(100, 200), "2.00x");
        assert_eq!(format_compression_ratio(0, 0), "N/A");
        assert_eq!(format_compression_ratio(100, 0), "N/A");
    }

    #[test]
    fn test_extract_filename() {
        assert_eq!(extract_filename("/path/to/file.parquet"), "file.parquet");
        assert_eq!(extract_filename("file.parquet"), "file.parquet");
        assert_eq!(extract_filename("/path/to/"), "to");
    }
}

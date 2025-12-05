//! Iceberg partition parsing and conversion utilities
//!
//! This module provides functions for parsing partition values from strings
//! and converting them to Iceberg literals based on partition transforms.

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Utc};
use iceberg::spec::{Literal, PrimitiveLiteral, Transform};
use std::collections::HashMap;

/// Parse a date string like "2024-10-07" to days since Unix epoch
pub fn parse_date_to_days_since_epoch(date_str: &str) -> Option<i32> {
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)?;
    Some((date - epoch).num_days() as i32)
}

/// Parse a date string to months since Unix epoch (year * 12 + month - 1)
pub fn parse_date_to_months_since_epoch(date_str: &str) -> Option<i32> {
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(&format!("{}-01", date_str), "%Y-%m-%d"))
        .ok()?;
    let year = date.year();
    let month = date.month() as i32;
    Some((year - 1970) * 12 + month - 1)
}

/// Parse a year string to year value
pub fn parse_year(year_str: &str) -> Option<i32> {
    year_str.parse::<i32>().ok()
}

/// Parse an hour string to hours since Unix epoch
///
/// Supports multiple formats:
/// - Integer: direct hours since epoch (e.g., "473256")
/// - Relative: "7d", "24h", "2w" (from now, going back)
/// - Absolute datetime: "2024-01-15T10:00:00"
/// - Absolute date: "2024-01-15" (uses hour 0)
pub fn parse_hour(hour_str: &str) -> Option<i32> {
    let s = hour_str.trim();

    // Try direct integer first (hours since epoch)
    if let Ok(hours) = s.parse::<i32>() {
        return Some(hours);
    }

    // Try relative duration (e.g., "7d", "24h", "2w")
    if let Some(hours) = parse_relative_to_hours(s) {
        return Some(hours);
    }

    // Unix epoch as NaiveDateTime for calculations
    let epoch = DateTime::<Utc>::UNIX_EPOCH.naive_utc();

    // Try absolute datetime formats
    // Format: "2024-01-15T10:00:00"
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        let hours = (dt - epoch).num_hours() as i32;
        return Some(hours);
    }

    // Format: "2024-01-15 10:00:00"
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        let hours = (dt - epoch).num_hours() as i32;
        return Some(hours);
    }

    // Format: "2024-01-15" (date only, assume hour 0)
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = date.and_hms_opt(0, 0, 0)?;
        let hours = (dt - epoch).num_hours() as i32;
        return Some(hours);
    }

    None
}

/// Parse relative duration string to hours since epoch (current time minus duration)
///
/// Supports: "7d" (days), "24h" (hours), "2w" (weeks)
pub fn parse_relative_to_hours(s: &str) -> Option<i32> {
    let s = s.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = if s.ends_with('d') {
        (&s[..s.len() - 1], 'd')
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 'h')
    } else if s.ends_with('w') {
        (&s[..s.len() - 1], 'w')
    } else {
        return None;
    };

    let num: i64 = num_str.parse().ok()?;

    let duration_hours = match unit {
        'w' => num * 7 * 24,
        'd' => num * 24,
        'h' => num,
        _ => return None,
    };

    // Calculate hours since epoch for (now - duration)
    let now = Utc::now().naive_utc();
    let target = now - chrono::Duration::hours(duration_hours);
    let epoch = DateTime::<Utc>::UNIX_EPOCH.naive_utc();
    let hours = (target - epoch).num_hours() as i32;

    Some(hours)
}

/// Extract partition information from a file path string
pub fn extract_partition_from_path_static(path: &str) -> HashMap<String, String> {
    let mut partition = HashMap::new();

    // Parse partition values from the file path (e.g., "year=2024/month=01/file.parquet")
    for component in path.split('/') {
        if let Some(eq_pos) = component.find('=') {
            let key = component[..eq_pos].to_string();
            let value = component[eq_pos + 1..].to_string();
            partition.insert(key, value);
        }
    }

    partition
}

/// Convert a partition value string to the appropriate Literal based on transform
pub fn convert_partition_value(value: &str, transform: &Transform) -> Option<Literal> {
    match transform {
        Transform::Identity => {
            // Identity transform keeps the original type - for strings extracted from paths, use String
            Some(Literal::Primitive(PrimitiveLiteral::String(
                value.to_string(),
            )))
        }
        Transform::Day => {
            // Day transform produces Integer (days since epoch)
            // Value is like "2024-10-07"
            parse_date_to_days_since_epoch(value)
                .map(|days| Literal::Primitive(PrimitiveLiteral::Int(days)))
        }
        Transform::Month => {
            // Month transform produces Integer (months since epoch)
            parse_date_to_months_since_epoch(value)
                .map(|months| Literal::Primitive(PrimitiveLiteral::Int(months)))
        }
        Transform::Year => {
            // Year transform produces Integer (years since epoch)
            parse_year(value).map(|year| Literal::Primitive(PrimitiveLiteral::Int(year)))
        }
        Transform::Hour => {
            // Hour transform produces Integer (hours since epoch)
            parse_hour(value).map(|hours| Literal::Primitive(PrimitiveLiteral::Int(hours)))
        }
        Transform::Bucket(_) | Transform::Truncate(_) => {
            // Bucket and Truncate produce Integer
            value
                .parse::<i32>()
                .ok()
                .map(|v| Literal::Primitive(PrimitiveLiteral::Int(v)))
        }
        Transform::Void => None,
        _ => {
            // Unknown transform - try as string
            Some(Literal::Primitive(PrimitiveLiteral::String(
                value.to_string(),
            )))
        }
    }
}

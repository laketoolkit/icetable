//! Time parsing utilities

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};

use crate::error::{Error, Result};

/// Parse a timestamp string into a UTC DateTime
///
/// Supports:
/// - Absolute: "2024-01-15", "2024-01-15T10:30:00", "2024-01-15 10:30:00"
/// - Relative durations: "7d" (days), "24h" (hours), "30m" (minutes), "2w" (weeks)
pub fn parse_timestamp(s: &str) -> Result<DateTime<Utc>> {
    let s = s.trim();

    // Try relative duration first (e.g., "7d", "24h", "30m", "2w")
    if let Some(duration) = parse_relative_duration(s) {
        return Ok(Utc::now() - duration);
    }

    // Try absolute formats
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt.and_utc());
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(dt.and_utc());
    }
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        // Safe: 0,0,0 is always a valid time
        return Ok(date.and_hms_opt(0, 0, 0).expect("00:00:00 is a valid time").and_utc());
    }

    Err(Error::General(format!(
        "Invalid timestamp format: '{}'. Expected:\n  \
         - Relative: 7d (days), 24h (hours), 30m (minutes), 2w (weeks)\n  \
         - Absolute: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS",
        s
    )))
}

/// Parse relative duration string (e.g., "7d", "24h", "30m", "2w")
pub fn parse_relative_duration(s: &str) -> Option<Duration> {
    let s = s.trim().to_lowercase();

    if s.is_empty() {
        return None;
    }

    // Split into number and unit
    let (num_str, unit) = if s.ends_with('d') {
        (&s[..s.len() - 1], 'd')
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 'h')
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 'm')
    } else if s.ends_with('w') {
        (&s[..s.len() - 1], 'w')
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], 's')
    } else {
        return None;
    };

    let num: i64 = num_str.parse().ok()?;

    match unit {
        'w' => Some(Duration::weeks(num)),
        'd' => Some(Duration::days(num)),
        'h' => Some(Duration::hours(num)),
        'm' => Some(Duration::minutes(num)),
        's' => Some(Duration::seconds(num)),
        _ => None,
    }
}

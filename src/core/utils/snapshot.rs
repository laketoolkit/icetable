//! Snapshot-related utilities

use chrono::{DateTime, Utc};
use std::sync::Arc;

use crate::error::Result;
use crate::utils::time::parse_timestamp;

/// Configuration for snapshot expiration
#[derive(Debug, Clone)]
pub struct ExpirationConfig {
    /// Expire snapshots older than this timestamp
    pub older_than: Option<String>,
    /// Retain this many most recent snapshots
    pub retain_last: Option<usize>,
    /// Specific snapshot IDs to expire
    pub ids: Option<Vec<i64>>,
    /// Whether to skip the current snapshot
    pub skip_current: bool,
}

impl Default for ExpirationConfig {
    fn default() -> Self {
        Self {
            older_than: None,
            retain_last: None,
            ids: None,
            skip_current: true,
        }
    }
}

/// Information about a snapshot for expiration decisions
pub trait SnapshotItem {
    /// Get the snapshot ID
    fn id(&self) -> i64;
    /// Get the snapshot timestamp
    fn timestamp(&self) -> Option<DateTime<Utc>>;
}

/// Determine which snapshots to expire based on configuration
pub fn determine_snapshots_to_expire<I>(
    items: &[I],
    config: &ExpirationConfig,
    current_id: Option<i64>,
) -> Result<Vec<i64>>
where
    I: SnapshotItem,
{
    let mut to_expire = Vec::new();

    if let Some(ids) = &config.ids {
        // Explicit IDs
        for id in ids {
            if config.skip_current && Some(*id) == current_id {
                continue;
            }
            if items.iter().any(|item| item.id() == *id) {
                to_expire.push(*id);
            }
        }
        return Ok(to_expire);
    }

    if let Some(older_than) = &config.older_than {
        let cutoff = parse_timestamp(older_than)?;
        for item in items {
            if let Some(ts) = item.timestamp()
                && ts < cutoff
            {
                if config.skip_current && Some(item.id()) == current_id {
                    continue;
                }
                to_expire.push(item.id());
            }
        }
        return Ok(to_expire);
    }

    if let Some(retain) = config.retain_last {
        // Sort by timestamp descending (most recent first)
        let mut sorted: Vec<&I> = items.iter().collect();
        sorted.sort_by(|a, b| match (b.timestamp(), a.timestamp()) {
            (Some(tb), Some(ta)) => tb.cmp(&ta),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.id().cmp(&a.id()),
        });

        // Skip the first N, expire the rest
        for item in sorted.iter().skip(retain) {
            if config.skip_current && Some(item.id()) == current_id {
                continue;
            }
            to_expire.push(item.id());
        }
        return Ok(to_expire);
    }

    // Default: 7 days retention
    let cutoff = Utc::now() - chrono::Duration::days(7);
    for item in items {
        if let Some(ts) = item.timestamp()
            && ts < cutoff
        {
            if config.skip_current && Some(item.id()) == current_id {
                continue;
            }
            to_expire.push(item.id());
        }
    }

    Ok(to_expire)
}

/// Determine cutoff timestamp for expiration
pub fn determine_cutoff_timestamp<I>(
    items: &[I],
    older_than: Option<&str>,
    retain_last: Option<usize>,
) -> Result<DateTime<Utc>>
where
    I: SnapshotItem,
{
    if let Some(older_than) = older_than {
        return parse_timestamp(older_than);
    }

    if let Some(retain) = retain_last {
        // Sort by timestamp descending
        let mut sorted: Vec<&I> = items.iter().collect();
        sorted.sort_by(|a, b| match (b.timestamp(), a.timestamp()) {
            (Some(tb), Some(ta)) => tb.cmp(&ta),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.id().cmp(&a.id()),
        });

        if sorted.len() <= retain {
            // Return a timestamp in the distant past so nothing is expired
            return Ok(DateTime::from_timestamp(0, 0).unwrap());
        }

        // Get timestamp of the (retain)th newest item
        return Ok(sorted
            .get(retain)
            .and_then(|item| item.timestamp())
            .unwrap_or_else(Utc::now));
    }

    // Default: 7 days retention
    Ok(Utc::now() - chrono::Duration::days(7))
}
impl SnapshotItem for iceberg::spec::Snapshot {
    fn id(&self) -> i64 {
        self.snapshot_id()
    }

    fn timestamp(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        chrono::DateTime::from_timestamp_millis(self.timestamp_ms())
    }
}

impl SnapshotItem for Arc<iceberg::spec::Snapshot> {
    fn id(&self) -> i64 {
        self.snapshot_id()
    }

    fn timestamp(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        chrono::DateTime::from_timestamp_millis(self.timestamp_ms())
    }
}

impl<T: SnapshotItem> SnapshotItem for &T {
    fn id(&self) -> i64 {
        (**self).id()
    }

    fn timestamp(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        (**self).timestamp()
    }
}

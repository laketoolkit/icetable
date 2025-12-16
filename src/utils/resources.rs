//! Resource limits management
//!
//! Provides utilities for parsing and applying memory limits, timeouts,
//! and concurrency controls for CLI operations.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::error::{Error, Result};

/// Global resource limits configuration
static RESOURCE_LIMITS: OnceLock<ResourceLimits> = OnceLock::new();

/// Resource limits for operations
#[derive(Debug, Clone, Default)]
pub struct ResourceLimits {
    /// Maximum memory in bytes (0 = unlimited)
    pub max_memory_bytes: u64,
    /// Operation timeout (None = no timeout)
    pub timeout: Option<Duration>,
    /// Maximum concurrent operations (0 = use system default)
    pub max_concurrency: u32,
}

impl ResourceLimits {
    /// Parse memory string (e.g., "2GB", "512MB", "1024KB") to bytes
    pub fn parse_memory(s: &str) -> Result<u64> {
        let s = s.trim();

        if s == "0" || s.is_empty() {
            return Ok(0);
        }

        // Use the shared parse_bytes function but convert error type
        crate::core::utils::parse_bytes(s).map_err(|e| Error::Parse {
            message: format!("Invalid memory format: {}", e),
            source: None,
        })
    }

    /// Create limits from CLI arguments
    pub fn from_cli(max_memory: &str, timeout_secs: u64, max_concurrency: u32) -> Result<Self> {
        let max_memory_bytes = Self::parse_memory(max_memory)?;

        let timeout = if timeout_secs > 0 {
            Some(Duration::from_secs(timeout_secs))
        } else {
            None
        };

        Ok(Self {
            max_memory_bytes,
            timeout,
            max_concurrency,
        })
    }

    /// Check if memory limit is set
    pub fn has_memory_limit(&self) -> bool {
        self.max_memory_bytes > 0
    }

    /// Check if timeout is set
    pub fn has_timeout(&self) -> bool {
        self.timeout.is_some()
    }

    /// Get effective concurrency (0 means use available parallelism)
    pub fn effective_concurrency(&self) -> usize {
        if self.max_concurrency > 0 {
            self.max_concurrency as usize
        } else {
            std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(4)
        }
    }

    /// Format memory for display
    pub fn format_memory(bytes: u64) -> String {
        if bytes == 0 {
            "unlimited".to_string()
        } else if bytes >= 1024 * 1024 * 1024 {
            format!("{:.1}GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        } else if bytes >= 1024 * 1024 {
            format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
        } else if bytes >= 1024 {
            format!("{:.1}KB", bytes as f64 / 1024.0)
        } else {
            format!("{}B", bytes)
        }
    }
}

/// Initialize global resource limits (call once at startup)
pub fn init_resource_limits(limits: ResourceLimits) {
    let _ = RESOURCE_LIMITS.set(limits);
}

/// Get current resource limits
pub fn get_resource_limits() -> &'static ResourceLimits {
    RESOURCE_LIMITS.get_or_init(ResourceLimits::default)
}

/// Memory tracker for checking against limits
static CURRENT_MEMORY: AtomicU64 = AtomicU64::new(0);

/// Track memory allocation (approximate)
pub fn track_memory_usage(bytes: u64) -> Result<()> {
    let limits = get_resource_limits();

    if limits.max_memory_bytes == 0 {
        return Ok(());
    }

    let new_total = CURRENT_MEMORY.fetch_add(bytes, Ordering::SeqCst) + bytes;

    if new_total > limits.max_memory_bytes {
        CURRENT_MEMORY.fetch_sub(bytes, Ordering::SeqCst);
        return Err(Error::Configuration {
            message: format!(
                "Memory limit exceeded: operation requires {} but limit is {}",
                ResourceLimits::format_memory(new_total),
                ResourceLimits::format_memory(limits.max_memory_bytes)
            ),
        });
    }

    Ok(())
}

/// Release tracked memory
pub fn release_memory(bytes: u64) {
    CURRENT_MEMORY.fetch_sub(bytes, Ordering::SeqCst);
}

/// Get current tracked memory usage
pub fn current_memory_usage() -> u64 {
    CURRENT_MEMORY.load(Ordering::SeqCst)
}

/// Run an async operation with timeout
pub async fn with_timeout<F, T>(operation: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    let limits = get_resource_limits();

    match limits.timeout {
        Some(duration) => tokio::time::timeout(duration, operation)
            .await
            .map_err(|_| Error::Configuration {
                message: format!("Operation timed out after {} seconds", duration.as_secs()),
            })?,
        None => operation.await,
    }
}

/// Run an async operation with all resource limits (timeout, cancellation, memory)
///
/// This is a convenience wrapper that combines:
/// - Timeout (from global resource limits)
/// - Cancellation checking
/// - Memory tracking
///
/// # Arguments
/// * `estimated_memory` - Estimated memory usage in bytes
/// * `operation` - The async operation to run
///
/// # Example
/// ```ignore
/// with_resource_limits(256 * 1024 * 1024, async {
///     // actual work here
/// }).await
/// ```
pub async fn with_resource_limits<F, T>(estimated_memory: u64, operation: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    with_timeout(async {
        super::cancellation::with_cancellation(async {
            track_memory_usage(estimated_memory)?;
            let result = operation.await;
            release_memory(estimated_memory);
            result
        })
        .await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_memory_gb() {
        assert_eq!(
            ResourceLimits::parse_memory("2GB").unwrap(),
            2 * 1024 * 1024 * 1024
        );
        assert_eq!(
            ResourceLimits::parse_memory("1gb").unwrap(),
            1024 * 1024 * 1024
        );
    }

    #[test]
    fn test_parse_memory_mb() {
        assert_eq!(
            ResourceLimits::parse_memory("512MB").unwrap(),
            512 * 1024 * 1024
        );
        assert_eq!(
            ResourceLimits::parse_memory("256mb").unwrap(),
            256 * 1024 * 1024
        );
    }

    #[test]
    fn test_parse_memory_kb() {
        assert_eq!(ResourceLimits::parse_memory("1024KB").unwrap(), 1024 * 1024);
    }

    #[test]
    fn test_parse_memory_zero() {
        assert_eq!(ResourceLimits::parse_memory("0").unwrap(), 0);
        assert_eq!(ResourceLimits::parse_memory("").unwrap(), 0);
    }

    #[test]
    fn test_parse_memory_invalid() {
        assert!(ResourceLimits::parse_memory("invalid").is_err());
        assert!(ResourceLimits::parse_memory("2XB").is_err());
    }

    #[test]
    fn test_format_memory() {
        assert_eq!(ResourceLimits::format_memory(0), "unlimited");
        assert_eq!(ResourceLimits::format_memory(1024 * 1024 * 1024), "1.0GB");
        assert_eq!(ResourceLimits::format_memory(512 * 1024 * 1024), "512.0MB");
        assert_eq!(ResourceLimits::format_memory(1024 * 1024), "1.0MB");
    }

    #[test]
    fn test_effective_concurrency() {
        let limits = ResourceLimits {
            max_memory_bytes: 0,
            timeout: None,
            max_concurrency: 4,
        };
        assert_eq!(limits.effective_concurrency(), 4);

        let limits_default = ResourceLimits::default();
        assert!(limits_default.effective_concurrency() > 0);
    }

    #[test]
    fn test_from_cli() {
        let limits = ResourceLimits::from_cli("2GB", 300, 8).unwrap();
        assert_eq!(limits.max_memory_bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(limits.timeout, Some(Duration::from_secs(300)));
        assert_eq!(limits.max_concurrency, 8);
    }

    #[test]
    fn test_from_cli_unlimited() {
        let limits = ResourceLimits::from_cli("0", 0, 0).unwrap();
        assert_eq!(limits.max_memory_bytes, 0);
        assert_eq!(limits.timeout, None);
        assert_eq!(limits.max_concurrency, 0);
    }
}

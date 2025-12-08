//! Retry logic with exponential backoff and circuit breaker for storage operations
//!
//! Provides automatic retry for transient failures (network issues, throttling, etc.)
//! using exponential backoff with jitter and circuit breaker pattern to prevent
//! cascading failures.

use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq)]
enum CircuitState {
    /// Circuit is closed - requests pass through normally
    Closed,
    /// Circuit is open - requests fail fast without attempting
    Open,
    /// Circuit is half-open - allowing limited requests to test if service recovered
    HalfOpen,
}

/// Circuit breaker statistics
#[derive(Debug)]
struct CircuitStats {
    /// Number of consecutive failures
    failures: AtomicU32,
    /// Number of consecutive successes in half-open state
    half_open_successes: AtomicU32,
    /// Last failure time
    last_failure: parking_lot::RwLock<Option<Instant>>,
    /// Last success time
    last_success: parking_lot::RwLock<Option<Instant>>,
    /// Current circuit state
    state: parking_lot::RwLock<CircuitState>,
}

impl CircuitStats {
    fn new() -> Self {
        Self {
            failures: AtomicU32::new(0),
            half_open_successes: AtomicU32::new(0),
            last_failure: parking_lot::RwLock::new(None),
            last_success: parking_lot::RwLock::new(None),
            state: parking_lot::RwLock::new(CircuitState::Closed),
        }
    }

    fn record_failure(&self) {
        self.failures.fetch_add(1, Ordering::SeqCst);
        self.half_open_successes.store(0, Ordering::SeqCst);
        *self.last_failure.write() = Some(Instant::now());
    }

    fn record_success(&self) {
        self.failures.store(0, Ordering::SeqCst);
        *self.last_success.write() = Some(Instant::now());
        
        // If in half-open state, count successes
        if self.get_state() == CircuitState::HalfOpen {
            self.half_open_successes.fetch_add(1, Ordering::SeqCst);
        } else {
            self.half_open_successes.store(0, Ordering::SeqCst);
        }
    }

    fn get_half_open_successes(&self) -> u32 {
        self.half_open_successes.load(Ordering::SeqCst)
    }

    fn get_failure_count(&self) -> u32 {
        self.failures.load(Ordering::SeqCst)
    }

    fn get_state(&self) -> CircuitState {
        *self.state.read()
    }

    fn set_state(&self, state: CircuitState) {
        *self.state.write() = state;
    }

    fn time_since_last_failure(&self) -> Option<Duration> {
        self.last_failure.read().map(|t| t.elapsed())
    }
}

/// Configuration for retry behavior with circuit breaker
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (not including the initial attempt)
    pub max_retries: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff (e.g., 2.0 = double delay each retry)
    pub backoff_multiplier: f64,
    /// Whether to add jitter to delays (recommended to avoid thundering herd)
    pub add_jitter: bool,
    /// Circuit breaker: number of consecutive failures before opening circuit
    pub circuit_failure_threshold: u32,
    /// Circuit breaker: time to wait before attempting half-open state
    pub circuit_reset_timeout: Duration,
    /// Circuit breaker: number of successful requests in half-open to close circuit
    pub circuit_half_open_success_threshold: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            add_jitter: true,
            circuit_failure_threshold: 5,
            circuit_reset_timeout: Duration::from_secs(30),
            circuit_half_open_success_threshold: 3,
        }
    }
}

impl RetryConfig {
    /// Create a config optimized for cloud storage operations
    pub fn for_cloud_storage() -> Self {
        Self {
            max_retries: 5,
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            add_jitter: true,
            circuit_failure_threshold: 10,
            circuit_reset_timeout: Duration::from_secs(60),
            circuit_half_open_success_threshold: 5,
        }
    }

    /// Create a config for quick local operations
    pub fn for_local() -> Self {
        Self {
            max_retries: 2,
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            add_jitter: false,
            circuit_failure_threshold: 0, // No circuit breaker for local
            circuit_reset_timeout: Duration::from_secs(0),
            circuit_half_open_success_threshold: 0,
        }
    }

    /// Calculate delay for a given attempt number (0-indexed)
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base_delay = self.initial_delay.as_millis() as f64
            * self.backoff_multiplier.powi(attempt as i32);

        let capped_delay = base_delay.min(self.max_delay.as_millis() as f64);

        let final_delay = if self.add_jitter {
            // Add up to 25% jitter
            let jitter = rand_jitter() * 0.25 * capped_delay;
            capped_delay + jitter
        } else {
            capped_delay
        };

        Duration::from_millis(final_delay as u64)
    }
}

/// Simple pseudo-random jitter based on current time
/// Returns a value between 0.0 and 1.0
fn rand_jitter() -> f64 {
    use std::time::SystemTime;

    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    // Simple LCG-style randomness from nanoseconds
    ((nanos % 1000) as f64) / 1000.0
}

/// Check if an error is retryable
pub fn is_retryable_error(error: &Error) -> bool {
    match error {
        // Network errors are always retryable
        Error::Network { .. } => true,
        // Timeouts are retryable
        Error::Timeout { .. } => true,
        // General errors - check the message for known retryable patterns
        Error::General(msg) => {
            let msg_lower = msg.to_lowercase();
            // AWS/S3 throttling
            msg_lower.contains("throttl")
                || msg_lower.contains("slow down")
                || msg_lower.contains("rate exceeded")
                // Connection issues
                || msg_lower.contains("connection reset")
                || msg_lower.contains("connection refused")
                || msg_lower.contains("connection closed")
                || msg_lower.contains("broken pipe")
                // Temporary failures
                || msg_lower.contains("temporary")
                || msg_lower.contains("try again")
                || msg_lower.contains("service unavailable")
                || msg_lower.contains("503")
                || msg_lower.contains("502")
                || msg_lower.contains("504")
                // Timeout patterns
                || msg_lower.contains("timed out")
                || msg_lower.contains("timeout")
        }
        // ObjectStore errors - check for retryable patterns
        Error::ObjectStore(e) => {
            let msg = e.to_string().to_lowercase();
            msg.contains("throttl")
                || msg.contains("timeout")
                || msg.contains("connection")
                || msg.contains("503")
                || msg.contains("502")
                || msg.contains("504")
        }
        // CloudStorage errors - check error code and HTTP status
        Error::CloudStorage { error_code, http_status, .. } => {
            match error_code.as_deref() {
                Some("Throttling") | Some("SlowDown") | Some("RequestThrottled") => true,
                Some("NetworkError") | Some("Timeout") => true,
                _ => match http_status {
                    Some(429) | Some(503) | Some(502) | Some(504) => true, // Rate limiting and service errors
                    _ => false,
                }
            }
        }
        // These are not retryable
        Error::FileNotFound { .. } => false,
        Error::PermissionDenied { .. } => false,
        Error::Configuration { .. } => false,
        Error::Conflict(_) => false,
        _ => false,
    }
}

/// Shared circuit breaker state for storage operations
#[derive(Clone)]
pub struct RetryContext {
    config: RetryConfig,
    circuit_stats: Arc<CircuitStats>,
}

impl RetryContext {
    /// Create a new retry context with the given configuration
    pub fn new(config: RetryConfig) -> Self {
        Self {
            config,
            circuit_stats: Arc::new(CircuitStats::new()),
        }
    }

    /// Create a context with default cloud storage configuration
    pub fn for_cloud_storage() -> Self {
        Self::new(RetryConfig::for_cloud_storage())
    }

    /// Create a context with local storage configuration
    pub fn for_local() -> Self {
        Self::new(RetryConfig::for_local())
    }

    /// Check if circuit is open and should fail fast
    fn check_circuit(&self) -> Result<()> {
        if self.config.circuit_failure_threshold == 0 {
            return Ok(()); // Circuit breaker disabled
        }

        let state = self.circuit_stats.get_state();
        match state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open => {
                // Check if reset timeout has passed
                if let Some(time_since_failure) = self.circuit_stats.time_since_last_failure() {
                    if time_since_failure >= self.config.circuit_reset_timeout {
                        // Move to half-open state
                        self.circuit_stats.set_state(CircuitState::HalfOpen);
                        log::debug!("Circuit moved to half-open state after timeout");
                        Ok(())
                    } else {
                        Err(Error::General(format!(
                            "Circuit breaker open - failing fast (last failure: {:?} ago)",
                            time_since_failure
                        )))
                    }
                } else {
                    Err(Error::General("Circuit breaker open - failing fast".to_string()))
                }
            }
            CircuitState::HalfOpen => {
                // Allow request in half-open state
                Ok(())
            }
        }
    }

    /// Update circuit state based on operation success
    fn record_success(&self) {
        if self.config.circuit_failure_threshold == 0 {
            return; // Circuit breaker disabled
        }

        self.circuit_stats.record_success();
        let state = self.circuit_stats.get_state();
        
        match state {
            CircuitState::HalfOpen => {
                // Check if we have enough successes to close circuit
                let success_count = self.circuit_stats.get_half_open_successes();
                if success_count >= self.config.circuit_half_open_success_threshold {
                    self.circuit_stats.set_state(CircuitState::Closed);
                    log::info!("Circuit closed after {} successful requests in half-open state", success_count);
                }
            }
            CircuitState::Open => {
                // Should not happen - open circuit should fail fast
            }
            CircuitState::Closed => {
                // Already handled in circuit_stats.record_success()
            }
        }
    }

    /// Update circuit state based on operation failure
    fn record_failure(&self, error: &Error) {
        if self.config.circuit_failure_threshold == 0 {
            return; // Circuit breaker disabled
        }

        if is_retryable_error(error) {
            self.circuit_stats.record_failure();
            let failure_count = self.circuit_stats.get_failure_count();
            
            if failure_count >= self.config.circuit_failure_threshold {
                self.circuit_stats.set_state(CircuitState::Open);
                log::warn!("Circuit opened after {} consecutive failures", failure_count);
            }
        } else {
            // Non-retryable errors don't affect circuit state
            self.circuit_stats.record_success();
        }
    }

    /// Execute an async operation with retry, exponential backoff, and circuit breaker
    ///
    /// # Arguments
    /// * `operation_name` - Name of the operation for logging
    /// * `operation` - The async operation to execute
    ///
    /// # Returns
    /// The result of the operation, or the last error if all retries failed
    pub async fn execute<F, Fut, T>(&self, operation_name: &str, operation: F) -> Result<T>
where
    F: Fn() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        // Check circuit breaker first
        self.check_circuit()?;

        let mut attempt = 0u32;

        loop {
            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        log::debug!(
                            "{} succeeded after {} retries",
                            operation_name,
                            attempt
                        );
                    }
                    
                    // Update circuit with success
                    self.record_success();
                    
                    return Ok(result);
                }
                Err(e) => {
                    // Update circuit with failure
                    self.record_failure(&e);
                    
                    if !is_retryable_error(&e) || attempt >= self.config.max_retries {
                        if attempt > 0 {
                            log::warn!(
                                "{} failed after {} retries: {}",
                                operation_name,
                                attempt,
                                e
                            );
                        }
                        return Err(e);
                    }

                    let delay = self.config.delay_for_attempt(attempt);
                    log::debug!(
                        "{} attempt {} failed ({}), retrying in {:?}",
                        operation_name,
                        attempt + 1,
                        e,
                        delay
                    );

                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }
}

/// Execute an async operation with retry and exponential backoff
///
/// # Arguments
/// * `config` - Retry configuration
/// * `operation_name` - Name of the operation for logging
/// * `operation` - The async operation to execute
///
/// # Returns
/// The result of the operation, or the last error if all retries failed
pub async fn with_retry<F, Fut, T>(
    config: &RetryConfig,
    operation_name: &str,
    operation: F,
) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let context = RetryContext::new(config.clone());
    context.execute(operation_name, operation).await
}

/// A simpler retry wrapper that uses default cloud storage config
pub async fn retry_cloud_operation<F, Fut, T>(operation_name: &str, operation: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    with_retry(&RetryConfig::for_cloud_storage(), operation_name, operation).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_default_config() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay, Duration::from_millis(100));
    }

    #[test]
    fn test_cloud_storage_config() {
        let config = RetryConfig::for_cloud_storage();
        assert_eq!(config.max_retries, 5);
        assert!(config.add_jitter);
    }

    #[test]
    fn test_delay_calculation() {
        let config = RetryConfig {
            initial_delay: Duration::from_millis(100),
            backoff_multiplier: 2.0,
            max_delay: Duration::from_secs(10),
            add_jitter: false,
            ..Default::default()
        };

        // Without jitter, delays should be exactly exponential
        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(config.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(config.delay_for_attempt(2), Duration::from_millis(400));
    }

    #[test]
    fn test_delay_capped_at_max() {
        let config = RetryConfig {
            initial_delay: Duration::from_secs(1),
            backoff_multiplier: 10.0,
            max_delay: Duration::from_secs(5),
            add_jitter: false,
            ..Default::default()
        };

        // Should be capped at max_delay
        let delay = config.delay_for_attempt(5);
        assert!(delay <= Duration::from_secs(5) + Duration::from_millis(1));
    }

    #[test]
    fn test_is_retryable_network_error() {
        let error = Error::Network {
            message: "connection failed".to_string(),
            source: None,
        };
        assert!(is_retryable_error(&error));
    }

    #[test]
    fn test_is_retryable_timeout() {
        let error = Error::Timeout {
            operation: "read".to_string(),
            seconds: 30,
        };
        assert!(is_retryable_error(&error));
    }

    #[test]
    fn test_not_retryable_file_not_found() {
        let error = Error::FileNotFound {
            path: std::path::PathBuf::from("/tmp/test"),
        };
        assert!(!is_retryable_error(&error));
    }

    #[test]
    fn test_is_retryable_throttling() {
        let error = Error::General("SlowDown: Rate exceeded".to_string());
        assert!(is_retryable_error(&error));
    }

    #[tokio::test]
    async fn test_retry_succeeds_immediately() {
        let config = RetryConfig::default();
        let result = with_retry(&config, "test", || async { Ok::<_, Error>(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_succeeds_after_failures() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(1),
            ..Default::default()
        };

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = with_retry(&config, "test", || {
            let c = counter_clone.clone();
            async move {
                let attempt = c.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    Err(Error::Network {
                        message: "transient failure".to_string(),
                        source: None,
                    })
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_exhausted() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay: Duration::from_millis(1),
            ..Default::default()
        };

        let result: Result<i32> = with_retry(&config, "test", || async {
            Err(Error::Network {
                message: "persistent failure".to_string(),
                source: None,
            })
        })
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_no_retry_for_non_retryable_error() {
        let config = RetryConfig {
            max_retries: 5,
            initial_delay: Duration::from_millis(1),
            ..Default::default()
        };

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result: Result<i32> = with_retry(&config, "test", || {
            let c = counter_clone.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err(Error::FileNotFound {
                    path: std::path::PathBuf::from("/tmp/test"),
                })
            }
        })
        .await;

        assert!(result.is_err());
        // Should only try once since FileNotFound is not retryable
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}

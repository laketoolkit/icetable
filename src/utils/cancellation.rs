//! Cancellation handling for long-running operations
//!
//! Provides a global cancellation flag that can be set by signal handlers
//! and checked by operations to enable clean shutdown.
//!
//! Uses `tokio::sync::watch` for efficient async notification without polling.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::sync::watch;

/// Global cancellation flag (for sync checks)
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Type alias for cleanup handler storage
type CleanupHandlers = Mutex<Vec<Box<dyn Fn() + Send + Sync>>>;

/// Global cleanup handlers
static CLEANUP_HANDLERS: OnceLock<CleanupHandlers> = OnceLock::new();

/// Global watch channel for async cancellation notification
static CANCELLATION_CHANNEL: OnceLock<(watch::Sender<bool>, Mutex<watch::Receiver<bool>>)> =
    OnceLock::new();

/// Initialize or get the global cancellation channel
fn get_cancellation_channel() -> &'static (watch::Sender<bool>, Mutex<watch::Receiver<bool>>) {
    CANCELLATION_CHANNEL.get_or_init(|| {
        let (tx, rx) = watch::channel(false);
        (tx, Mutex::new(rx))
    })
}

/// Check if cancellation was requested
pub fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

/// Request cancellation and run cleanup handlers
pub fn request_cancellation() {
    CANCELLED.store(true, Ordering::SeqCst);

    // Notify async waiters via watch channel
    let (sender, _) = get_cancellation_channel();
    let _ = sender.send(true);

    // Run all cleanup handlers
    if let Some(handlers) = CLEANUP_HANDLERS.get()
        && let Ok(mut handlers_lock) = handlers.lock()
    {
        for handler in handlers_lock.drain(..) {
            handler();
        }
    }
}

/// Register a cleanup handler to be called on cancellation
pub fn register_cleanup_handler<F>(handler: F)
where
    F: Fn() + Send + Sync + 'static,
{
    if let Some(handlers) = CLEANUP_HANDLERS.get()
        && let Ok(mut handlers_lock) = handlers.lock()
    {
        handlers_lock.push(Box::new(handler));
    }
}

/// Create a temporary directory that will be cleaned up on cancellation
pub fn temp_dir_with_cleanup() -> std::io::Result<tempfile::TempDir> {
    let temp_dir = tempfile::TempDir::new()?;
    let temp_dir_path = temp_dir.path().to_path_buf();

    register_cleanup_handler(move || {
        let _ = std::fs::remove_dir_all(&temp_dir_path);
    });

    Ok(temp_dir)
}

/// Run cleanup handlers (useful for tests)
#[cfg(test)]
pub fn run_cleanup_handlers() {
    request_cancellation();
}

/// Reset cancellation flag (for testing)
#[cfg(test)]
pub fn reset_cancellation() {
    CANCELLED.store(false, Ordering::SeqCst);
    // Also reset the watch channel
    let (sender, _) = get_cancellation_channel();
    let _ = sender.send(false);
}

/// Cancellation token for async operations
#[derive(Clone)]
pub struct CancellationToken {
    receiver: watch::Receiver<bool>,
}

impl CancellationToken {
    /// Check if this specific token is cancelled
    ///
    /// Note: This only checks the local token state. For global cancellation,
    /// use the `is_cancelled()` function directly.
    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// Check if cancelled (either local token or global)
    pub fn is_cancelled_global(&self) -> bool {
        *self.receiver.borrow() || is_cancelled()
    }

    /// Wait until cancelled
    pub async fn cancelled(&mut self) {
        while !self.is_cancelled() {
            if self.receiver.changed().await.is_err() {
                break;
            }
        }
    }
}

/// Cancellation token source - owns the sender
pub struct CancellationTokenSource {
    sender: watch::Sender<bool>,
    token: CancellationToken,
}

impl CancellationTokenSource {
    /// Create a new cancellation token source
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(false);
        Self {
            sender,
            token: CancellationToken { receiver },
        }
    }

    /// Get a token that can be passed to operations
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Cancel all operations using this token
    ///
    /// Note: This only cancels the local token. Use `cancel_global()` to also
    /// trigger global cancellation (e.g., for signal handlers).
    pub fn cancel(&self) {
        let _ = self.sender.send(true);
    }

    /// Cancel this token and trigger global cancellation
    ///
    /// Use this when handling signals to ensure both local and global
    /// cancellation is triggered.
    pub fn cancel_global(&self) {
        self.cancel();
        request_cancellation();
    }
}

impl Default for CancellationTokenSource {
    fn default() -> Self {
        Self::new()
    }
}

/// Setup signal handlers for graceful shutdown
pub async fn setup_signal_handlers() -> CancellationTokenSource {
    let cts = CancellationTokenSource::new();

    #[cfg(unix)]
    {
        tokio::spawn(async move {
            let sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
            let sigint =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt());

            // If signal handlers fail to setup, gracefully continue without them
            match (sigterm, sigint) {
                (Ok(mut sigterm), Ok(mut sigint)) => {
                    tokio::select! {
                        _ = sigterm.recv() => {
                            request_cancellation();
                        }
                        _ = sigint.recv() => {
                            request_cancellation();
                        }
                    }
                }
                _ => {
                    // Signal handlers failed - continue without graceful shutdown support
                    // This is not critical for CLI operation
                }
            }
        });
    }

    #[cfg(windows)]
    {
        tokio::spawn(async {
            if tokio::signal::ctrl_c().await.is_ok() {
                request_cancellation();
            }
        });
    }

    cts
}

/// Check cancellation and return error if cancelled
pub fn check_cancellation() -> crate::error::Result<()> {
    if is_cancelled() {
        Err(crate::error::Error::Cancelled)
    } else {
        Ok(())
    }
}

/// Run an operation with cancellation support
/// Returns Err(Error::Cancelled) if cancelled before completion
pub async fn with_cancellation<F, T>(operation: F) -> crate::error::Result<T>
where
    F: std::future::Future<Output = crate::error::Result<T>>,
{
    tokio::select! {
        biased;
        _ = wait_for_cancellation() => {
            Err(crate::error::Error::Cancelled)
        }
        result = operation => {
            result
        }
    }
}

/// Wait for cancellation signal (no polling - uses watch channel)
async fn wait_for_cancellation() {
    // Fast path: already cancelled
    if is_cancelled() {
        return;
    }

    // Get a receiver clone for this wait
    let (_, receiver_lock) = get_cancellation_channel();
    let mut receiver = receiver_lock
        .lock()
        .map(|r| r.clone())
        .unwrap_or_else(|_| watch::channel(false).1);

    // Wait for the channel to signal cancellation
    loop {
        if *receiver.borrow() || is_cancelled() {
            return;
        }
        if receiver.changed().await.is_err() {
            // Channel closed, fall back to flag check
            if is_cancelled() {
                return;
            }
            // Channel closed without cancellation - wait indefinitely
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    // Tests that use global state must run serially to avoid race conditions.
    // The #[serial] attribute ensures these tests don't run in parallel.

    #[test]
    #[serial]
    fn test_cancellation_flag() {
        reset_cancellation();
        assert!(!is_cancelled());

        request_cancellation();
        assert!(is_cancelled());

        reset_cancellation();
        assert!(!is_cancelled());
    }

    #[test]
    fn test_cancellation_token_source_isolated() {
        // This test uses only local token state - fully isolated, no #[serial] needed
        let cts = CancellationTokenSource::new();
        let token = cts.token();

        assert!(!token.is_cancelled());

        cts.cancel(); // Only affects local token, not global state
        assert!(token.is_cancelled());
    }

    #[test]
    #[serial]
    fn test_cancellation_token_global() {
        reset_cancellation();
        let cts = CancellationTokenSource::new();
        let token = cts.token();

        assert!(!token.is_cancelled_global());

        // Cancel local only
        cts.cancel();
        assert!(token.is_cancelled());
        // is_cancelled_global returns true because local is cancelled
        assert!(token.is_cancelled_global());

        reset_cancellation();
    }

    #[tokio::test]
    #[serial]
    async fn test_check_cancellation() {
        reset_cancellation();
        assert!(check_cancellation().is_ok());

        request_cancellation();
        assert!(check_cancellation().is_err());

        reset_cancellation();
    }

    #[test]
    #[serial]
    fn test_cancel_global() {
        reset_cancellation();
        let cts = CancellationTokenSource::new();

        assert!(!is_cancelled());
        cts.cancel_global();
        assert!(is_cancelled());
        assert!(cts.token().is_cancelled());

        reset_cancellation();
    }
}

//! Cancellation handling for long-running operations
//!
//! Provides a global cancellation flag that can be set by signal handlers
//! and checked by operations to enable clean shutdown.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::sync::watch;

/// Global cancellation flag
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Global cleanup handlers
static CLEANUP_HANDLERS: OnceLock<Mutex<Vec<Box<dyn Fn() + Send + Sync>>>> = OnceLock::new();



/// Check if cancellation was requested
pub fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

/// Request cancellation and run cleanup handlers
pub fn request_cancellation() {
    CANCELLED.store(true, Ordering::SeqCst);
    
    // Run all cleanup handlers
    if let Some(handlers) = CLEANUP_HANDLERS.get()
        && let Ok(mut handlers_lock) = handlers.lock() {
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
        && let Ok(mut handlers_lock) = handlers.lock() {
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
}

/// Cancellation token for async operations
#[derive(Clone)]
pub struct CancellationToken {
    receiver: watch::Receiver<bool>,
}

impl CancellationToken {
    /// Check if cancelled
    pub fn is_cancelled(&self) -> bool {
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
    pub fn cancel(&self) {
        let _ = self.sender.send(true);
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
            let mut sigterm = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::terminate()
            ).expect("Failed to setup SIGTERM handler");

            let mut sigint = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::interrupt()
            ).expect("Failed to setup SIGINT handler");

            tokio::select! {
                _ = sigterm.recv() => {
                    request_cancellation();
                }
                _ = sigint.recv() => {
                    request_cancellation();
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

/// Wait for cancellation signal
async fn wait_for_cancellation() {
    loop {
        if is_cancelled() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cancellation_flag() {
        reset_cancellation();
        assert!(!is_cancelled());

        request_cancellation();
        assert!(is_cancelled());

        reset_cancellation();
        assert!(!is_cancelled());
    }

    #[test]
    fn test_cancellation_token_source() {
        let cts = CancellationTokenSource::new();
        let token = cts.token();

        assert!(!token.is_cancelled());

        cts.cancel();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn test_check_cancellation() {
        reset_cancellation();
        assert!(check_cancellation().is_ok());

        request_cancellation();
        assert!(check_cancellation().is_err());

        reset_cancellation();
    }
}
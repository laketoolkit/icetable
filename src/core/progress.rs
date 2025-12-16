//! Progress reporting abstraction for core services
//!
//! This module provides an abstract progress reporting interface that allows
//! core services to report progress without depending on CLI utilities.
//!
//! # Design
//!
//! Core services accept an optional `ProgressReporter` which they use to
//! report progress events. The CLI layer provides concrete implementations
//! that display progress bars, spinners, etc.
//!
//! # Example
//!
//! ```ignore
//! // In core service
//! pub struct OptimizeService {
//!     progress: Option<Box<dyn ProgressReporter>>,
//! }
//!
//! impl OptimizeService {
//!     pub fn with_progress(mut self, reporter: Box<dyn ProgressReporter>) -> Self {
//!         self.progress = Some(reporter);
//!         self
//!     }
//!
//!     async fn execute(&self) {
//!         if let Some(p) = &self.progress {
//!             p.set_total(100);
//!             for i in 0..100 {
//!                 p.inc(1);
//!             }
//!             p.finish();
//!         }
//!     }
//! }
//!
//! // In CLI
//! let reporter = IndicatifReporter::progress_bar(100, "Processing");
//! let service = OptimizeService::new().with_progress(Box::new(reporter));
//! ```

use std::sync::Arc;

/// Progress reporter trait for core services
///
/// Implementations of this trait provide visual feedback for long-running operations.
/// Core services use this abstraction to report progress without depending on
/// specific UI libraries like indicatif.
pub trait ProgressReporter: Send + Sync {
    /// Set the total number of items to process
    fn set_total(&self, total: u64);

    /// Set the current progress message
    fn set_message(&self, message: &str);

    /// Increment progress by delta
    fn inc(&self, delta: u64);

    /// Set absolute progress position
    fn set_position(&self, position: u64);

    /// Mark the operation as complete
    fn finish(&self);

    /// Mark as complete with a final message
    fn finish_with_message(&self, message: &str);
}

/// A no-op progress reporter for when progress reporting is disabled
pub struct NoopReporter;

impl ProgressReporter for NoopReporter {
    fn set_total(&self, _total: u64) {}
    fn set_message(&self, _message: &str) {}
    fn inc(&self, _delta: u64) {}
    fn set_position(&self, _position: u64) {}
    fn finish(&self) {}
    fn finish_with_message(&self, _message: &str) {}
}

impl NoopReporter {
    /// Create a new no-op reporter
    pub fn new() -> Self {
        Self
    }

    /// Create as a boxed trait object
    pub fn boxed() -> Box<dyn ProgressReporter> {
        Box::new(Self)
    }
}

impl Default for NoopReporter {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for optional progress reporter
pub type OptionalProgress = Option<Arc<dyn ProgressReporter>>;

/// Helper to report progress if a reporter is available
pub fn report_progress(reporter: &OptionalProgress, delta: u64) {
    if let Some(r) = reporter {
        r.inc(delta);
    }
}

/// Helper to set progress message if a reporter is available
pub fn report_message(reporter: &OptionalProgress, message: &str) {
    if let Some(r) = reporter {
        r.set_message(message);
    }
}

/// Helper to finish progress if a reporter is available
pub fn finish_progress(reporter: &OptionalProgress) {
    if let Some(r) = reporter {
        r.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_reporter() {
        let reporter = NoopReporter::new();
        reporter.set_total(100);
        reporter.set_message("test");
        reporter.inc(1);
        reporter.set_position(50);
        reporter.finish();
        reporter.finish_with_message("done");
        // No panic = success
    }

    #[test]
    fn test_helper_functions_with_none() {
        let reporter: OptionalProgress = None;
        report_progress(&reporter, 1);
        report_message(&reporter, "test");
        finish_progress(&reporter);
        // No panic = success
    }
}

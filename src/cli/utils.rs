//! CLI utilities
//!
//! Re-exports utilities commonly used by CLI commands.
//! This module provides a cleaner import path for CLI code.
//!
//! # Usage in CLI commands
//!
//! Instead of:
//! ```ignore
//! use crate::utils::{create_spinner, create_progress_bar};
//! ```
//!
//! Prefer:
//! ```ignore
//! use crate::cli::utils::{create_spinner, create_progress_bar};
//! ```
//!
//! # Progress Reporting
//!
//! Core services use the abstract `ProgressReporter` trait from `core::progress`.
//! This module provides `IndicatifReporter` which implements that trait using
//! the indicatif library for terminal progress bars.
//!
//! ```ignore
//! use crate::cli::utils::IndicatifReporter;
//! use crate::core::progress::ProgressReporter;
//!
//! let reporter = IndicatifReporter::progress_bar(100, "Processing");
//! reporter.inc(1);
//! reporter.finish();
//! ```

use crate::core::progress::ProgressReporter;
use indicatif::ProgressBar;
use std::sync::Arc;

// Progress indicators (direct indicatif usage - prefer IndicatifReporter for core services)
pub use crate::utils::{ProgressTracker, create_progress_bar, create_spinner};

/// Adapter that implements `ProgressReporter` using indicatif
///
/// Use this to pass progress reporting to core services.
pub struct IndicatifReporter {
    bar: ProgressBar,
}

impl IndicatifReporter {
    /// Create a new reporter wrapping an indicatif ProgressBar
    pub fn new(bar: ProgressBar) -> Self {
        Self { bar }
    }

    /// Create a spinner reporter for indeterminate operations
    pub fn spinner(message: &str) -> Self {
        Self::new(create_spinner(message))
    }

    /// Create a progress bar reporter for operations with known total
    pub fn progress_bar(total: u64, action: &str) -> Self {
        Self::new(create_progress_bar(total, action))
    }

    /// Create as a boxed trait object
    pub fn boxed(self) -> Box<dyn ProgressReporter> {
        Box::new(self)
    }

    /// Create as an Arc for shared ownership
    pub fn arc(self) -> Arc<dyn ProgressReporter> {
        Arc::new(self)
    }
}

impl ProgressReporter for IndicatifReporter {
    fn set_total(&self, total: u64) {
        self.bar.set_length(total);
    }

    fn set_message(&self, message: &str) {
        self.bar.set_message(message.to_string());
    }

    fn inc(&self, delta: u64) {
        self.bar.inc(delta);
    }

    fn set_position(&self, position: u64) {
        self.bar.set_position(position);
    }

    fn finish(&self) {
        self.bar.finish_and_clear();
    }

    fn finish_with_message(&self, message: &str) {
        self.bar.finish_with_message(message.to_string());
    }
}

// Cancellation handling
pub use crate::utils::{
    CancellationToken, CancellationTokenSource, check_cancellation, is_cancelled,
    register_cleanup_handler, request_cancellation, setup_signal_handlers, temp_dir_with_cleanup,
    with_cancellation,
};

// Resource limits
pub use crate::utils::{
    ResourceLimits, current_memory_usage, get_resource_limits, init_resource_limits,
    release_memory, track_memory_usage, with_resource_limits, with_timeout,
};

// Logging
pub use crate::utils::{LogLevel, init_logger};

// Text utilities
pub use crate::utils::{strip_ansi_codes, visual_width, wrap_line};

// Time parsing
pub use crate::utils::{parse_relative_duration, parse_timestamp};

// Type parsing
pub use crate::utils::parse_data_type;

// Box frame for formatted output
pub use crate::utils::create_box_frame;

// Telemetry
pub use crate::utils::TelemetryCollector;

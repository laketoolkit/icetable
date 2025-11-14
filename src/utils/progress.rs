//! Progress tracking for long-running operations

use indicatif::{ProgressBar, ProgressStyle};

/// Progress tracker for operations
pub struct ProgressTracker {
    bar: Option<ProgressBar>,
}

impl ProgressTracker {
    /// Create a new progress tracker
    pub fn new(total: u64) -> Self {
        let bar = ProgressBar::new(total);
        bar.set_style(
            ProgressStyle::default_bar()
                .template("[{bar:40}] {percent}% ({eta})")
                .expect("Invalid progress bar template"),
        );

        Self { bar: Some(bar) }
    }

    /// Create a spinner for indeterminate progress
    pub fn spinner(message: &str) -> Self {
        let bar = ProgressBar::new_spinner();
        bar.set_message(message.to_string());
        Self { bar: Some(bar) }
    }

    /// Update progress
    pub fn update(&self, current: u64) {
        if let Some(bar) = &self.bar {
            bar.set_position(current);
        }
    }

    /// Increment progress
    pub fn inc(&self, delta: u64) {
        if let Some(bar) = &self.bar {
            bar.inc(delta);
        }
    }

    /// Finish and clear progress bar
    pub fn finish(&self) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }

    /// Finish with message
    pub fn finish_with_message(&self, message: &str) {
        if let Some(bar) = &self.bar {
            bar.finish_with_message(message.to_string());
        }
    }
}

impl Drop for ProgressTracker {
    fn drop(&mut self) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }
}

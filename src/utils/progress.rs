//! Progress tracking for long-running operations

use indicatif::{ProgressBar, ProgressStyle};

/// Display mode for progress indication
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// Show animated spinner (for indeterminate operations)
    Spinner,
    /// Show progress bar with percentage (for operations with known total)
    Bar,
    /// Show progress bar with rows/sec throughput
    BarWithThroughput,
    /// Silent mode - no visual feedback
    Silent,
}

/// Progress tracker for operations
pub struct ProgressTracker {
    bar: Option<ProgressBar>,
}

impl ProgressTracker {
    /// Create a new progress tracker with specified display mode
    pub fn with_mode(mode: DisplayMode, message: &str, total: Option<u64>) -> Self {
        match mode {
            DisplayMode::Silent => Self { bar: None },
            DisplayMode::Spinner => {
                let bar = ProgressBar::new_spinner();
                bar.set_style(
                    ProgressStyle::default_spinner()
                        .template("{spinner:.cyan} {msg}")
                        .expect("Invalid spinner template")
                        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
                );
                bar.set_message(message.to_string());
                bar.enable_steady_tick(std::time::Duration::from_millis(80));
                Self { bar: Some(bar) }
            }
            DisplayMode::Bar => {
                let bar = ProgressBar::new(total.unwrap_or(0));
                bar.set_style(
                    ProgressStyle::default_bar()
                        .template("[{bar:40.cyan/blue}] {percent}% ({eta})")
                        .expect("Invalid progress bar template")
                        .progress_chars("█▓▒░ "),
                );
                bar.set_message(message.to_string());
                Self { bar: Some(bar) }
            }
            DisplayMode::BarWithThroughput => {
                let bar = ProgressBar::new(total.unwrap_or(0));
                bar.set_style(
                    ProgressStyle::default_bar()
                        .template("[{bar:40.cyan/blue}] {human_pos}/{human_len} ({per_sec}) {eta}")
                        .expect("Invalid progress bar template")
                        .progress_chars("█▓▒░ "),
                );
                bar.set_message(message.to_string());
                Self { bar: Some(bar) }
            }
        }
    }

    /// Create a new progress bar with known total
    pub fn new(total: u64) -> Self {
        Self::with_mode(DisplayMode::Bar, "", Some(total))
    }

    /// Create a spinner for indeterminate progress
    pub fn spinner(message: &str) -> Self {
        Self::with_mode(DisplayMode::Spinner, message, None)
    }

    /// Create a silent tracker (no output)
    pub fn silent() -> Self {
        Self::with_mode(DisplayMode::Silent, "", None)
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

    /// Finish and clear progress bar (alias for finish)
    pub fn finish_and_clear(&self) {
        self.finish();
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

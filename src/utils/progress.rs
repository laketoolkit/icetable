//! Progress tracking for long-running operations

use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

// =============================================================================
// Simple factory functions (preferred API)
// =============================================================================

/// Create a spinner for indeterminate operations
///
/// Use this when you don't know the total count upfront.
pub fn create_spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    if let Ok(style) = ProgressStyle::default_spinner()
        .template(&format!("{{spinner:.cyan}} {}...", message))
    {
        pb.set_style(style);
    }
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// Create a progress bar for operations with known total
///
/// Use this when you know how many items will be processed.
pub fn create_progress_bar(total: u64, action: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    if let Ok(style) = ProgressStyle::default_bar()
        .template(&format!(
            "{{spinner:.green}} {} {{bar:30.cyan/blue}} {{percent}}% {{msg}}",
            action
        ))
    {
        pb.set_style(style.progress_chars("━━╺"));
    }
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

// =============================================================================
// ProgressTracker (alternative API with more control)
// =============================================================================

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
                if let Ok(style) = ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                {
                    bar.set_style(
                        style.tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
                    );
                }
                bar.set_message(message.to_string());
                bar.enable_steady_tick(std::time::Duration::from_millis(80));
                Self { bar: Some(bar) }
            }
            DisplayMode::Bar => {
                let bar = ProgressBar::new(total.unwrap_or(0));
                if let Ok(style) = ProgressStyle::default_bar()
                    .template("[{bar:40.cyan/blue}] {percent}% ({eta})")
                {
                    bar.set_style(style.progress_chars("█▓▒░ "));
                }
                bar.set_message(message.to_string());
                Self { bar: Some(bar) }
            }
            DisplayMode::BarWithThroughput => {
                let bar = ProgressBar::new(total.unwrap_or(0));
                if let Ok(style) = ProgressStyle::default_bar()
                    .template("[{bar:40.cyan/blue}] {human_pos}/{human_len} ({per_sec}) {eta}")
                {
                    bar.set_style(style.progress_chars("█▓▒░ "));
                }
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

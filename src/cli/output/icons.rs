//! Icon definitions for output formatting

use colored::Colorize;

/// Status icons for overall status (valid/invalid/warning)
#[derive(Debug, Clone, Copy)]
pub enum StatusIcon {
    /// Success/Valid (green tick)
    Success,
    /// Warning (yellow warning sign)
    Warning,
    /// Error/Failed (red cross)
    Error,
}

impl StatusIcon {
    /// Get the colored icon as a string
    pub fn as_str(&self) -> String {
        match self {
            StatusIcon::Success => "✓".green().to_string(),
            StatusIcon::Warning => "⚠".yellow().to_string(),
            StatusIcon::Error => "✗".red().to_string(),
        }
    }

    /// Get just the icon without color
    pub fn icon(&self) -> &'static str {
        match self {
            StatusIcon::Success => "✓",
            StatusIcon::Warning => "⚠",
            StatusIcon::Error => "✗",
        }
    }
}

impl std::fmt::Display for StatusIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Severity icons for individual messages (same shape, different colors)
#[derive(Debug, Clone, Copy)]
pub enum SeverityIcon {
    /// Error message (red)
    Error,
    /// Warning message (yellow)
    Warning,
    /// Info message (cyan)
    Info,
}

impl SeverityIcon {
    /// Get the colored icon as a string
    pub fn as_str(&self) -> String {
        match self {
            SeverityIcon::Error => "🛈".red().to_string(),
            SeverityIcon::Warning => "🛈".yellow().to_string(),
            SeverityIcon::Info => "🛈".cyan().to_string(),
        }
    }

    /// Get just the icon without color
    pub fn icon(&self) -> &'static str {
        "🛈"
    }
}

impl std::fmt::Display for SeverityIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

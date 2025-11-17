//! Logging configuration and utilities

use std::str::FromStr;

/// Log level for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// No logging output
    Off,
    /// Only errors
    Error,
    /// Errors and warnings
    Warn,
    /// Errors, warnings, and info
    Info,
    /// Errors, warnings, info, and debug
    Debug,
    /// All logging including trace
    Trace,
}

impl LogLevel {
    /// Convert to env_logger LevelFilter
    pub fn to_level_filter(&self) -> log::LevelFilter {
        match self {
            LogLevel::Off => log::LevelFilter::Off,
            LogLevel::Error => log::LevelFilter::Error,
            LogLevel::Warn => log::LevelFilter::Warn,
            LogLevel::Info => log::LevelFilter::Info,
            LogLevel::Debug => log::LevelFilter::Debug,
            LogLevel::Trace => log::LevelFilter::Trace,
        }
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "off" | "none" => Ok(LogLevel::Off),
            "error" => Ok(LogLevel::Error),
            "warn" | "warning" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            "trace" => Ok(LogLevel::Trace),
            _ => Err(format!(
                "Invalid log level '{}'. Valid values: off, error, warn, info, debug, trace",
                s
            )),
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Off => write!(f, "off"),
            LogLevel::Error => write!(f, "error"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Trace => write!(f, "trace"),
        }
    }
}

/// Initialize the logger with the specified level
pub fn init_logger(log_level: LogLevel) {
    env_logger::Builder::from_default_env()
        .filter_level(log_level.to_level_filter())
        .format_timestamp_millis() // Show timestamps with millisecond precision
        .format_module_path(false)
        .init();

    if log_level != LogLevel::Off {
        log::debug!("Logging initialized at level: {}", log_level);
    }
}

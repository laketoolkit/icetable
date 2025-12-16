//! Validation rules definitions

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Collection of validation rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRules {
    /// List of validation rules
    pub rules: Vec<ValidationRule>,
}

/// A single validation rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    /// Rule name/description (Arc for cheap cloning in hot paths)
    #[serde(deserialize_with = "deserialize_arc_str", serialize_with = "serialize_arc_str")]
    pub name: Arc<str>,

    /// Rule type
    #[serde(rename = "type")]
    pub rule_type: RuleType,

    /// Severity level
    pub severity: Severity,

    /// Whether rule is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn deserialize_arc_str<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(Arc::from(s))
}

fn serialize_arc_str<S>(value: &Arc<str>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(value)
}

fn default_enabled() -> bool {
    true
}

/// Type of validation rule
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleType {
    /// Minimum number of rows
    MinRows {
        /// Minimum row count
        value: i64,
    },

    /// Maximum number of rows
    MaxRows {
        /// Maximum row count
        value: i64,
    },

    /// Required compression codec
    CompressionRequired {
        /// Allowed compression codecs
        allowed: Vec<String>,
    },

    /// Required columns must exist
    RequiredColumns {
        /// List of required column names
        columns: Vec<String>,
    },

    /// Maximum null percentage in a column
    MaxNullPercent {
        /// Column name (if None, applies to all columns)
        column: Option<String>,
        /// Maximum percentage of nulls (0-100)
        max_percent: f64,
    },

    /// Column must have specific data type
    ColumnType {
        /// Column name
        column: String,
        /// Expected Arrow data type as string
        expected_type: String,
    },

    /// File size constraints
    FileSize {
        /// Minimum size in bytes
        min_bytes: Option<u64>,
        /// Maximum size in bytes
        max_bytes: Option<u64>,
    },

    /// Row group size for Parquet
    RowGroupSize {
        /// Minimum row group size
        min_size: Option<usize>,
        /// Maximum row group size
        max_size: Option<usize>,
    },

    /// Column name pattern matching
    ColumnNamePattern {
        /// Regex pattern column names must match
        pattern: String,
    },

    /// Custom SQL-like expression
    CustomExpression {
        /// SQL expression to evaluate
        expression: String,
    },
}

/// Severity level for validation rules
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Error - validation fails
    Error,
    /// Warning - validation succeeds with warnings
    Warning,
    /// Info - informational message only
    Info,
}

/// Result of applying a validation rule
#[derive(Debug, Clone)]
pub struct RuleResult {
    /// The rule that was applied (Arc for cheap cloning)
    pub rule_name: Arc<str>,

    /// Whether the rule passed
    pub passed: bool,

    /// Severity of the result
    pub severity: Severity,

    /// Message describing the result
    pub message: String,

    /// Additional context/details
    pub details: Option<String>,
}

impl RuleResult {
    /// Create a passing result
    pub fn pass(rule_name: Arc<str>, severity: Severity, message: String) -> Self {
        Self {
            rule_name,
            passed: true,
            severity,
            message,
            details: None,
        }
    }

    /// Create a failing result
    pub fn fail(rule_name: Arc<str>, severity: Severity, message: String) -> Self {
        Self {
            rule_name,
            passed: false,
            severity,
            message,
            details: None,
        }
    }

    /// Add details to the result
    pub fn with_details(mut self, details: String) -> Self {
        self.details = Some(details);
        self
    }
}

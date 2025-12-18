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
    #[serde(
        deserialize_with = "deserialize_arc_str",
        serialize_with = "serialize_arc_str"
    )]
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

    /// Create a skipped result (rule not applicable or not yet implemented)
    pub fn skip(rule_name: Arc<str>, message: String) -> Self {
        Self {
            rule_name,
            passed: true, // Skipped rules don't fail validation
            severity: Severity::Info,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_result_pass() {
        let result = RuleResult::pass(
            Arc::from("test_rule"),
            Severity::Error,
            "All good".to_string(),
        );
        assert!(result.passed);
        assert_eq!(result.rule_name.as_ref(), "test_rule");
        assert_eq!(result.severity, Severity::Error);
    }

    #[test]
    fn test_rule_result_fail() {
        let result = RuleResult::fail(
            Arc::from("test_rule"),
            Severity::Warning,
            "Something failed".to_string(),
        );
        assert!(!result.passed);
        assert_eq!(result.severity, Severity::Warning);
    }

    #[test]
    fn test_rule_result_skip() {
        let result = RuleResult::skip(Arc::from("test_rule"), "Not applicable".to_string());
        assert!(result.passed); // Skipped rules don't fail
        assert_eq!(result.severity, Severity::Info);
    }

    #[test]
    fn test_rule_result_with_details() {
        let result = RuleResult::pass(Arc::from("test_rule"), Severity::Info, "Passed".to_string())
            .with_details("More info here".to_string());

        assert_eq!(result.details, Some("More info here".to_string()));
    }

    #[test]
    fn test_validation_rules_deserialize() {
        let yaml = r#"
rules:
  - name: "min_rows_check"
    type:
      kind: min_rows
      value: 1000
    severity: error
  - name: "compression_check"
    type:
      kind: compression_required
      allowed: ["snappy", "zstd"]
    severity: warning
"#;
        let rules: ValidationRules = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(rules.rules.len(), 2);
        assert_eq!(rules.rules[0].name.as_ref(), "min_rows_check");
        assert!(matches!(
            rules.rules[0].rule_type,
            RuleType::MinRows { value: 1000 }
        ));
        assert_eq!(rules.rules[0].severity, Severity::Error);
        assert!(rules.rules[0].enabled); // default
    }

    #[test]
    fn test_validation_rule_disabled() {
        let yaml = r#"
rules:
  - name: "disabled_rule"
    type:
      kind: min_rows
      value: 100
    severity: info
    enabled: false
"#;
        let rules: ValidationRules = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(!rules.rules[0].enabled);
    }

    #[test]
    fn test_required_columns_rule() {
        let yaml = r#"
rules:
  - name: "required_cols"
    type:
      kind: required_columns
      columns: ["id", "timestamp", "value"]
    severity: error
"#;
        let rules: ValidationRules = serde_yaml_ng::from_str(yaml).unwrap();
        if let RuleType::RequiredColumns { columns } = &rules.rules[0].rule_type {
            assert_eq!(columns, &vec!["id", "timestamp", "value"]);
        } else {
            panic!("Expected RequiredColumns rule type");
        }
    }

    #[test]
    fn test_column_type_rule() {
        let yaml = r#"
rules:
  - name: "type_check"
    type:
      kind: column_type
      column: "id"
      expected_type: "Int64"
    severity: error
"#;
        let rules: ValidationRules = serde_yaml_ng::from_str(yaml).unwrap();
        if let RuleType::ColumnType {
            column,
            expected_type,
        } = &rules.rules[0].rule_type
        {
            assert_eq!(column, "id");
            assert_eq!(expected_type, "Int64");
        } else {
            panic!("Expected ColumnType rule type");
        }
    }

    #[test]
    fn test_file_size_rule() {
        let yaml = r#"
rules:
  - name: "size_check"
    type:
      kind: file_size
      min_bytes: 1024
      max_bytes: 1073741824
    severity: warning
"#;
        let rules: ValidationRules = serde_yaml_ng::from_str(yaml).unwrap();
        if let RuleType::FileSize {
            min_bytes,
            max_bytes,
        } = &rules.rules[0].rule_type
        {
            assert_eq!(*min_bytes, Some(1024));
            assert_eq!(*max_bytes, Some(1073741824));
        } else {
            panic!("Expected FileSize rule type");
        }
    }

    #[test]
    fn test_column_name_pattern_rule() {
        let yaml = r#"
rules:
  - name: "naming_convention"
    type:
      kind: column_name_pattern
      pattern: "^[a-z][a-z0-9_]*$"
    severity: warning
"#;
        let rules: ValidationRules = serde_yaml_ng::from_str(yaml).unwrap();
        if let RuleType::ColumnNamePattern { pattern } = &rules.rules[0].rule_type {
            assert_eq!(pattern, "^[a-z][a-z0-9_]*$");
        } else {
            panic!("Expected ColumnNamePattern rule type");
        }
    }

    #[test]
    fn test_severity_deserialize() {
        assert_eq!(
            serde_yaml_ng::from_str::<Severity>("error").unwrap(),
            Severity::Error
        );
        assert_eq!(
            serde_yaml_ng::from_str::<Severity>("warning").unwrap(),
            Severity::Warning
        );
        assert_eq!(
            serde_yaml_ng::from_str::<Severity>("info").unwrap(),
            Severity::Info
        );
    }
}

//! Validation engine for executing rules

use std::sync::Arc;

use arrow::array::Array;
use arrow::datatypes::{DataType, Schema};
use regex::Regex;

use super::rules::{RuleResult, RuleType, Severity, ValidationRule, ValidationRules};
use crate::core::formats::{FileMetadata, FormatHandler, ReadOptions};
use crate::error::{Error, Result};

/// Validation engine that executes rules against data
pub struct ValidationEngine {
    handler: Arc<dyn FormatHandler>,
    rules: ValidationRules,
}

impl ValidationEngine {
    /// Create a new validation engine
    pub fn new(handler: Arc<dyn FormatHandler>, rules: ValidationRules) -> Self {
        Self { handler, rules }
    }

    /// Load rules from a YAML file
    pub async fn load_rules(path: &str) -> Result<ValidationRules> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| Error::General(format!("Failed to read rules file: {}", e)))?;

        serde_yaml::from_str(&content)
            .map_err(|e| Error::General(format!("Failed to parse rules file: {}", e)))
    }

    /// Execute all enabled rules
    pub async fn execute(&self) -> Result<Vec<RuleResult>> {
        let mut results = Vec::new();

        // Get metadata once for efficiency
        let metadata = self.handler.read_metadata().await?;
        let schema = self.handler.read_schema().await?;

        for rule in &self.rules.rules {
            if !rule.enabled {
                continue;
            }

            let result = self
                .execute_rule(rule, &metadata, &schema)
                .await
                .unwrap_or_else(|e| {
                    RuleResult::fail(
                        rule.name.clone(),
                        rule.severity,
                        format!("Rule execution failed: {}", e),
                    )
                });

            results.push(result);
        }

        Ok(results)
    }

    /// Execute a single rule
    async fn execute_rule(
        &self,
        rule: &ValidationRule,
        metadata: &FileMetadata,
        schema: &Schema,
    ) -> Result<RuleResult> {
        match &rule.rule_type {
            RuleType::MinRows { value } => self.check_min_rows(rule, metadata, *value),

            RuleType::MaxRows { value } => self.check_max_rows(rule, metadata, *value),

            RuleType::CompressionRequired { allowed } => {
                self.check_compression(rule, metadata, allowed)
            }

            RuleType::RequiredColumns { columns } => {
                self.check_required_columns(rule, schema, columns)
            }

            RuleType::MaxNullPercent {
                column,
                max_percent,
            } => self.check_null_percent(rule, column, *max_percent).await,

            RuleType::ColumnType {
                column,
                expected_type,
            } => self.check_column_type(rule, schema, column, expected_type),

            RuleType::FileSize {
                min_bytes,
                max_bytes,
            } => self.check_file_size(rule, metadata, *min_bytes, *max_bytes),

            RuleType::RowGroupSize { min_size, max_size } => {
                self.check_row_group_size(rule, metadata, *min_size, *max_size)
            }

            RuleType::ColumnNamePattern { pattern } => {
                self.check_column_name_pattern(rule, schema, pattern)
            }

            RuleType::CustomExpression { expression } => {
                self.check_custom_expression(rule, expression).await
            }
        }
    }

    fn check_min_rows(
        &self,
        rule: &ValidationRule,
        metadata: &FileMetadata,
        min_rows: i64,
    ) -> Result<RuleResult> {
        match metadata.num_rows {
            Some(rows) if rows >= min_rows => Ok(RuleResult::pass(
                rule.name.clone(),
                rule.severity,
                format!("File has {} rows (>= {} required)", rows, min_rows),
            )),
            Some(rows) => Ok(RuleResult::fail(
                rule.name.clone(),
                rule.severity,
                format!("File has {} rows (< {} required)", rows, min_rows),
            )),
            None => Ok(RuleResult::fail(
                rule.name.clone(),
                rule.severity,
                "Unable to determine row count".to_string(),
            )),
        }
    }

    fn check_max_rows(
        &self,
        rule: &ValidationRule,
        metadata: &FileMetadata,
        max_rows: i64,
    ) -> Result<RuleResult> {
        match metadata.num_rows {
            Some(rows) if rows <= max_rows => Ok(RuleResult::pass(
                rule.name.clone(),
                rule.severity,
                format!("File has {} rows (<= {} allowed)", rows, max_rows),
            )),
            Some(rows) => Ok(RuleResult::fail(
                rule.name.clone(),
                rule.severity,
                format!("File has {} rows (> {} allowed)", rows, max_rows),
            )),
            None => Ok(RuleResult::fail(
                rule.name.clone(),
                rule.severity,
                "Unable to determine row count".to_string(),
            )),
        }
    }

    fn check_compression(
        &self,
        rule: &ValidationRule,
        metadata: &FileMetadata,
        allowed: &[String],
    ) -> Result<RuleResult> {
        match &metadata.compression {
            Some(compression) if allowed.iter().any(|a| a.eq_ignore_ascii_case(compression)) => {
                Ok(RuleResult::pass(
                    rule.name.clone(),
                    rule.severity,
                    format!("Using allowed compression: {}", compression),
                ))
            }
            Some(compression) => Ok(RuleResult::fail(
                rule.name.clone(),
                rule.severity,
                format!(
                    "Using {} compression (allowed: {})",
                    compression,
                    allowed.join(", ")
                ),
            )),
            None => Ok(RuleResult::fail(
                rule.name.clone(),
                rule.severity,
                "No compression detected".to_string(),
            )),
        }
    }

    fn check_required_columns(
        &self,
        rule: &ValidationRule,
        schema: &Schema,
        required: &[String],
    ) -> Result<RuleResult> {
        let column_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        let missing: Vec<&String> = required
            .iter()
            .filter(|col| !column_names.contains(&col.as_str()))
            .collect();

        if missing.is_empty() {
            Ok(RuleResult::pass(
                rule.name.clone(),
                rule.severity,
                format!("All {} required columns present", required.len()),
            ))
        } else {
            Ok(RuleResult::fail(
                rule.name.clone(),
                rule.severity,
                format!(
                    "Missing required columns: {}",
                    missing
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ))
        }
    }

    async fn check_null_percent(
        &self,
        rule: &ValidationRule,
        column: &Option<String>,
        max_percent: f64,
    ) -> Result<RuleResult> {
        let batches = self.handler.read_batches(&ReadOptions::default()).await?;

        if let Some(col_name) = column {
            // Check specific column
            let schema = self.handler.read_schema().await?;
            let col_idx = schema
                .index_of(col_name)
                .map_err(|_| Error::General(format!("Column '{}' not found", col_name)))?;

            let mut total_rows = 0usize;
            let mut null_count = 0usize;

            for batch in &batches {
                let array = batch.column(col_idx);
                total_rows += array.len();
                null_count += array.null_count();
            }

            let null_percent = (null_count as f64 / total_rows as f64) * 100.0;

            if null_percent <= max_percent {
                Ok(RuleResult::pass(
                    rule.name.clone(),
                    rule.severity,
                    format!(
                        "Column '{}' has {:.2}% nulls (<= {:.2}%)",
                        col_name, null_percent, max_percent
                    ),
                ))
            } else {
                Ok(RuleResult::fail(
                    rule.name.clone(),
                    rule.severity,
                    format!(
                        "Column '{}' has {:.2}% nulls (> {:.2}%)",
                        col_name, null_percent, max_percent
                    ),
                ))
            }
        } else {
            // Check all columns
            let schema = self.handler.read_schema().await?;
            let mut violations = Vec::new();

            for (idx, field) in schema.fields().iter().enumerate() {
                let mut total_rows = 0usize;
                let mut null_count = 0usize;

                for batch in &batches {
                    let array = batch.column(idx);
                    total_rows += array.len();
                    null_count += array.null_count();
                }

                let null_percent = (null_count as f64 / total_rows as f64) * 100.0;

                if null_percent > max_percent {
                    violations.push(format!("{} ({:.2}%)", field.name(), null_percent));
                }
            }

            if violations.is_empty() {
                Ok(RuleResult::pass(
                    rule.name.clone(),
                    rule.severity,
                    format!("All columns have <= {:.2}% nulls", max_percent),
                ))
            } else {
                Ok(RuleResult::fail(
                    rule.name.clone(),
                    rule.severity,
                    format!(
                        "Columns with > {:.2}% nulls: {}",
                        max_percent,
                        violations.join(", ")
                    ),
                ))
            }
        }
    }

    fn check_column_type(
        &self,
        rule: &ValidationRule,
        schema: &Schema,
        column: &str,
        expected: &str,
    ) -> Result<RuleResult> {
        let field = schema
            .field_with_name(column)
            .map_err(|_| Error::General(format!("Column '{}' not found", column)))?;

        let actual_type = format!("{:?}", field.data_type());

        if actual_type.contains(expected) || expected.eq_ignore_ascii_case(&actual_type) {
            Ok(RuleResult::pass(
                rule.name.clone(),
                rule.severity,
                format!("Column '{}' has expected type: {}", column, actual_type),
            ))
        } else {
            Ok(RuleResult::fail(
                rule.name.clone(),
                rule.severity,
                format!(
                    "Column '{}' has type {} (expected: {})",
                    column, actual_type, expected
                ),
            ))
        }
    }

    fn check_file_size(
        &self,
        rule: &ValidationRule,
        metadata: &FileMetadata,
        min_bytes: Option<u64>,
        max_bytes: Option<u64>,
    ) -> Result<RuleResult> {
        let size = metadata
            .compressed_size
            .ok_or_else(|| Error::General("File size not available".to_string()))?;

        if let Some(min) = min_bytes {
            if size < min {
                return Ok(RuleResult::fail(
                    rule.name.clone(),
                    rule.severity,
                    format!("File size {} bytes (< {} minimum)", size, min),
                ));
            }
        }

        if let Some(max) = max_bytes {
            if size > max {
                return Ok(RuleResult::fail(
                    rule.name.clone(),
                    rule.severity,
                    format!("File size {} bytes (> {} maximum)", size, max),
                ));
            }
        }

        Ok(RuleResult::pass(
            rule.name.clone(),
            rule.severity,
            format!("File size {} bytes is within limits", size),
        ))
    }

    fn check_row_group_size(
        &self,
        rule: &ValidationRule,
        metadata: &FileMetadata,
        min_size: Option<usize>,
        max_size: Option<usize>,
    ) -> Result<RuleResult> {
        // Try to get row_groups from metadata
        let row_groups = metadata
            .metadata
            .get("row_groups")
            .and_then(|s| s.parse::<usize>().ok());

        match row_groups {
            Some(rg) => {
                if let Some(min) = min_size {
                    if rg < min {
                        return Ok(RuleResult::fail(
                            rule.name.clone(),
                            rule.severity,
                            format!("Has {} row groups (< {} minimum)", rg, min),
                        ));
                    }
                }

                if let Some(max) = max_size {
                    if rg > max {
                        return Ok(RuleResult::fail(
                            rule.name.clone(),
                            rule.severity,
                            format!("Has {} row groups (> {} maximum)", rg, max),
                        ));
                    }
                }

                Ok(RuleResult::pass(
                    rule.name.clone(),
                    rule.severity,
                    format!("Has {} row groups within limits", rg),
                ))
            }
            None => Ok(RuleResult::pass(
                rule.name.clone(),
                Severity::Info,
                "Row group information not available".to_string(),
            )),
        }
    }

    fn check_column_name_pattern(
        &self,
        rule: &ValidationRule,
        schema: &Schema,
        pattern: &str,
    ) -> Result<RuleResult> {
        let regex = Regex::new(pattern)
            .map_err(|e| Error::General(format!("Invalid regex pattern: {}", e)))?;

        let invalid_columns: Vec<&str> = schema
            .fields()
            .iter()
            .filter(|f| !regex.is_match(f.name()))
            .map(|f| f.name().as_str())
            .collect();

        if invalid_columns.is_empty() {
            Ok(RuleResult::pass(
                rule.name.clone(),
                rule.severity,
                format!("All column names match pattern: {}", pattern),
            ))
        } else {
            Ok(RuleResult::fail(
                rule.name.clone(),
                rule.severity,
                format!(
                    "Columns not matching pattern '{}': {}",
                    pattern,
                    invalid_columns.join(", ")
                ),
            ))
        }
    }

    async fn check_custom_expression(
        &self,
        rule: &ValidationRule,
        _expression: &str,
    ) -> Result<RuleResult> {
        // Custom expressions would require DataFusion or similar SQL engine
        // For now, return info that this is not yet implemented
        Ok(RuleResult::pass(
            rule.name.clone(),
            Severity::Info,
            "Custom expression validation not yet implemented".to_string(),
        ))
    }
}

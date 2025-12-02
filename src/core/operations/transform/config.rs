//! Configuration for data transformations

use arrow::datatypes::DataType;
use std::collections::HashMap;

/// Configuration for data transformations
#[derive(Debug, Clone, Default)]
pub struct TransformConfig {
    /// Columns to select (None = all columns)
    pub select_columns: Option<Vec<String>>,

    /// Column rename mappings (old_name -> new_name)
    pub rename_columns: HashMap<String, String>,

    /// Column type casts (column_name -> target_type)
    pub cast_columns: HashMap<String, DataType>,

    /// Filter expression (simple SQL-like WHERE clause)
    pub filter_expression: Option<String>,
}

impl TransformConfig {
    /// Create a new empty transform config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set columns to select
    pub fn with_columns(mut self, columns: Vec<String>) -> Self {
        self.select_columns = Some(columns);
        self
    }

    /// Add a column rename
    pub fn with_rename(mut self, old_name: String, new_name: String) -> Self {
        self.rename_columns.insert(old_name, new_name);
        self
    }

    /// Add a column cast
    pub fn with_cast(mut self, column: String, data_type: DataType) -> Self {
        self.cast_columns.insert(column, data_type);
        self
    }

    /// Set filter expression
    pub fn with_filter(mut self, expression: String) -> Self {
        self.filter_expression = Some(expression);
        self
    }

    /// Check if any transformations are configured
    pub fn has_transforms(&self) -> bool {
        self.select_columns.is_some()
            || !self.rename_columns.is_empty()
            || !self.cast_columns.is_empty()
            || self.filter_expression.is_some()
    }
}

//! Transformation pipeline for composable data transformations
//!
//! Provides a flexible pipeline system where transformations can be added,
//! reordered, and custom steps can be injected.

use arrow::record_batch::RecordBatch;
use std::collections::HashMap;

use crate::error::Result;

/// A single transformation step that can be applied to a batch
///
/// Implement this trait to create custom transformations that can be
/// integrated into the transformation pipeline.
///
/// # Example
///
/// ```ignore
/// struct UppercaseStep {
///     columns: Vec<String>,
/// }
///
/// impl TransformStep for UppercaseStep {
///     fn apply(&self, batch: RecordBatch) -> Result<RecordBatch> {
///         // Transform specified string columns to uppercase
///         // ...
///         Ok(batch)
///     }
///
///     fn name(&self) -> &str {
///         "uppercase"
///     }
/// }
/// ```
pub trait TransformStep: Send + Sync {
    /// Apply this transformation to a record batch
    fn apply(&self, batch: RecordBatch) -> Result<RecordBatch>;

    /// Name of this transformation (for debugging/logging)
    fn name(&self) -> &str;

    /// Whether this step can be skipped if batch is empty
    ///
    /// Most transformations can skip empty batches, but some (like
    /// aggregations) may need to process them.
    fn skip_empty(&self) -> bool {
        true
    }
}

/// Builder for constructing transformation pipelines
///
/// Transformations are applied in the order they are added. The pipeline
/// can be executed multiple times with different batches.
///
/// # Example
///
/// ```ignore
/// let pipeline = TransformPipeline::new()
///     .add_step(FilterStep::new("age > 18"))
///     .add_step(ProjectStep::new(vec!["name", "email"]))
///     .add_step(RenameStep::new(HashMap::from([
///         ("name".to_string(), "full_name".to_string())
///     ])));
///
/// let result = pipeline.apply(batch)?;
/// ```
pub struct TransformPipeline {
    steps: Vec<Box<dyn TransformStep>>,
}

impl TransformPipeline {
    /// Create a new empty pipeline
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// Add a transformation step to the end of the pipeline
    pub fn add_step<T: TransformStep + 'static>(mut self, step: T) -> Self {
        self.steps.push(Box::new(step));
        self
    }

    /// Apply all transformations in order
    ///
    /// Each step receives the output from the previous step. If any step
    /// fails, the pipeline stops and returns the error.
    pub fn apply(&self, mut batch: RecordBatch) -> Result<RecordBatch> {
        for step in &self.steps {
            if batch.num_rows() == 0 && step.skip_empty() {
                continue;
            }
            batch = step.apply(batch)?;
        }
        Ok(batch)
    }

    /// Get step names (for debugging/logging)
    pub fn step_names(&self) -> Vec<&str> {
        self.steps.iter().map(|s| s.name()).collect()
    }

    /// Get number of steps in pipeline
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Check if pipeline is empty
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

impl Default for TransformPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// Built-in transformation steps

/// Filter rows based on an expression
pub struct FilterStep {
    expression: String,
}

impl FilterStep {
    /// Creates a new filter step with the given expression
    pub fn new(expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
        }
    }
}

impl TransformStep for FilterStep {
    fn apply(&self, batch: RecordBatch) -> Result<RecordBatch> {
        super::filter::apply_filter(batch, &self.expression)
    }

    fn name(&self) -> &str {
        "filter"
    }
}

/// Project (select) specific columns
pub struct ProjectStep {
    columns: Vec<String>,
}

impl ProjectStep {
    /// Creates a new project step with the specified columns to select
    pub fn new(columns: Vec<String>) -> Self {
        Self { columns }
    }
}

impl TransformStep for ProjectStep {
    fn apply(&self, batch: RecordBatch) -> Result<RecordBatch> {
        super::project::apply_projection(batch, &self.columns)
    }

    fn name(&self) -> &str {
        "project"
    }
}

/// Rename columns
pub struct RenameStep {
    renames: HashMap<String, String>,
}

impl RenameStep {
    /// Creates a new rename step with the mapping of old to new column names
    pub fn new(renames: HashMap<String, String>) -> Self {
        Self { renames }
    }
}

impl TransformStep for RenameStep {
    fn apply(&self, batch: RecordBatch) -> Result<RecordBatch> {
        super::rename::apply_renames(batch, &self.renames)
    }

    fn name(&self) -> &str {
        "rename"
    }
}

/// Cast column types
pub struct CastStep {
    casts: HashMap<String, arrow::datatypes::DataType>,
}

impl CastStep {
    /// Creates a new cast step with the mapping of column names to target data types
    pub fn new(casts: HashMap<String, arrow::datatypes::DataType>) -> Self {
        Self { casts }
    }
}

impl TransformStep for CastStep {
    fn apply(&self, batch: RecordBatch) -> Result<RecordBatch> {
        super::cast::apply_casts(batch, &self.casts)
    }

    fn name(&self) -> &str {
        "cast"
    }
}

/// Custom transformation step using a closure
///
/// Allows creating simple transformations without implementing the trait.
///
/// # Example
///
/// ```ignore
/// let custom = CustomTransformStep::new("deduplicate", |batch| {
///     // Custom deduplication logic
///     Ok(batch)
/// });
/// ```
pub struct CustomTransformStep<F>
where
    F: Fn(RecordBatch) -> Result<RecordBatch> + Send + Sync,
{
    name: String,
    transform_fn: F,
}

impl<F> CustomTransformStep<F>
where
    F: Fn(RecordBatch) -> Result<RecordBatch> + Send + Sync,
{
    /// Creates a new custom transformation step with the given name and function
    pub fn new(name: impl Into<String>, transform_fn: F) -> Self {
        Self {
            name: name.into(),
            transform_fn,
        }
    }
}

impl<F> TransformStep for CustomTransformStep<F>
where
    F: Fn(RecordBatch) -> Result<RecordBatch> + Send + Sync,
{
    fn apply(&self, batch: RecordBatch) -> Result<RecordBatch> {
        (self.transform_fn)(batch)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoOpStep;

    impl TransformStep for NoOpStep {
        fn apply(&self, batch: RecordBatch) -> Result<RecordBatch> {
            Ok(batch)
        }

        fn name(&self) -> &str {
            "noop"
        }
    }

    #[test]
    fn test_pipeline_creation() {
        let pipeline = TransformPipeline::new();
        assert_eq!(pipeline.len(), 0);
        assert!(pipeline.is_empty());
    }

    #[test]
    fn test_add_steps() {
        let pipeline = TransformPipeline::new()
            .add_step(NoOpStep)
            .add_step(NoOpStep)
            .add_step(NoOpStep);

        assert_eq!(pipeline.len(), 3);
        assert!(!pipeline.is_empty());

        let names = pipeline.step_names();
        assert_eq!(names, vec!["noop", "noop", "noop"]);
    }

    #[test]
    fn test_custom_transform_step() {
        let custom = CustomTransformStep::new("test", |batch| Ok(batch));
        assert_eq!(custom.name(), "test");
    }
}

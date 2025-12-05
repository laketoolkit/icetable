//! Utility functions for table formats (Delta Lake, Iceberg)
//!
//! This module provides shared utility functions for table formats that don't
//! fit the file-based BaseFormatHandler pattern. Table formats like Delta and
//! Iceberg manage collections of files and have different characteristics than
//! single-file formats like Parquet or CSV.
//!
//! # Functions
//!
//! - `merge_batches()` - Consolidate multiple RecordBatch into a single one
//! - `apply_batch_pagination()` - Apply offset/limit to a vector of batches
//! - `apply_column_projection()` - Project specific columns from a batch
//! - `calculate_basic_statistics()` - Compute basic stats from batches

use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::compute;
use arrow::datatypes::Schema;

use crate::core::formats::traits::ColumnStats;
use crate::error::{Error, Result};

/// Merge multiple RecordBatch into a single RecordBatch
///
/// This is a common operation when handlers need to return a single batch
/// from multiple underlying batches (e.g., reading from multiple files).
///
/// # Arguments
///
/// * `batches` - Vector of batches to merge. All batches must have the same schema.
/// * `schema` - Schema to use for empty result if batches is empty
///
/// # Returns
///
/// - Empty batch with provided schema if input is empty
/// - Original batch if only one batch provided
/// - Concatenated batch if multiple batches provided
///
/// # Example
///
/// ```ignore
/// let schema = Arc::new(Schema::new(vec![...]));
/// let batches = vec![batch1, batch2, batch3];
/// let merged = merge_batches(batches, schema)?;
/// ```
pub fn merge_batches(batches: Vec<RecordBatch>, schema: Arc<Schema>) -> Result<RecordBatch> {
    if batches.is_empty() {
        Ok(RecordBatch::new_empty(schema))
    } else if batches.len() == 1 {
        // SAFETY: We just checked that batches.len() == 1
        Ok(batches.into_iter().next().expect("batch exists"))
    } else {
        let batch_schema = batches[0].schema();
        compute::concat_batches(&batch_schema, &batches).map_err(Error::Arrow)
    }
}

/// Apply pagination (offset and limit) to a vector of RecordBatch
///
/// This function extracts a subset of rows from a vector of batches based on
/// offset and limit parameters. It handles slicing across batch boundaries.
///
/// # Arguments
///
/// * `batches` - Input batches to paginate
/// * `offset` - Number of rows to skip from the beginning
/// * `limit` - Maximum number of rows to return
///
/// # Returns
///
/// Vector of batches containing the requested rows. The total number of rows
/// in the result will be at most `limit`, and will skip the first `offset` rows.
///
/// # Example
///
/// ```ignore
/// // Skip first 100 rows, return next 50
/// let paginated = apply_batch_pagination(batches, 100, 50)?;
/// ```
pub fn apply_batch_pagination(
    batches: Vec<RecordBatch>,
    offset: usize,
    limit: usize,
) -> Result<Vec<RecordBatch>> {
    let mut result = Vec::new();
    let mut total_rows_processed = 0usize;
    let mut total_rows_collected = 0usize;

    for batch in batches {
        let batch_rows = batch.num_rows();

        // Skip batches before offset
        if total_rows_processed + batch_rows <= offset {
            total_rows_processed += batch_rows;
            continue;
        }

        // If we've collected enough rows, stop
        if total_rows_collected >= limit {
            break;
        }

        // Calculate which portion of this batch to include
        let skip_rows = offset.saturating_sub(total_rows_processed);

        let take_rows = (batch_rows - skip_rows).min(limit - total_rows_collected);

        if take_rows > 0 {
            let sliced_batch = batch.slice(skip_rows, take_rows);
            total_rows_collected += take_rows;
            result.push(sliced_batch);
        }

        total_rows_processed += batch_rows;
    }

    Ok(result)
}

/// Apply column projection to a RecordBatch
///
/// Filters the batch to only include the specified columns. If a requested
/// column doesn't exist, it is skipped (lenient behavior).
///
/// # Arguments
///
/// * `batch` - Input batch to project
/// * `columns` - Names of columns to keep
///
/// # Returns
///
/// New RecordBatch with only the requested columns. If no valid columns are
/// found, returns an empty batch with matching schema structure.
///
/// # Example
///
/// ```ignore
/// let columns = vec!["id".to_string(), "name".to_string()];
/// let projected = apply_column_projection(batch, &columns)?;
/// ```
pub fn apply_column_projection(batch: RecordBatch, columns: &[String]) -> Result<RecordBatch> {
    let schema = batch.schema();
    let mut indices = Vec::new();

    for col_name in columns {
        match schema.index_of(col_name) {
            Ok(idx) => indices.push(idx),
            Err(_) => {
                // Column not found - skip it (lenient behavior)
                continue;
            }
        }
    }

    if indices.is_empty() {
        // No valid columns found - return empty batch with schema
        return Ok(RecordBatch::new_empty(batch.schema()));
    }

    // Project columns
    let projected_columns: Vec<_> = indices
        .iter()
        .map(|&idx| batch.column(idx).clone())
        .collect();

    let projected_fields: Vec<_> = indices
        .iter()
        .map(|&idx| schema.field(idx).clone())
        .collect();

    let projected_schema = Arc::new(Schema::new(projected_fields));

    RecordBatch::try_new(projected_schema, projected_columns).map_err(Error::Arrow)
}

/// Calculate basic statistics from a vector of RecordBatch
///
/// Computes null counts for all columns across all provided batches.
/// This is a fallback for table formats that don't have native statistics
/// readily available (or when you want to verify native stats).
///
/// # Arguments
///
/// * `batches` - Batches to compute statistics from
///
/// # Returns
///
/// Vector of `ColumnStats` with null counts populated. Other statistics
/// (min, max, mean, etc.) are set to None.
///
/// # Example
///
/// ```ignore
/// let stats = calculate_basic_statistics(&batches)?;
/// for stat in stats {
///     println!("{}: {} nulls", stat.name, stat.null_count.unwrap_or(0));
/// }
/// ```
pub fn calculate_basic_statistics(batches: &[RecordBatch]) -> Result<Vec<ColumnStats>> {
    if batches.is_empty() {
        return Ok(Vec::new());
    }

    let schema = batches[0].schema();
    let mut stats = Vec::new();

    for (col_idx, field) in schema.fields().iter().enumerate() {
        let mut null_count = 0i64;

        for batch in batches {
            let column = batch.column(col_idx);
            null_count += column.null_count() as i64;
        }

        stats.push(ColumnStats {
            name: field.name().clone(),
            null_count: Some(null_count),
            distinct_count: None,
            min_value: None,
            max_value: None,
            mean: None,
            std_dev: None,
        });
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Int32Array, StringArray};
    use arrow::datatypes::{DataType, Field};

    fn create_test_schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, true),
        ]))
    }

    fn create_test_batch(ids: Vec<i32>, names: Vec<Option<&str>>) -> RecordBatch {
        let schema = create_test_schema();
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int32Array::from(ids)),
                Arc::new(StringArray::from(names)),
            ],
        )
        .unwrap()
    }

    #[test]
    fn test_merge_batches_empty() {
        let schema = create_test_schema();
        let result = merge_batches(vec![], schema.clone()).unwrap();
        assert_eq!(result.num_rows(), 0);
        assert_eq!(result.schema(), schema);
    }

    #[test]
    fn test_merge_batches_single() {
        let batch = create_test_batch(vec![1, 2, 3], vec![Some("a"), Some("b"), Some("c")]);
        let schema = batch.schema();
        let result = merge_batches(vec![batch.clone()], schema).unwrap();
        assert_eq!(result.num_rows(), 3);
        assert_eq!(result.num_columns(), 2);
    }

    #[test]
    fn test_merge_batches_multiple() {
        let batch1 = create_test_batch(vec![1, 2], vec![Some("a"), Some("b")]);
        let batch2 = create_test_batch(vec![3, 4], vec![Some("c"), Some("d")]);
        let schema = batch1.schema();
        let result = merge_batches(vec![batch1, batch2], schema).unwrap();
        assert_eq!(result.num_rows(), 4);
        assert_eq!(result.num_columns(), 2);
    }

    #[test]
    fn test_apply_batch_pagination_no_offset_no_limit() {
        let batch1 = create_test_batch(vec![1, 2], vec![Some("a"), Some("b")]);
        let batch2 = create_test_batch(vec![3, 4], vec![Some("c"), Some("d")]);
        let result = apply_batch_pagination(vec![batch1, batch2], 0, usize::MAX).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.iter().map(|b| b.num_rows()).sum::<usize>(), 4);
    }

    #[test]
    fn test_apply_batch_pagination_with_offset() {
        let batch1 = create_test_batch(vec![1, 2], vec![Some("a"), Some("b")]);
        let batch2 = create_test_batch(vec![3, 4], vec![Some("c"), Some("d")]);
        let result = apply_batch_pagination(vec![batch1, batch2], 2, usize::MAX).unwrap();
        // Should skip first batch (2 rows), return only batch2
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].num_rows(), 2);
    }

    #[test]
    fn test_apply_batch_pagination_with_limit() {
        let batch1 = create_test_batch(vec![1, 2], vec![Some("a"), Some("b")]);
        let batch2 = create_test_batch(vec![3, 4], vec![Some("c"), Some("d")]);
        let result = apply_batch_pagination(vec![batch1, batch2], 0, 3).unwrap();
        // Should return batch1 (2 rows) + partial batch2 (1 row)
        let total_rows: usize = result.iter().map(|b| b.num_rows()).sum();
        assert_eq!(total_rows, 3);
    }

    #[test]
    fn test_apply_batch_pagination_offset_and_limit() {
        let batch1 = create_test_batch(vec![1, 2], vec![Some("a"), Some("b")]);
        let batch2 = create_test_batch(vec![3, 4], vec![Some("c"), Some("d")]);
        let result = apply_batch_pagination(vec![batch1, batch2], 1, 2).unwrap();
        // Skip 1 row (id=1), take 2 rows (id=2,3)
        let total_rows: usize = result.iter().map(|b| b.num_rows()).sum();
        assert_eq!(total_rows, 2);
    }

    #[test]
    fn test_apply_column_projection_all_columns() {
        let batch = create_test_batch(vec![1, 2, 3], vec![Some("a"), Some("b"), Some("c")]);
        let columns = vec!["id".to_string(), "name".to_string()];
        let result = apply_column_projection(batch, &columns).unwrap();
        assert_eq!(result.num_columns(), 2);
        assert_eq!(result.num_rows(), 3);
    }

    #[test]
    fn test_apply_column_projection_single_column() {
        let batch = create_test_batch(vec![1, 2, 3], vec![Some("a"), Some("b"), Some("c")]);
        let columns = vec!["id".to_string()];
        let result = apply_column_projection(batch, &columns).unwrap();
        assert_eq!(result.num_columns(), 1);
        assert_eq!(result.num_rows(), 3);
    }

    #[test]
    fn test_apply_column_projection_nonexistent_column() {
        let batch = create_test_batch(vec![1, 2, 3], vec![Some("a"), Some("b"), Some("c")]);
        let columns = vec!["nonexistent".to_string()];
        let result = apply_column_projection(batch, &columns).unwrap();
        // Should return empty batch
        assert_eq!(result.num_rows(), 0);
    }

    #[test]
    fn test_apply_column_projection_mixed_valid_invalid() {
        let batch = create_test_batch(vec![1, 2, 3], vec![Some("a"), Some("b"), Some("c")]);
        let columns = vec!["id".to_string(), "nonexistent".to_string()];
        let result = apply_column_projection(batch, &columns).unwrap();
        // Should return only valid column
        assert_eq!(result.num_columns(), 1);
        assert_eq!(result.num_rows(), 3);
    }

    #[test]
    fn test_calculate_basic_statistics_empty() {
        let result = calculate_basic_statistics(&[]).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_calculate_basic_statistics_no_nulls() {
        let batch = create_test_batch(vec![1, 2, 3], vec![Some("a"), Some("b"), Some("c")]);
        let result = calculate_basic_statistics(&[batch]).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "id");
        assert_eq!(result[0].null_count, Some(0));
        assert_eq!(result[1].name, "name");
        assert_eq!(result[1].null_count, Some(0));
    }

    #[test]
    fn test_calculate_basic_statistics_with_nulls() {
        let batch = create_test_batch(vec![1, 2, 3], vec![Some("a"), None, Some("c")]);
        let result = calculate_basic_statistics(&[batch]).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].null_count, Some(0)); // id column
        assert_eq!(result[1].null_count, Some(1)); // name column has 1 null
    }

    #[test]
    fn test_calculate_basic_statistics_multiple_batches() {
        let batch1 = create_test_batch(vec![1, 2], vec![Some("a"), None]);
        let batch2 = create_test_batch(vec![3, 4], vec![None, Some("d")]);
        let result = calculate_basic_statistics(&[batch1, batch2]).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].null_count, Some(0)); // id: no nulls
        assert_eq!(result[1].null_count, Some(2)); // name: 2 nulls total
    }
}

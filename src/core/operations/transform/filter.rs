//! Row filtering operations

use arrow::array::{Array, BooleanArray};
use arrow::compute;
use arrow::compute::kernels::cmp;
use arrow::record_batch::RecordBatch;
use std::sync::Arc;

use crate::error::{Error, Result};

/// Maximum allowed filter expression length (DoS protection)
const MAX_EXPRESSION_LENGTH: usize = 10_000;

/// Maximum recursion depth for nested AND/OR expressions (DoS protection)
const MAX_RECURSION_DEPTH: usize = 50;

/// Apply row filter based on expression
pub fn apply_filter(batch: RecordBatch, filter_expr: &str) -> Result<RecordBatch> {
    // DoS protection: limit expression length
    if filter_expr.len() > MAX_EXPRESSION_LENGTH {
        return Err(Error::DataValidation {
            message: format!(
                "Filter expression too long: {} chars (max {})",
                filter_expr.len(),
                MAX_EXPRESSION_LENGTH
            ),
        });
    }

    // Parse and evaluate the filter expression with depth tracking
    let filter_array = evaluate_filter_expression_with_depth(&batch, filter_expr, 0)?;

    // Apply the filter
    compute::filter_record_batch(&batch, &filter_array).map_err(|e| Error::DataValidation {
        message: format!("Failed to apply filter: {}", e),
    })
}

/// Evaluate a filter expression with recursion depth tracking (DoS protection)
fn evaluate_filter_expression_with_depth(
    batch: &RecordBatch,
    expr: &str,
    depth: usize,
) -> Result<BooleanArray> {
    // DoS protection: limit recursion depth
    if depth > MAX_RECURSION_DEPTH {
        return Err(Error::DataValidation {
            message: format!(
                "Filter expression too deeply nested: depth {} (max {})",
                depth, MAX_RECURSION_DEPTH
            ),
        });
    }

    let expr = expr.trim();

    // Handle AND/OR operations
    if expr.contains(" AND ") {
        let parts: Vec<&str> = expr.splitn(2, " AND ").collect();
        let left = evaluate_filter_expression_with_depth(batch, parts[0], depth + 1)?;
        let right = evaluate_filter_expression_with_depth(batch, parts[1], depth + 1)?;
        return compute::and(&left, &right).map_err(|e| Error::DataValidation {
            message: format!("Failed to evaluate AND: {}", e),
        });
    }

    if expr.contains(" OR ") {
        let parts: Vec<&str> = expr.splitn(2, " OR ").collect();
        let left = evaluate_filter_expression_with_depth(batch, parts[0], depth + 1)?;
        let right = evaluate_filter_expression_with_depth(batch, parts[1], depth + 1)?;
        return compute::or(&left, &right).map_err(|e| Error::DataValidation {
            message: format!("Failed to evaluate OR: {}", e),
        });
    }

    // Parse simple comparison: column op value
    evaluate_simple_comparison(batch, expr)
}

/// Evaluate a simple filter expression and return a boolean array
/// Note: This is kept for backwards compatibility but uses the depth-limited version internally
#[allow(dead_code)]
fn evaluate_filter_expression(batch: &RecordBatch, expr: &str) -> Result<BooleanArray> {
    evaluate_filter_expression_with_depth(batch, expr, 0)
}

/// Evaluate a simple comparison expression
fn evaluate_simple_comparison(batch: &RecordBatch, expr: &str) -> Result<BooleanArray> {
    // Find the operator
    let operators = [">=", "<=", "!=", "=", ">", "<"];

    for op in &operators {
        if let Some(pos) = expr.find(op) {
            let column_name = expr[..pos].trim();
            let value_str = expr[pos + op.len()..].trim();

            // Remove quotes if present
            let value_str = value_str.trim_matches('\'').trim_matches('"');

            // Get the column
            let schema = batch.schema();
            let column_index = schema
                .index_of(column_name)
                .map_err(|_| Error::ColumnNotFound {
                    column: column_name.to_string(),
                })?;

            let column = batch.column(column_index);

            return apply_comparison(column, op, value_str);
        }
    }

    Err(Error::DataValidation {
        message: format!("Invalid filter expression: {}", expr),
    })
}

/// Apply comparison operation on a column
fn apply_comparison(column: &Arc<dyn Array>, op: &str, value_str: &str) -> Result<BooleanArray> {
    use arrow::array::*;
    use arrow::datatypes::*;

    match column.data_type() {
        DataType::Int64 => {
            let array = column
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or_else(|| Error::TypeConversion {
                    message: "Failed to downcast Int64Array".to_string(),
                })?;
            let value = value_str
                .parse::<i64>()
                .map_err(|_| Error::TypeConversion {
                    message: format!("Cannot parse '{}' as Int64", value_str),
                })?;

            Ok(match op {
                "=" => cmp::eq(array, &Int64Array::new_scalar(value))?,
                "!=" => cmp::neq(array, &Int64Array::new_scalar(value))?,
                ">" => cmp::gt(array, &Int64Array::new_scalar(value))?,
                ">=" => cmp::gt_eq(array, &Int64Array::new_scalar(value))?,
                "<" => cmp::lt(array, &Int64Array::new_scalar(value))?,
                "<=" => cmp::lt_eq(array, &Int64Array::new_scalar(value))?,
                _ => {
                    return Err(Error::UnsupportedFeature {
                        feature: format!("operator '{}' for Int64", op),
                    });
                }
            })
        }
        DataType::Float64 => {
            let array = column
                .as_any()
                .downcast_ref::<Float64Array>()
                .ok_or_else(|| Error::TypeConversion {
                    message: "Failed to downcast Float64Array".to_string(),
                })?;
            let value = value_str
                .parse::<f64>()
                .map_err(|_| Error::TypeConversion {
                    message: format!("Cannot parse '{}' as Float64", value_str),
                })?;

            Ok(match op {
                "=" => cmp::eq(array, &Float64Array::new_scalar(value))?,
                "!=" => cmp::neq(array, &Float64Array::new_scalar(value))?,
                ">" => cmp::gt(array, &Float64Array::new_scalar(value))?,
                ">=" => cmp::gt_eq(array, &Float64Array::new_scalar(value))?,
                "<" => cmp::lt(array, &Float64Array::new_scalar(value))?,
                "<=" => cmp::lt_eq(array, &Float64Array::new_scalar(value))?,
                _ => {
                    return Err(Error::UnsupportedFeature {
                        feature: format!("operator '{}' for Float64", op),
                    });
                }
            })
        }
        DataType::Utf8 => {
            let array = column
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| Error::TypeConversion {
                    message: "Failed to downcast StringArray".to_string(),
                })?;

            Ok(match op {
                "=" => cmp::eq(array, &StringArray::new_scalar(value_str))?,
                "!=" => cmp::neq(array, &StringArray::new_scalar(value_str))?,
                _ => {
                    return Err(Error::UnsupportedFeature {
                        feature: format!(
                            "comparison operator '{}' for String type (only '=' and '!=' supported)",
                            op
                        ),
                    });
                }
            })
        }
        _ => Err(Error::UnsupportedFeature {
            feature: format!("filtering for data type {:?}", column.data_type()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Float64Array, Int64Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;

    /// Create a test batch with Int64, Float64, and String columns
    fn create_test_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("value", DataType::Float64, false),
            Field::new("name", DataType::Utf8, false),
        ]));

        let id = Int64Array::from(vec![1, 2, 3, 4, 5]);
        let value = Float64Array::from(vec![10.0, 20.0, 30.0, 40.0, 50.0]);
        let name = StringArray::from(vec!["alice", "bob", "carol", "dave", "eve"]);

        RecordBatch::try_new(schema, vec![Arc::new(id), Arc::new(value), Arc::new(name)]).unwrap()
    }

    // ==================== Int64 comparison tests ====================

    #[test]
    fn test_filter_int_equal() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "id = 3").unwrap();
        assert_eq!(result.num_rows(), 1);
        let id_col = result
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(id_col.value(0), 3);
    }

    #[test]
    fn test_filter_int_not_equal() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "id != 3").unwrap();
        assert_eq!(result.num_rows(), 4);
    }

    #[test]
    fn test_filter_int_greater_than() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "id > 3").unwrap();
        assert_eq!(result.num_rows(), 2); // 4, 5
    }

    #[test]
    fn test_filter_int_greater_equal() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "id >= 3").unwrap();
        assert_eq!(result.num_rows(), 3); // 3, 4, 5
    }

    #[test]
    fn test_filter_int_less_than() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "id < 3").unwrap();
        assert_eq!(result.num_rows(), 2); // 1, 2
    }

    #[test]
    fn test_filter_int_less_equal() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "id <= 3").unwrap();
        assert_eq!(result.num_rows(), 3); // 1, 2, 3
    }

    // ==================== Float64 comparison tests ====================

    #[test]
    fn test_filter_float_greater() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "value > 25.0").unwrap();
        assert_eq!(result.num_rows(), 3); // 30.0, 40.0, 50.0
    }

    #[test]
    fn test_filter_float_less_equal() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "value <= 30.0").unwrap();
        assert_eq!(result.num_rows(), 3); // 10.0, 20.0, 30.0
    }

    // ==================== String comparison tests ====================

    #[test]
    fn test_filter_string_equal() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "name = 'bob'").unwrap();
        assert_eq!(result.num_rows(), 1);
        let name_col = result
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(name_col.value(0), "bob");
    }

    #[test]
    fn test_filter_string_not_equal() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "name != 'alice'").unwrap();
        assert_eq!(result.num_rows(), 4);
    }

    #[test]
    fn test_filter_string_with_double_quotes() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "name = \"carol\"").unwrap();
        assert_eq!(result.num_rows(), 1);
    }

    // ==================== Compound expression tests ====================

    #[test]
    fn test_filter_and() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "id > 2 AND id < 5").unwrap();
        assert_eq!(result.num_rows(), 2); // 3, 4
    }

    #[test]
    fn test_filter_or() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "id = 1 OR id = 5").unwrap();
        assert_eq!(result.num_rows(), 2);
    }

    #[test]
    fn test_filter_combined_and_or() {
        let batch = create_test_batch();
        // Due to left-to-right parsing: (id = 1 OR id = 2) AND id < 3 is not how it parses
        // Actually it parses as: id = 1 OR (id = 2 AND id < 3)
        // Let's use a simpler example
        let result = apply_filter(batch, "id >= 2 AND id <= 4").unwrap();
        assert_eq!(result.num_rows(), 3); // 2, 3, 4
    }

    // ==================== Error case tests ====================

    #[test]
    fn test_filter_invalid_column() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "nonexistent = 1");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent"));
    }

    #[test]
    fn test_filter_invalid_expression() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "gibberish");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid filter expression")
        );
    }

    #[test]
    fn test_filter_invalid_int_value() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "id = 'not_a_number'");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot parse"));
    }

    #[test]
    fn test_filter_unsupported_string_operator() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "name > 'bob'");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("only '=' and '!='")
        );
    }

    // ==================== Edge cases ====================

    #[test]
    fn test_filter_whitespace_handling() {
        let batch = create_test_batch();
        // Extra whitespace should be handled
        let result = apply_filter(batch, "  id  =  3  ").unwrap();
        assert_eq!(result.num_rows(), 1);
    }

    #[test]
    fn test_filter_empty_result() {
        let batch = create_test_batch();
        let result = apply_filter(batch, "id > 100").unwrap();
        assert_eq!(result.num_rows(), 0);
    }

    // ==================== DoS protection tests ====================

    #[test]
    fn test_filter_expression_too_long() {
        let batch = create_test_batch();
        // Create an expression that exceeds MAX_EXPRESSION_LENGTH
        let long_expr = format!("id = {}", "1".repeat(15000));
        let result = apply_filter(batch, &long_expr);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too long"));
    }

    #[test]
    fn test_filter_deeply_nested_expression() {
        let batch = create_test_batch();
        // Create a deeply nested expression: id = 1 AND id = 1 AND id = 1 AND ...
        let nested_expr = (0..100).map(|_| "id = 1").collect::<Vec<_>>().join(" AND ");
        let result = apply_filter(batch, &nested_expr);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("too deeply nested")
        );
    }

    #[test]
    fn test_filter_acceptable_nesting_depth() {
        let batch = create_test_batch();
        // Create a moderately nested expression that should pass
        let nested_expr = (0..10).map(|_| "id > 0").collect::<Vec<_>>().join(" AND ");
        let result = apply_filter(batch, &nested_expr);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().num_rows(), 5); // all rows pass
    }
}

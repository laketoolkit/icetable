//! Row filtering operations

use arrow::array::{Array, BooleanArray};
use arrow::compute;
use arrow::compute::kernels::cmp;
use arrow::record_batch::RecordBatch;
use std::sync::Arc;

use crate::error::{Error, Result};

/// Apply row filter based on expression
pub fn apply_filter(batch: RecordBatch, filter_expr: &str) -> Result<RecordBatch> {
    // Parse and evaluate the filter expression
    let filter_array = evaluate_filter_expression(&batch, filter_expr)?;

    // Apply the filter
    compute::filter_record_batch(&batch, &filter_array).map_err(|e| Error::DataValidation {
        message: format!("Failed to apply filter: {}", e),
    })
}

/// Evaluate a simple filter expression and return a boolean array
fn evaluate_filter_expression(batch: &RecordBatch, expr: &str) -> Result<BooleanArray> {
    // Simple expression parser for basic comparisons
    // Supports: column op value [AND/OR column op value]*
    // Where op is: =, !=, <, <=, >, >=

    let expr = expr.trim();

    // Handle AND/OR operations
    if expr.contains(" AND ") {
        let parts: Vec<&str> = expr.splitn(2, " AND ").collect();
        let left = evaluate_filter_expression(batch, parts[0])?;
        let right = evaluate_filter_expression(batch, parts[1])?;
        return compute::and(&left, &right).map_err(|e| Error::DataValidation {
            message: format!("Failed to evaluate AND: {}", e),
        });
    }

    if expr.contains(" OR ") {
        let parts: Vec<&str> = expr.splitn(2, " OR ").collect();
        let left = evaluate_filter_expression(batch, parts[0])?;
        let right = evaluate_filter_expression(batch, parts[1])?;
        return compute::or(&left, &right).map_err(|e| Error::DataValidation {
            message: format!("Failed to evaluate OR: {}", e),
        });
    }

    // Parse simple comparison: column op value
    evaluate_simple_comparison(batch, expr)
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

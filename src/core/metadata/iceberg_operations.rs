//! Iceberg operation type conversions
//!
//! This module provides conversions between internal operation types
//! and Iceberg's native operation types.

use super::traits::OperationType;

/// Convert internal OperationType to Iceberg Operation
pub fn to_iceberg_operation(op: OperationType) -> iceberg::spec::Operation {
    match op {
        OperationType::Append => iceberg::spec::Operation::Append,
        OperationType::Replace => iceberg::spec::Operation::Replace,
        OperationType::Delete => iceberg::spec::Operation::Delete,
        OperationType::Overwrite => iceberg::spec::Operation::Overwrite,
        OperationType::Restore => iceberg::spec::Operation::Replace,
        OperationType::Repair => iceberg::spec::Operation::Replace,
    }
}

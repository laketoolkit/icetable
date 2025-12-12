//! Type parsing utilities for Arrow data types

use crate::error::{Error, Result};
use arrow::datatypes::DataType;

/// Parse an Arrow DataType from a string representation
///
/// Supports common type names and aliases:
/// - Integer types: Int8, Int16, Int32, Int64, UInt8, UInt16, UInt32, UInt64
/// - Floating point: Float32, Float64
/// - String: String, Utf8
/// - Boolean: Bool, Boolean
/// - Date/Time: Date32, Date64, Timestamp
///
/// # Examples
/// ```ignore
/// let data_type = parse_data_type("Int64")?;
/// let data_type = parse_data_type("float64")?;
/// let data_type = parse_data_type("string")?;
/// ```
pub fn parse_data_type(type_str: &str) -> Result<DataType> {
    match type_str.to_lowercase().as_str() {
        // Integer types
        "int8" | "i8" => Ok(DataType::Int8),
        "int16" | "i16" => Ok(DataType::Int16),
        "int32" | "i32" => Ok(DataType::Int32),
        "int64" | "i64" => Ok(DataType::Int64),

        // Unsigned integer types
        "uint8" | "u8" => Ok(DataType::UInt8),
        "uint16" | "u16" => Ok(DataType::UInt16),
        "uint32" | "u32" => Ok(DataType::UInt32),
        "uint64" | "u64" => Ok(DataType::UInt64),

        // Floating point types
        "float32" | "f32" | "float" => Ok(DataType::Float32),
        "float64" | "f64" | "double" => Ok(DataType::Float64),

        // String types
        "string" | "utf8" => Ok(DataType::Utf8),
        "largestring" | "largeutf8" => Ok(DataType::LargeUtf8),

        // Boolean
        "bool" | "boolean" => Ok(DataType::Boolean),

        // Date and time types
        "date32" => Ok(DataType::Date32),
        "date64" => Ok(DataType::Date64),
        "timestamp" | "timestamp_us" => Ok(DataType::Timestamp(
            arrow::datatypes::TimeUnit::Microsecond,
            None,
        )),
        "timestamp_s" => Ok(DataType::Timestamp(
            arrow::datatypes::TimeUnit::Second,
            None,
        )),
        "timestamp_ms" => Ok(DataType::Timestamp(
            arrow::datatypes::TimeUnit::Millisecond,
            None,
        )),
        "timestamp_ns" => Ok(DataType::Timestamp(
            arrow::datatypes::TimeUnit::Nanosecond,
            None,
        )),

        _ => Err(Error::Parse {
            message: format!(
                "Unsupported data type '{}'. Supported types: \
                 Int8, Int16, Int32, Int64, UInt8, UInt16, UInt32, UInt64, \
                 Float32, Float64, String, LargeString, Boolean, \
                 Date32, Date64, Timestamp (with optional suffixes: _s, _ms, _us, _ns)",
                type_str
            ),
            source: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_integer_types() {
        assert!(matches!(parse_data_type("Int64").unwrap(), DataType::Int64));
        assert!(matches!(parse_data_type("i64").unwrap(), DataType::Int64));
        assert!(matches!(
            parse_data_type("uint32").unwrap(),
            DataType::UInt32
        ));
    }

    #[test]
    fn test_parse_float_types() {
        assert!(matches!(
            parse_data_type("Float64").unwrap(),
            DataType::Float64
        ));
        assert!(matches!(
            parse_data_type("double").unwrap(),
            DataType::Float64
        ));
        assert!(matches!(
            parse_data_type("float").unwrap(),
            DataType::Float32
        ));
    }

    #[test]
    fn test_parse_string_types() {
        assert!(matches!(parse_data_type("String").unwrap(), DataType::Utf8));
        assert!(matches!(parse_data_type("utf8").unwrap(), DataType::Utf8));
    }

    #[test]
    fn test_parse_unsupported() {
        assert!(parse_data_type("InvalidType").is_err());
    }
}

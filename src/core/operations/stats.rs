//! Stats operation - compute statistics for tabular data
//!
//! This module provides comprehensive statistical analysis for all column types
//! using Apache Arrow compute kernels for efficiency. It processes data in batches
//! to support large files without loading everything into memory.

use std::collections::HashMap;
use std::sync::Arc;

use arrow::array::{Array, ArrayRef, AsArray, BooleanArray, PrimitiveArray, StringArray};
use arrow::compute;
use arrow::datatypes::{
    ArrowPrimitiveType, DataType, Date32Type, Date64Type, Float32Type, Float64Type, Int8Type,
    Int16Type, Int32Type, Int64Type, TimestampMicrosecondType, TimestampMillisecondType,
    TimestampNanosecondType, TimestampSecondType, UInt8Type, UInt16Type, UInt32Type, UInt64Type,
};
use serde::Serialize;

use crate::core::formats::{FormatHandler, ReadOptions};
use crate::error::{Error, Result};

/// Options for stats operation
#[derive(Debug, Clone)]
pub struct StatsOptions {
    /// Generate histograms for numeric columns
    pub include_histogram: bool,

    /// Percentiles to compute (e.g., [0.25, 0.5, 0.75] for quartiles)
    pub percentiles: Vec<f64>,

    /// Full profiling (slower, includes distinct counts and most common values)
    pub profile: bool,

    /// Specific columns to analyze (None = all columns)
    pub columns: Option<Vec<String>>,
}

impl Default for StatsOptions {
    fn default() -> Self {
        Self {
            include_histogram: false,
            percentiles: vec![0.25, 0.5, 0.75],
            profile: false,
            columns: None,
        }
    }
}

/// Operation for computing statistics
pub struct StatsOperation {
    handler: Arc<dyn FormatHandler>,
}

impl StatsOperation {
    /// Create a new stats operation
    pub fn new(handler: Arc<dyn FormatHandler>) -> Self {
        Self { handler }
    }

    /// Execute stats computation
    pub async fn execute(&self, options: &StatsOptions) -> Result<StatsResult> {
        let schema = self.handler.read_schema().await?;

        // Determine which columns to analyze
        let column_names: Vec<String> = if let Some(cols) = &options.columns {
            cols.clone()
        } else {
            schema.fields().iter().map(|f| f.name().clone()).collect()
        };

        // Read data in batches
        let read_opts = ReadOptions::builder().batch_size(8192).build();
        let batches = self.handler.read_batches(&read_opts).await?;

        if batches.is_empty() {
            return Ok(StatsResult {
                total_rows: 0,
                column_stats: Vec::new(),
            });
        }

        // Compute total rows
        let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();

        // Compute statistics for each column
        let mut column_stats = Vec::new();

        for col_name in &column_names {
            let field = schema
                .field_with_name(col_name)
                .map_err(|e| Error::General(format!("Column '{}' not found: {}", col_name, e)))?;

            let col_index = schema
                .index_of(col_name)
                .map_err(|e| Error::General(format!("Column '{}' not found: {}", col_name, e)))?;

            // Collect all arrays for this column from all batches
            let arrays: Vec<ArrayRef> = batches
                .iter()
                .map(|b| b.column(col_index).clone())
                .collect();

            let stats = Self::compute_column_stats(
                col_name,
                field.data_type(),
                &arrays,
                total_rows,
                options,
            )?;

            column_stats.push(stats);
        }

        Ok(StatsResult {
            total_rows: total_rows as i64,
            column_stats,
        })
    }

    /// Compute statistics for a single column
    fn compute_column_stats(
        name: &str,
        data_type: &DataType,
        arrays: &[ArrayRef],
        total_rows: usize,
        options: &StatsOptions,
    ) -> Result<ColumnStatistics> {
        let null_count: usize = arrays.iter().map(|a| a.null_count()).sum();

        let mut stats = ColumnStatistics {
            name: name.to_string(),
            data_type: data_type.clone(),
            null_count,
            non_null_count: total_rows - null_count,
            numeric_stats: None,
            string_stats: None,
            boolean_stats: None,
            temporal_stats: None,
        };

        // Dispatch based on data type
        match data_type {
            DataType::Int8 => {
                stats.numeric_stats =
                    Some(Self::compute_numeric_stats::<Int8Type>(arrays, options)?);
            }
            DataType::Int16 => {
                stats.numeric_stats =
                    Some(Self::compute_numeric_stats::<Int16Type>(arrays, options)?);
            }
            DataType::Int32 => {
                stats.numeric_stats =
                    Some(Self::compute_numeric_stats::<Int32Type>(arrays, options)?);
            }
            DataType::Int64 => {
                stats.numeric_stats =
                    Some(Self::compute_numeric_stats::<Int64Type>(arrays, options)?);
            }
            DataType::UInt8 => {
                stats.numeric_stats =
                    Some(Self::compute_numeric_stats::<UInt8Type>(arrays, options)?);
            }
            DataType::UInt16 => {
                stats.numeric_stats =
                    Some(Self::compute_numeric_stats::<UInt16Type>(arrays, options)?);
            }
            DataType::UInt32 => {
                stats.numeric_stats =
                    Some(Self::compute_numeric_stats::<UInt32Type>(arrays, options)?);
            }
            DataType::UInt64 => {
                stats.numeric_stats =
                    Some(Self::compute_numeric_stats::<UInt64Type>(arrays, options)?);
            }
            DataType::Float32 => {
                stats.numeric_stats =
                    Some(Self::compute_numeric_stats::<Float32Type>(arrays, options)?);
            }
            DataType::Float64 => {
                stats.numeric_stats =
                    Some(Self::compute_numeric_stats::<Float64Type>(arrays, options)?);
            }
            DataType::Utf8 | DataType::LargeUtf8 => {
                stats.string_stats = Some(Self::compute_string_stats(arrays, options)?);
            }
            DataType::Boolean => {
                stats.boolean_stats = Some(Self::compute_boolean_stats(arrays)?);
            }
            DataType::Date32 => {
                stats.temporal_stats = Some(Self::compute_temporal_stats::<Date32Type>(arrays)?);
            }
            DataType::Date64 => {
                stats.temporal_stats = Some(Self::compute_temporal_stats::<Date64Type>(arrays)?);
            }
            DataType::Timestamp(_, _) => {
                stats.temporal_stats = Some(Self::compute_timestamp_stats(arrays, data_type)?);
            }
            _ => {
                // Unsupported type - skip
            }
        }

        Ok(stats)
    }

    /// Convert primitive native type to f64
    fn native_to_f64<T: ArrowPrimitiveType>(value: T::Native) -> f64 {
        // Use std::mem::transmute is unsafe, so we use manual conversion
        // This works for all numeric types
        use arrow::datatypes::*;
        use std::any::TypeId;

        let type_id = TypeId::of::<T::Native>();

        if type_id == TypeId::of::<i8>() {
            unsafe { std::mem::transmute_copy::<T::Native, i8>(&value) as f64 }
        } else if type_id == TypeId::of::<i16>() {
            unsafe { std::mem::transmute_copy::<T::Native, i16>(&value) as f64 }
        } else if type_id == TypeId::of::<i32>() {
            unsafe { std::mem::transmute_copy::<T::Native, i32>(&value) as f64 }
        } else if type_id == TypeId::of::<i64>() {
            unsafe { std::mem::transmute_copy::<T::Native, i64>(&value) as f64 }
        } else if type_id == TypeId::of::<u8>() {
            unsafe { std::mem::transmute_copy::<T::Native, u8>(&value) as f64 }
        } else if type_id == TypeId::of::<u16>() {
            unsafe { std::mem::transmute_copy::<T::Native, u16>(&value) as f64 }
        } else if type_id == TypeId::of::<u32>() {
            unsafe { std::mem::transmute_copy::<T::Native, u32>(&value) as f64 }
        } else if type_id == TypeId::of::<u64>() {
            unsafe { std::mem::transmute_copy::<T::Native, u64>(&value) as f64 }
        } else if type_id == TypeId::of::<f32>() {
            unsafe { std::mem::transmute_copy::<T::Native, f32>(&value) as f64 }
        } else if type_id == TypeId::of::<f64>() {
            unsafe { std::mem::transmute_copy::<T::Native, f64>(&value) }
        } else {
            0.0 // Fallback for unsupported types
        }
    }

    /// Compute numeric statistics using Arrow compute kernels
    fn compute_numeric_stats<T>(arrays: &[ArrayRef], options: &StatsOptions) -> Result<NumericStats>
    where
        T: ArrowPrimitiveType,
    {
        let mut min_val: Option<f64> = None;
        let mut max_val: Option<f64> = None;
        let mut sum = 0.0;
        let mut count = 0i64;

        for array in arrays {
            let primitive_array: &PrimitiveArray<T> = array.as_primitive();

            if let Some(min) = compute::min(primitive_array) {
                let min_f64 = Self::native_to_f64::<T>(min);
                min_val = Some(min_val.map_or(min_f64, |v| v.min(min_f64)));
            }

            if let Some(max) = compute::max(primitive_array) {
                let max_f64 = Self::native_to_f64::<T>(max);
                max_val = Some(max_val.map_or(max_f64, |v| v.max(max_f64)));
            }

            // Compute sum manually to handle nulls
            for i in 0..primitive_array.len() {
                if !primitive_array.is_null(i) {
                    sum += Self::native_to_f64::<T>(primitive_array.value(i));
                    count += 1;
                }
            }
        }

        let mean = if count > 0 {
            Some(sum / count as f64)
        } else {
            None
        };

        // Compute standard deviation
        let std_dev = if let Some(mean_val) = mean {
            let mut variance_sum = 0.0;
            for array in arrays {
                let primitive_array: &PrimitiveArray<T> = array.as_primitive();
                for i in 0..primitive_array.len() {
                    if !primitive_array.is_null(i) {
                        let val = Self::native_to_f64::<T>(primitive_array.value(i));
                        let diff = val - mean_val;
                        variance_sum += diff * diff;
                    }
                }
            }
            if count > 1 {
                Some((variance_sum / (count - 1) as f64).sqrt())
            } else {
                None
            }
        } else {
            None
        };

        // Compute median and percentiles if profiling
        let (median, percentiles) =
            if options.profile && !options.percentiles.is_empty() && count > 0 {
                let mut all_values = Vec::new();
                for array in arrays {
                    let primitive_array: &PrimitiveArray<T> = array.as_primitive();
                    for i in 0..primitive_array.len() {
                        if !primitive_array.is_null(i) {
                            all_values.push(Self::native_to_f64::<T>(primitive_array.value(i)));
                        }
                    }
                }
                all_values.sort_by(|a: &f64, b: &f64| {
                    a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                });

                let median_val = Self::compute_percentile(&all_values, 0.5);

                let percentile_vals: HashMap<String, f64> = options
                    .percentiles
                    .iter()
                    .map(|&p| {
                        let val = Self::compute_percentile(&all_values, p);
                        (format!("p{}", (p * 100.0) as i32), val)
                    })
                    .collect();

                (Some(median_val), Some(percentile_vals))
            } else {
                (None, None)
            };

        Ok(NumericStats {
            min: min_val,
            max: max_val,
            mean,
            median,
            std_dev,
            percentiles,
        })
    }

    /// Compute percentile from sorted values
    fn compute_percentile(sorted_values: &[f64], p: f64) -> f64 {
        if sorted_values.is_empty() {
            return 0.0;
        }
        let idx = (p * (sorted_values.len() - 1) as f64).round() as usize;
        sorted_values[idx.min(sorted_values.len() - 1)]
    }

    /// Compute string statistics
    fn compute_string_stats(arrays: &[ArrayRef], options: &StatsOptions) -> Result<StringStats> {
        let mut min_length: Option<usize> = None;
        let mut max_length: Option<usize> = None;
        let mut total_length = 0usize;
        let mut count = 0usize;

        // For profiling: track distinct values and frequencies
        let mut value_counts: HashMap<String, usize> = HashMap::new();

        for array in arrays {
            let string_array = array.as_string::<i32>();

            for i in 0..string_array.len() {
                if !string_array.is_null(i) {
                    let s = string_array.value(i);
                    let len = s.len();

                    min_length = Some(min_length.map_or(len, |v| v.min(len)));
                    max_length = Some(max_length.map_or(len, |v| v.max(len)));
                    total_length += len;
                    count += 1;

                    if options.profile {
                        *value_counts.entry(s.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }

        let avg_length = if count > 0 {
            Some(total_length as f64 / count as f64)
        } else {
            None
        };

        let (distinct_count, most_common) = if options.profile {
            let distinct = value_counts.len();

            // Get top 5 most common values
            let mut sorted_counts: Vec<_> = value_counts.into_iter().collect();
            sorted_counts.sort_by(|a, b| b.1.cmp(&a.1));
            let top_5: Vec<(String, usize)> = sorted_counts.into_iter().take(5).collect();

            (Some(distinct), Some(top_5))
        } else {
            (None, None)
        };

        Ok(StringStats {
            min_length,
            max_length,
            avg_length,
            distinct_count,
            most_common,
        })
    }

    /// Compute boolean statistics
    fn compute_boolean_stats(arrays: &[ArrayRef]) -> Result<BooleanStats> {
        let mut true_count = 0usize;
        let mut false_count = 0usize;

        for array in arrays {
            let bool_array = array.as_boolean();

            for i in 0..bool_array.len() {
                if !bool_array.is_null(i) {
                    if bool_array.value(i) {
                        true_count += 1;
                    } else {
                        false_count += 1;
                    }
                }
            }
        }

        let total = true_count + false_count;
        let true_percentage = if total > 0 {
            (true_count as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        Ok(BooleanStats {
            true_count,
            false_count,
            true_percentage,
        })
    }

    /// Compute temporal statistics for dates
    fn compute_temporal_stats<T>(arrays: &[ArrayRef]) -> Result<TemporalStats>
    where
        T: ArrowPrimitiveType,
    {
        let mut min_val: Option<i64> = None;
        let mut max_val: Option<i64> = None;

        for array in arrays {
            let primitive_array: &PrimitiveArray<T> = array.as_primitive();

            for i in 0..primitive_array.len() {
                if !primitive_array.is_null(i) {
                    let val = Self::native_to_f64::<T>(primitive_array.value(i)) as i64;
                    min_val = Some(min_val.map_or(val, |v| v.min(val)));
                    max_val = Some(max_val.map_or(val, |v| v.max(val)));
                }
            }
        }

        Ok(TemporalStats {
            min: min_val.map(|v| v.to_string()),
            max: max_val.map(|v| v.to_string()),
        })
    }

    /// Compute timestamp statistics
    fn compute_timestamp_stats(arrays: &[ArrayRef], data_type: &DataType) -> Result<TemporalStats> {
        match data_type {
            DataType::Timestamp(arrow::datatypes::TimeUnit::Second, _) => {
                Self::compute_temporal_stats::<TimestampSecondType>(arrays)
            }
            DataType::Timestamp(arrow::datatypes::TimeUnit::Millisecond, _) => {
                Self::compute_temporal_stats::<TimestampMillisecondType>(arrays)
            }
            DataType::Timestamp(arrow::datatypes::TimeUnit::Microsecond, _) => {
                Self::compute_temporal_stats::<TimestampMicrosecondType>(arrays)
            }
            DataType::Timestamp(arrow::datatypes::TimeUnit::Nanosecond, _) => {
                Self::compute_temporal_stats::<TimestampNanosecondType>(arrays)
            }
            _ => Err(Error::General("Invalid timestamp type".to_string())),
        }
    }
}

/// Result of a stats operation
#[derive(Debug, Serialize)]
pub struct StatsResult {
    /// Total number of rows analyzed
    pub total_rows: i64,

    /// Statistics for each column
    #[serde(skip)]
    pub column_stats: Vec<ColumnStatistics>,
}

/// Statistics for a single column
#[derive(Debug, Clone)]
pub struct ColumnStatistics {
    /// Column name
    pub name: String,

    /// Arrow data type
    #[allow(dead_code)]
    pub data_type: DataType,

    /// Number of null values
    pub null_count: usize,

    /// Number of non-null values
    pub non_null_count: usize,

    /// Numeric statistics (if applicable)
    pub numeric_stats: Option<NumericStats>,

    /// String statistics (if applicable)
    pub string_stats: Option<StringStats>,

    /// Boolean statistics (if applicable)
    pub boolean_stats: Option<BooleanStats>,

    /// Temporal statistics (if applicable)
    pub temporal_stats: Option<TemporalStats>,
}

/// Numeric column statistics
#[derive(Debug, Clone)]
pub struct NumericStats {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub std_dev: Option<f64>,
    pub percentiles: Option<HashMap<String, f64>>,
}

/// String column statistics
#[derive(Debug, Clone)]
pub struct StringStats {
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub avg_length: Option<f64>,
    pub distinct_count: Option<usize>,
    pub most_common: Option<Vec<(String, usize)>>,
}

/// Boolean column statistics
#[derive(Debug, Clone)]
pub struct BooleanStats {
    pub true_count: usize,
    pub false_count: usize,
    pub true_percentage: f64,
}

/// Temporal column statistics (dates, timestamps)
#[derive(Debug, Clone)]
pub struct TemporalStats {
    pub min: Option<String>,
    pub max: Option<String>,
}

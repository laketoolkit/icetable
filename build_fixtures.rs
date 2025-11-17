//! Generate test fixture Parquet and Arrow files
//! Run with: cargo run --bin build_fixtures

use datafusion::arrow::array::{
    ArrayRef, BooleanArray, Float64Array, Int32Array, Int64Array, StringArray,
    TimestampMillisecondArray,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use datafusion::arrow::ipc::writer::FileWriter as ArrowFileWriter;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::parquet::arrow::ArrowWriter;
use datafusion::parquet::file::properties::WriterProperties;
use std::fs::{self, File};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create fixtures directory
    fs::create_dir_all("tests/fixtures")?;

    // Generate Parquet files
    generate_simple_parquet()?;
    generate_types_parquet()?;
    generate_larger_parquet()?;

    // Generate Arrow IPC files
    generate_simple_arrow()?;
    generate_types_arrow()?;
    generate_larger_arrow()?;

    println!("Test fixtures generated successfully!");
    Ok(())
}

/// Generate a simple Parquet file with basic types
fn generate_simple_parquet() -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("name", DataType::Utf8, true),
        Field::new("age", DataType::Int32, true),
        Field::new("salary", DataType::Float64, true),
        Field::new("active", DataType::Boolean, false),
    ]));

    let id_array = Arc::new(Int32Array::from(vec![1, 2, 3, 4, 5])) as ArrayRef;
    let name_array = Arc::new(StringArray::from(vec![
        Some("Alice"),
        Some("Bob"),
        Some("Charlie"),
        None,
        Some("Eve"),
    ])) as ArrayRef;
    let age_array = Arc::new(Int32Array::from(vec![
        Some(30),
        Some(25),
        Some(35),
        Some(28),
        None,
    ])) as ArrayRef;
    let salary_array = Arc::new(Float64Array::from(vec![
        Some(75000.0),
        Some(65000.0),
        Some(95000.0),
        Some(72000.0),
        Some(88000.0),
    ])) as ArrayRef;
    let active_array =
        Arc::new(BooleanArray::from(vec![true, true, false, true, true])) as ArrayRef;

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![id_array, name_array, age_array, salary_array, active_array],
    )?;

    let file = File::create("tests/fixtures/sample.parquet")?;
    let props = WriterProperties::builder()
        .set_compression(datafusion::parquet::basic::Compression::SNAPPY)
        .build();

    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    println!("Created: tests/fixtures/sample.parquet");
    Ok(())
}

/// Generate Parquet file with various data types
fn generate_types_parquet() -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("int32_col", DataType::Int32, false),
        Field::new("int64_col", DataType::Int64, true),
        Field::new("float_col", DataType::Float64, true),
        Field::new("string_col", DataType::Utf8, true),
        Field::new("bool_col", DataType::Boolean, true),
        Field::new(
            "timestamp_col",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            true,
        ),
    ]));

    let int32_array = Arc::new(Int32Array::from(vec![1, 2, 3])) as ArrayRef;
    let int64_array = Arc::new(Int64Array::from(vec![Some(100), Some(200), None])) as ArrayRef;
    let float_array =
        Arc::new(Float64Array::from(vec![Some(1.1), Some(2.2), Some(3.3)])) as ArrayRef;
    let string_array =
        Arc::new(StringArray::from(vec![Some("foo"), None, Some("bar")])) as ArrayRef;
    let bool_array = Arc::new(BooleanArray::from(vec![Some(true), Some(false), None])) as ArrayRef;
    let timestamp_array = Arc::new(TimestampMillisecondArray::from(vec![
        Some(1609459200000), // 2021-01-01
        Some(1640995200000), // 2022-01-01
        None,
    ])) as ArrayRef;

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            int32_array,
            int64_array,
            float_array,
            string_array,
            bool_array,
            timestamp_array,
        ],
    )?;

    let file = File::create("tests/fixtures/types.parquet")?;
    let props = WriterProperties::builder()
        .set_compression(datafusion::parquet::basic::Compression::GZIP(
            Default::default(),
        ))
        .set_statistics_enabled(datafusion::parquet::file::properties::EnabledStatistics::Page)
        .build();

    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    println!("Created: tests/fixtures/types.parquet");
    Ok(())
}

/// Generate a larger Parquet file (100 rows)
fn generate_larger_parquet() -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("category", DataType::Utf8, false),
        Field::new("value", DataType::Float64, false),
        Field::new("description", DataType::Utf8, true),
    ]));

    let num_rows = 100;
    let categories = ["A", "B", "C", "D"];

    let id_data: Vec<i64> = (0..num_rows).collect();
    let category_data: Vec<&str> = (0..num_rows)
        .map(|i| categories[(i as usize) % categories.len()])
        .collect();
    let value_data: Vec<f64> = (0..num_rows).map(|i| i as f64 * 1.5).collect();
    let description_data: Vec<Option<String>> = (0..num_rows)
        .map(|i| {
            if i % 5 == 0 {
                None
            } else {
                Some(format!("Description for row {}", i))
            }
        })
        .collect();

    let id_array = Arc::new(Int64Array::from(id_data)) as ArrayRef;
    let category_array = Arc::new(StringArray::from(category_data)) as ArrayRef;
    let value_array = Arc::new(Float64Array::from(value_data)) as ArrayRef;
    let description_array = Arc::new(StringArray::from(description_data)) as ArrayRef;

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![id_array, category_array, value_array, description_array],
    )?;

    let file = File::create("tests/fixtures/larger.parquet")?;
    let props = WriterProperties::builder()
        .set_compression(datafusion::parquet::basic::Compression::ZSTD(
            Default::default(),
        ))
        .set_statistics_enabled(datafusion::parquet::file::properties::EnabledStatistics::Page)
        .build();

    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    println!("Created: tests/fixtures/larger.parquet");
    Ok(())
}

/// Generate a simple Arrow IPC file with basic types
fn generate_simple_arrow() -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("name", DataType::Utf8, true),
        Field::new("age", DataType::Int32, true),
        Field::new("salary", DataType::Float64, true),
        Field::new("active", DataType::Boolean, false),
    ]));

    let id_array = Arc::new(Int32Array::from(vec![1, 2, 3, 4, 5])) as ArrayRef;
    let name_array = Arc::new(StringArray::from(vec![
        Some("Alice"),
        Some("Bob"),
        Some("Charlie"),
        None,
        Some("Eve"),
    ])) as ArrayRef;
    let age_array = Arc::new(Int32Array::from(vec![
        Some(30),
        Some(25),
        Some(35),
        Some(28),
        None,
    ])) as ArrayRef;
    let salary_array = Arc::new(Float64Array::from(vec![
        Some(75000.0),
        Some(65000.0),
        Some(95000.0),
        Some(72000.0),
        Some(88000.0),
    ])) as ArrayRef;
    let active_array =
        Arc::new(BooleanArray::from(vec![true, true, false, true, true])) as ArrayRef;

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![id_array, name_array, age_array, salary_array, active_array],
    )?;

    let file = File::create("tests/fixtures/sample.arrow")?;
    let mut writer = ArrowFileWriter::try_new(file, &schema)?;
    writer.write(&batch)?;
    writer.finish()?;

    println!("Created: tests/fixtures/sample.arrow");
    Ok(())
}

/// Generate Arrow IPC file with various data types
fn generate_types_arrow() -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("int32_col", DataType::Int32, false),
        Field::new("int64_col", DataType::Int64, true),
        Field::new("float_col", DataType::Float64, true),
        Field::new("string_col", DataType::Utf8, true),
        Field::new("bool_col", DataType::Boolean, true),
        Field::new(
            "timestamp_col",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            true,
        ),
    ]));

    let int32_array = Arc::new(Int32Array::from(vec![1, 2, 3])) as ArrayRef;
    let int64_array = Arc::new(Int64Array::from(vec![Some(100), Some(200), None])) as ArrayRef;
    let float_array =
        Arc::new(Float64Array::from(vec![Some(1.1), Some(2.2), Some(3.3)])) as ArrayRef;
    let string_array =
        Arc::new(StringArray::from(vec![Some("foo"), None, Some("bar")])) as ArrayRef;
    let bool_array = Arc::new(BooleanArray::from(vec![Some(true), Some(false), None])) as ArrayRef;
    let timestamp_array = Arc::new(TimestampMillisecondArray::from(vec![
        Some(1609459200000), // 2021-01-01
        Some(1640995200000), // 2022-01-01
        None,
    ])) as ArrayRef;

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            int32_array,
            int64_array,
            float_array,
            string_array,
            bool_array,
            timestamp_array,
        ],
    )?;

    let file = File::create("tests/fixtures/types.arrow")?;
    let mut writer = ArrowFileWriter::try_new(file, &schema)?;
    writer.write(&batch)?;
    writer.finish()?;

    println!("Created: tests/fixtures/types.arrow");
    Ok(())
}

/// Generate a larger Arrow IPC file (100 rows)
fn generate_larger_arrow() -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("category", DataType::Utf8, false),
        Field::new("value", DataType::Float64, false),
        Field::new("description", DataType::Utf8, true),
    ]));

    let num_rows = 100;
    let categories = vec!["A", "B", "C", "D"];

    let id_data: Vec<i64> = (0..num_rows).collect();
    let category_data: Vec<&str> = (0..num_rows)
        .map(|i| categories[(i as usize) % categories.len()])
        .collect();
    let value_data: Vec<f64> = (0..num_rows).map(|i| i as f64 * 1.5).collect();
    let description_data: Vec<Option<String>> = (0..num_rows)
        .map(|i| {
            if i % 5 == 0 {
                None
            } else {
                Some(format!("Description for row {}", i))
            }
        })
        .collect();

    let id_array = Arc::new(Int64Array::from(id_data)) as ArrayRef;
    let category_array = Arc::new(StringArray::from(category_data)) as ArrayRef;
    let value_array = Arc::new(Float64Array::from(value_data)) as ArrayRef;
    let description_array = Arc::new(StringArray::from(description_data)) as ArrayRef;

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![id_array, category_array, value_array, description_array],
    )?;

    let file = File::create("tests/fixtures/larger.arrow")?;
    let mut writer = ArrowFileWriter::try_new(file, &schema)?;
    writer.write(&batch)?;
    writer.finish()?;

    println!("Created: tests/fixtures/larger.arrow");
    Ok(())
}

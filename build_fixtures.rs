//! Generate test fixture Parquet and Arrow files
//! Run with: cargo run --bin build_fixtures

use datafusion::arrow::array::{
    ArrayRef, BooleanArray, DictionaryArray, Float64Array, Int32Array, Int64Array, StringArray,
    TimestampMillisecondArray, UInt16Array,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema, TimeUnit, UInt16Type};
use datafusion::arrow::ipc::writer::FileWriter as ArrowFileWriter;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::parquet::arrow::ArrowWriter;
use datafusion::parquet::file::properties::WriterProperties;
use std::collections::HashMap;
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
    generate_arrow_with_metadata()?;

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

/// Generate Arrow IPC file with custom metadata and dictionary columns
fn generate_arrow_with_metadata() -> Result<(), Box<dyn std::error::Error>> {
    // Create schema with custom metadata and dictionary columns
    let mut metadata = HashMap::new();
    metadata.insert("writer.name".to_string(), "tabletools-test".to_string());
    metadata.insert("writer.version".to_string(), "0.1.0".to_string());
    metadata.insert("created_by".to_string(), "build_fixtures".to_string());
    metadata.insert(
        "description".to_string(),
        "Test file with dictionaries and custom metadata".to_string(),
    );

    let schema = Schema::new_with_metadata(
        vec![
            Field::new("id", DataType::Int32, false),
            Field::new(
                "category",
                DataType::Dictionary(Box::new(DataType::UInt16), Box::new(DataType::Utf8)),
                false,
            ),
            Field::new(
                "status",
                DataType::Dictionary(Box::new(DataType::UInt16), Box::new(DataType::Utf8)),
                false,
            ),
            Field::new("value", DataType::Int32, true),
        ],
        metadata,
    );

    let schema_ref = Arc::new(schema);

    // Create output file
    let file = File::create("tests/fixtures/with-metadata.arrow")?;
    let mut writer = ArrowFileWriter::try_new(file, &schema_ref)?;

    // Generate 3 record batches with dictionary-encoded data
    for batch_num in 0..3 {
        let batch_size = 1000;
        let start_id = batch_num * batch_size;

        // ID column
        let ids: Vec<i32> = (start_id..start_id + batch_size).collect();
        let id_array = Arc::new(Int32Array::from(ids)) as ArrayRef;

        // Category dictionary column (3 categories)
        let categories = vec!["Electronics", "Clothing", "Food"];
        let category_values: Vec<u16> = (0..batch_size).map(|i| (i % 3) as u16).collect();

        let category_keys = UInt16Array::from(category_values);
        let category_dict: ArrayRef = Arc::new(StringArray::from(categories));
        let category_array =
            Arc::new(DictionaryArray::<UInt16Type>::try_new(category_keys, category_dict)?)
                as ArrayRef;

        // Status dictionary column (4 statuses)
        let statuses = vec!["pending", "active", "completed", "cancelled"];
        let status_values: Vec<u16> = (0..batch_size).map(|i| (i % 4) as u16).collect();

        let status_keys = UInt16Array::from(status_values);
        let status_dict: ArrayRef = Arc::new(StringArray::from(statuses));
        let status_array =
            Arc::new(DictionaryArray::<UInt16Type>::try_new(status_keys, status_dict)?) as ArrayRef;

        // Value column (some nulls)
        let values: Vec<Option<i32>> = (0..batch_size)
            .map(|i| if i % 10 == 0 { None } else { Some(i * 100) })
            .collect();
        let value_array = Arc::new(Int32Array::from(values)) as ArrayRef;

        let batch = RecordBatch::try_new(
            schema_ref.clone(),
            vec![id_array, category_array, status_array, value_array],
        )?;

        writer.write(&batch)?;
    }

    writer.finish()?;

    println!("Created: tests/fixtures/with-metadata.arrow");
    println!("  - 3 RecordBatches (3000 rows total)");
    println!("  - 2 Dictionary columns (category, status)");
    println!("  - 4 custom metadata entries");

    Ok(())
}

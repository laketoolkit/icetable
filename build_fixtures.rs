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
    generate_parquet_with_advanced_features()?;

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

/// Generate Parquet file with advanced features (bloom filters, column indexes, etc.)
fn generate_parquet_with_advanced_features() -> Result<(), Box<dyn std::error::Error>> {
    use datafusion::parquet::file::properties::EnabledStatistics;

    let schema = Arc::new(Schema::new(vec![
        Field::new("user_id", DataType::Int64, false),
        Field::new("username", DataType::Utf8, false),
        Field::new("email", DataType::Utf8, false),
        Field::new("age", DataType::Int32, true),
        Field::new("score", DataType::Float64, true),
        Field::new("active", DataType::Boolean, false),
        Field::new("created_at", DataType::Timestamp(TimeUnit::Millisecond, None), false),
    ]));

    // Generate data with patterns that benefit from bloom filters
    let num_rows = 10000;
    let mut user_ids = Vec::with_capacity(num_rows);
    let mut usernames = Vec::with_capacity(num_rows);
    let mut emails = Vec::with_capacity(num_rows);
    let mut ages = Vec::with_capacity(num_rows);
    let mut scores = Vec::with_capacity(num_rows);
    let mut actives = Vec::with_capacity(num_rows);
    let mut timestamps = Vec::with_capacity(num_rows);

    for i in 0..num_rows {
        user_ids.push(i as i64);
        usernames.push(format!("user_{:06}", i));
        emails.push(format!("user{}@example{}.com", i, i % 100));
        ages.push(if i % 10 == 0 { None } else { Some(20 + (i % 50) as i32) });
        scores.push(if i % 15 == 0 { None } else { Some((i as f64) * 0.123 + 50.0) });
        actives.push(i % 3 != 0);
        timestamps.push(1609459200000 + (i as i64) * 86400000); // Daily increments from 2021-01-01
    }

    let user_id_array = Arc::new(Int64Array::from(user_ids)) as ArrayRef;
    let username_array = Arc::new(StringArray::from(usernames)) as ArrayRef;
    let email_array = Arc::new(StringArray::from(emails)) as ArrayRef;
    let age_array = Arc::new(Int32Array::from(ages)) as ArrayRef;
    let score_array = Arc::new(Float64Array::from(scores)) as ArrayRef;
    let active_array = Arc::new(BooleanArray::from(actives)) as ArrayRef;
    let timestamp_array = Arc::new(TimestampMillisecondArray::from(timestamps)) as ArrayRef;

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            user_id_array,
            username_array,
            email_array,
            age_array,
            score_array,
            active_array,
            timestamp_array,
        ],
    )?;

    let file = File::create("tests/fixtures/with_extra_metadata.parquet")?;

    // Create writer properties with advanced features
    let props = WriterProperties::builder()
        .set_compression(datafusion::parquet::basic::Compression::ZSTD(datafusion::parquet::basic::ZstdLevel::default()))
        .set_statistics_enabled(EnabledStatistics::Page)
        .set_column_bloom_filter_enabled("username".into(), true)
        .set_column_bloom_filter_enabled("email".into(), true)
        .set_column_bloom_filter_enabled("user_id".into(), true)
        .set_max_row_group_size(5000) // Create 2 row groups
        .set_write_batch_size(1024)
        .set_data_page_size_limit(8192)
        .set_dictionary_enabled(true)
        .set_column_dictionary_enabled("email".into(), true)
        .set_key_value_metadata(Some(vec![
            datafusion::parquet::file::metadata::KeyValue::new(
                "created_by".to_string(),
                "tabletools advanced test generator".to_string(),
            ),
            datafusion::parquet::file::metadata::KeyValue::new(
                "version".to_string(),
                "2.0".to_string(),
            ),
            datafusion::parquet::file::metadata::KeyValue::new(
                "description".to_string(),
                "Parquet file with bloom filters, column indexes, and advanced features".to_string(),
            ),
            datafusion::parquet::file::metadata::KeyValue::new(
                "test_features".to_string(),
                "bloom_filters,column_indexes,page_statistics,dictionary_encoding,zstd_compression".to_string(),
            ),
        ]))
        .build();

    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    println!("Created: tests/fixtures/with_extra_metadata.parquet");
    println!("  - 10,000 rows across 2 row groups");
    println!("  - Bloom filters on: user_id, username, email");
    println!("  - ZSTD compression");
    println!("  - Page-level statistics");
    println!("  - Dictionary encoding on email column");
    println!("  - Custom metadata entries");

    Ok(())
}

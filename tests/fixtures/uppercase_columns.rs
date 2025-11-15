//! Generate test fixture with UPPERCASE column names to test case sensitivity

use arrow::array::{ArrayRef, Float64Array, Int32Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::ipc::writer::FileWriter as ArrowFileWriter;
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    generate_uppercase_parquet()?;
    generate_uppercase_arrow()?;
    println!("Uppercase column fixtures generated successfully!");
    Ok(())
}

/// Generate a Parquet file with UPPERCASE column names (like real flight data)
fn generate_uppercase_parquet() -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("FL_DATE", DataType::Utf8, false),
        Field::new("ARR_DELAY", DataType::Int32, true),
        Field::new("DEP_DELAY", DataType::Int32, true),
        Field::new("AIR_TIME", DataType::Float64, true),
        Field::new("DISTANCE", DataType::Int32, false),
    ]));

    let fl_date_array = Arc::new(StringArray::from(vec![
        "2024-01-01",
        "2024-01-02",
        "2024-01-03",
        "2024-01-04",
        "2024-01-05",
    ])) as ArrayRef;

    let arr_delay_array = Arc::new(Int32Array::from(vec![
        Some(-5),
        Some(10),
        Some(-2),
        None,
        Some(25),
    ])) as ArrayRef;

    let dep_delay_array = Arc::new(Int32Array::from(vec![
        Some(0),
        Some(5),
        Some(-3),
        Some(15),
        Some(20),
    ])) as ArrayRef;

    let air_time_array = Arc::new(Float64Array::from(vec![
        Some(120.5),
        Some(135.0),
        Some(115.8),
        Some(142.3),
        Some(128.9),
    ])) as ArrayRef;

    let distance_array = Arc::new(Int32Array::from(vec![
        500, 600, 480, 650, 550,
    ])) as ArrayRef;

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![fl_date_array, arr_delay_array, dep_delay_array, air_time_array, distance_array],
    )?;

    let file = File::create("tests/fixtures/uppercase.parquet")?;
    let props = WriterProperties::builder()
        .set_compression(parquet::basic::Compression::SNAPPY)
        .build();

    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    println!("Created: tests/fixtures/uppercase.parquet");
    Ok(())
}

/// Generate an Arrow IPC file with UPPERCASE column names
fn generate_uppercase_arrow() -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("FL_DATE", DataType::Utf8, false),
        Field::new("ARR_DELAY", DataType::Int32, true),
        Field::new("DEP_DELAY", DataType::Int32, true),
        Field::new("AIR_TIME", DataType::Float64, true),
        Field::new("DISTANCE", DataType::Int32, false),
    ]));

    let fl_date_array = Arc::new(StringArray::from(vec![
        "2024-01-01",
        "2024-01-02",
        "2024-01-03",
        "2024-01-04",
        "2024-01-05",
    ])) as ArrayRef;

    let arr_delay_array = Arc::new(Int32Array::from(vec![
        Some(-5),
        Some(10),
        Some(-2),
        None,
        Some(25),
    ])) as ArrayRef;

    let dep_delay_array = Arc::new(Int32Array::from(vec![
        Some(0),
        Some(5),
        Some(-3),
        Some(15),
        Some(20),
    ])) as ArrayRef;

    let air_time_array = Arc::new(Float64Array::from(vec![
        Some(120.5),
        Some(135.0),
        Some(115.8),
        Some(142.3),
        Some(128.9),
    ])) as ArrayRef;

    let distance_array = Arc::new(Int32Array::from(vec![
        500, 600, 480, 650, 550,
    ])) as ArrayRef;

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![fl_date_array, arr_delay_array, dep_delay_array, air_time_array, distance_array],
    )?;

    let file = File::create("tests/fixtures/uppercase.arrow")?;
    let mut writer = ArrowFileWriter::try_new(file, &schema)?;
    writer.write(&batch)?;
    writer.finish()?;

    println!("Created: tests/fixtures/uppercase.arrow");
    Ok(())
}

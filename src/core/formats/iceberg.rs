//! Iceberg format handler implementation
//!
//! This module provides support for reading Apache Iceberg tables.
//! Apache Iceberg is a high-performance table format for huge analytic datasets.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Fields, Schema as ArrowSchema, TimeUnit};
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use futures::StreamExt;
use iceberg::TableIdent;
use iceberg::io::FileIOBuilder;
use iceberg::table::StaticTable;

use crate::core::formats::table_utils;
use crate::core::formats::traits::*;
use crate::core::storage::StorageBackend;
use crate::error::{Error, Result};

/// Handler for Apache Iceberg tables
pub struct IcebergHandler {
    path: PathBuf,
    storage: Arc<dyn StorageBackend>,
    time_travel: TimeTravelOptions,
}

impl IcebergHandler {
    /// Create a new Iceberg handler
    pub fn new(path: &Path, storage: Arc<dyn StorageBackend>) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            storage,
            time_travel: TimeTravelOptions::default(),
        })
    }

    /// Create a new Iceberg handler with time-travel options
    pub fn with_time_travel(
        path: &Path,
        storage: Arc<dyn StorageBackend>,
        time_travel: TimeTravelOptions,
    ) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            storage,
            time_travel,
        })
    }

    /// Get the snapshot ID to use based on time-travel options
    fn get_target_snapshot_id(&self, table: &StaticTable) -> Result<Option<i64>> {
        let metadata = table.metadata();

        // If version is specified, use it directly as snapshot ID
        if let Some(snapshot_id) = self.time_travel.version {
            // Verify the snapshot exists
            if metadata.snapshot_by_id(snapshot_id).is_some() {
                return Ok(Some(snapshot_id));
            } else {
                return Err(Error::General(format!(
                    "Snapshot with ID {} not found in table",
                    snapshot_id
                )));
            }
        }

        // If as_of timestamp is specified, find the appropriate snapshot
        if let Some(ref as_of) = self.time_travel.as_of {
            let target_ts = Self::parse_timestamp(as_of)?;

            // Find the latest snapshot at or before the target timestamp
            let mut best_snapshot: Option<i64> = None;
            let mut best_ts: i64 = 0;

            for snapshot in metadata.snapshots() {
                let snapshot_ts = snapshot.timestamp_ms();
                if snapshot_ts <= target_ts && snapshot_ts > best_ts {
                    best_ts = snapshot_ts;
                    best_snapshot = Some(snapshot.snapshot_id());
                }
            }

            if let Some(snapshot_id) = best_snapshot {
                return Ok(Some(snapshot_id));
            } else {
                return Err(Error::General(format!(
                    "No snapshot found at or before timestamp '{}'",
                    as_of
                )));
            }
        }

        // No time-travel options, use current snapshot
        Ok(None)
    }

    /// Parse timestamp string into milliseconds since epoch
    fn parse_timestamp(ts: &str) -> Result<i64> {
        use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};

        // Try parsing as full datetime first (e.g., "2024-01-15T10:30:00")
        if let Ok(dt) = NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S") {
            return Ok(Utc.from_utc_datetime(&dt).timestamp_millis());
        }

        // Try parsing as date only (e.g., "2024-01-15")
        if let Ok(date) = NaiveDate::parse_from_str(ts, "%Y-%m-%d") {
            let dt = date.and_hms_opt(23, 59, 59).unwrap();
            return Ok(Utc.from_utc_datetime(&dt).timestamp_millis());
        }

        Err(Error::General(format!(
            "Invalid timestamp format '{}'. Use 'YYYY-MM-DD' or 'YYYY-MM-DDTHH:MM:SS'",
            ts
        )))
    }

    /// Open the Iceberg table using StaticTable
    ///
    /// This uses StaticTable which loads the table directly from the metadata file
    /// without requiring a catalog.
    async fn open_table(&self) -> Result<StaticTable> {
        let table_path = self.path.to_string_lossy().to_string();

        // Check if this is a cloud storage path
        let is_cloud = table_path.starts_with("s3://")
            || table_path.starts_with("s3a://")
            || table_path.starts_with("gs://")
            || table_path.starts_with("gcs://")
            || table_path.starts_with("az://")
            || table_path.starts_with("abfs://")
            || table_path.starts_with("abfss://");

        // For local paths, convert to absolute
        let table_path = if !is_cloud && !self.path.is_absolute() {
            std::env::current_dir()
                .map_err(|e| Error::General(format!("Failed to get current dir: {}", e)))?
                .join(&self.path)
                .to_string_lossy()
                .to_string()
        } else {
            table_path
        };

        // Find the metadata location
        let metadata_location = self.find_metadata_location(&table_path).await?;

        // Create FileIO based on path scheme
        let file_io = Self::create_file_io(&table_path)?;

        // Create a table identifier (just for identification purposes)
        let table_ident = TableIdent::from_strs(["iceberg", "table"])
            .map_err(|e| Error::General(format!("Failed to create table identifier: {}", e)))?;

        // Load the static table from the metadata file
        let table =
            StaticTable::from_metadata_file(&metadata_location, table_ident, file_io.clone())
                .await
                .map_err(|e| {
                    Error::General(format!(
                        "Failed to load Iceberg table from '{}': {}",
                        metadata_location, e
                    ))
                })?;

        Ok(table)
    }

    /// Create FileIO based on path scheme
    fn create_file_io(path: &str) -> Result<iceberg::io::FileIO> {
        if path.starts_with("s3://") || path.starts_with("s3a://") {
            // For S3, build FileIO with s3 scheme
            // Read credentials and config from environment
            let mut builder = FileIOBuilder::new("s3");

            // S3 credentials
            if let Ok(key) = std::env::var("AWS_ACCESS_KEY_ID") {
                builder = builder.with_prop("s3.access-key-id", key);
            }
            if let Ok(secret) = std::env::var("AWS_SECRET_ACCESS_KEY") {
                builder = builder.with_prop("s3.secret-access-key", secret);
            }
            if let Ok(token) = std::env::var("AWS_SESSION_TOKEN") {
                builder = builder.with_prop("s3.session-token", token);
            }

            // S3 endpoint (for MinIO or other S3-compatible storage)
            if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
                builder = builder.with_prop("s3.endpoint", endpoint);
            }

            // S3 region
            if let Ok(region) = std::env::var("AWS_REGION") {
                builder = builder.with_prop("s3.region", region);
            } else if let Ok(region) = std::env::var("AWS_DEFAULT_REGION") {
                builder = builder.with_prop("s3.region", region);
            } else {
                // Default region for MinIO/local S3
                builder = builder.with_prop("s3.region", "us-east-1");
            }

            // Enable path-style access for MinIO
            builder = builder.with_prop("s3.path-style-access", "true");

            builder
                .build()
                .map_err(|e| Error::General(format!("Failed to create S3 FileIO: {}", e)))
        } else if path.starts_with("gs://") || path.starts_with("gcs://") {
            // For GCS
            FileIOBuilder::new("gcs")
                .build()
                .map_err(|e| Error::General(format!("Failed to create GCS FileIO: {}", e)))
        } else if path.starts_with("az://")
            || path.starts_with("abfs://")
            || path.starts_with("abfss://")
        {
            // For Azure
            FileIOBuilder::new("azblob")
                .build()
                .map_err(|e| Error::General(format!("Failed to create Azure FileIO: {}", e)))
        } else {
            // Local filesystem
            FileIOBuilder::new_fs_io()
                .build()
                .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))
        }
    }

    /// Find the metadata file location for an Iceberg table
    async fn find_metadata_location(&self, table_path: &str) -> Result<String> {
        use crate::core::storage::traits::{GetOptions, ListOptions};

        // If path already points to a metadata.json file, use it directly
        if table_path.ends_with(".metadata.json") || table_path.ends_with("metadata.json") {
            return Ok(table_path.to_string());
        }

        let metadata_dir = format!("{}/metadata", table_path.trim_end_matches('/'));

        // Try to read version-hint.text first
        let version_hint_path = format!("{}/version-hint.text", metadata_dir);
        let get_opts = GetOptions::default();

        if let Ok(version_bytes) = self.storage.get(&version_hint_path, &get_opts).await {
            let version_str = String::from_utf8_lossy(&version_bytes).trim().to_string();
            if let Ok(version) = version_str.parse::<i32>() {
                return Ok(format!("{}/v{}.metadata.json", metadata_dir, version));
            }
        }

        // Fallback: list metadata directory and find the latest metadata file
        let list_opts = ListOptions {
            prefix: Some(format!("{}/", metadata_dir)),
            delimiter: None,
            max_results: Some(100),
            continuation_token: None,
        };

        if let Ok(listing) = self.storage.list(&list_opts).await {
            // Find the latest v*.metadata.json file
            let metadata_file = listing
                .objects
                .iter()
                .filter(|obj| obj.path.contains(".metadata.json"))
                .max_by_key(|obj| &obj.last_modified);

            if let Some(file) = metadata_file {
                return Ok(file.path.clone());
            }
        }

        Err(Error::General(format!(
            "Could not find metadata file in {}",
            metadata_dir
        )))
    }

    /// Convert Iceberg schema to Arrow schema
    fn iceberg_schema_to_arrow(iceberg_schema: &iceberg::spec::Schema) -> Result<ArrowSchema> {
        // Get the struct representation of the schema
        let struct_type = iceberg_schema.as_struct();

        // Convert each field
        let fields: Result<Vec<Field>> = struct_type
            .fields()
            .iter()
            .map(|field| {
                // Convert the iceberg field to an arrow field
                // Note: Iceberg's `required` means NOT nullable, so we invert it
                let data_type = Self::iceberg_type_to_arrow(&field.field_type)?;
                Ok(Field::new(field.name.clone(), data_type, !field.required))
            })
            .collect();

        Ok(ArrowSchema::new(Fields::from(fields?)))
    }

    /// Convert Iceberg type to Arrow DataType
    fn iceberg_type_to_arrow(iceberg_type: &iceberg::spec::Type) -> Result<DataType> {
        use iceberg::spec::PrimitiveType;

        match iceberg_type {
            iceberg::spec::Type::Primitive(prim) => match prim {
                PrimitiveType::Boolean => Ok(DataType::Boolean),
                PrimitiveType::Int => Ok(DataType::Int32),
                PrimitiveType::Long => Ok(DataType::Int64),
                PrimitiveType::Float => Ok(DataType::Float32),
                PrimitiveType::Double => Ok(DataType::Float64),
                PrimitiveType::Date => Ok(DataType::Date32),
                PrimitiveType::Time => Ok(DataType::Time64(TimeUnit::Microsecond)),
                PrimitiveType::Timestamp => Ok(DataType::Timestamp(TimeUnit::Microsecond, None)),
                PrimitiveType::Timestamptz => Ok(DataType::Timestamp(
                    TimeUnit::Microsecond,
                    Some("UTC".into()),
                )),
                PrimitiveType::TimestampNs => Ok(DataType::Timestamp(TimeUnit::Nanosecond, None)),
                PrimitiveType::TimestamptzNs => Ok(DataType::Timestamp(
                    TimeUnit::Nanosecond,
                    Some("UTC".into()),
                )),
                PrimitiveType::String => Ok(DataType::Utf8),
                PrimitiveType::Uuid => Ok(DataType::FixedSizeBinary(16)),
                PrimitiveType::Fixed(size) => Ok(DataType::FixedSizeBinary(*size as i32)),
                PrimitiveType::Binary => Ok(DataType::Binary),
                PrimitiveType::Decimal { precision, scale } => {
                    Ok(DataType::Decimal128(*precision as u8, *scale as i8))
                }
            },
            iceberg::spec::Type::Struct(_) => {
                // For complex types, we'll return a placeholder for now
                Err(Error::UnsupportedFeature {
                    feature: "Nested struct types in Iceberg not yet fully supported".to_string(),
                })
            }
            iceberg::spec::Type::List(_) => Err(Error::UnsupportedFeature {
                feature: "List types in Iceberg not yet fully supported".to_string(),
            }),
            iceberg::spec::Type::Map(_) => Err(Error::UnsupportedFeature {
                feature: "Map types in Iceberg not yet fully supported".to_string(),
            }),
        }
    }
}

#[async_trait]
impl FormatHandler for IcebergHandler {
    async fn can_handle(&self, path: &Path) -> Result<bool> {
        // Check if this is an Iceberg table by looking for metadata directory
        let table_path = path.to_string_lossy().to_string();

        // Look for metadata directory or metadata.json
        let metadata_dir = if table_path.ends_with('/') {
            format!("{}/metadata", table_path.trim_end_matches('/'))
        } else {
            format!("{}/metadata", table_path)
        };

        // For local storage, check if metadata directory exists
        if self.storage.storage_type() == "local" {
            let metadata_path = Path::new(&metadata_dir);
            if metadata_path.exists() && metadata_path.is_dir() {
                return Ok(true);
            }
        }

        // For cloud storage or uncertain cases, try to open the table
        match self.open_table().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    fn format_name(&self) -> &str {
        "Apache Iceberg"
    }

    async fn read_schema(&self) -> Result<Arc<ArrowSchema>> {
        let table = self.open_table().await?;
        let metadata = table.metadata().clone();
        let iceberg_schema = metadata.current_schema();

        let arrow_schema = Self::iceberg_schema_to_arrow(iceberg_schema)?;
        Ok(Arc::new(arrow_schema))
    }

    async fn read_metadata(&self) -> Result<FileMetadata> {
        let table = self.open_table().await?;
        let metadata = table.metadata();

        let mut metadata_map = std::collections::HashMap::new();

        // Get table UUID
        metadata_map.insert("table_uuid".to_string(), metadata.uuid().to_string());

        // Get format version
        let format_version = metadata.format_version();

        // Extract snapshot summary statistics
        let mut num_rows: Option<i64> = None;
        let mut compressed_size: Option<u64> = None;
        let mut created_at: Option<chrono::DateTime<chrono::Utc>> = None;

        if let Some(snapshot) = metadata.current_snapshot() {
            // Snapshot timestamp
            created_at = chrono::DateTime::from_timestamp_millis(snapshot.timestamp_ms());

            // Extract summary properties
            let summary = &snapshot.summary().additional_properties;

            if let Some(records) = summary.get("total-records") {
                num_rows = records.parse().ok();
            }

            if let Some(size) = summary.get("total-files-size") {
                compressed_size = size.parse().ok();
            }

            // Copy useful summary fields directly (without prefix)
            for key in &[
                "operation",
                "total-data-files",
                "total-delete-files",
                "added-records",
                "deleted-records",
                "added-data-files",
                "deleted-data-files",
            ] {
                if let Some(value) = summary.get(*key) {
                    metadata_map.insert(key.to_string(), value.clone());
                }
            }
        }

        Ok(FileMetadata {
            num_rows,
            compressed_size,
            uncompressed_size: None,
            compression: None,
            format_version: Some(format_version.to_string()),
            created_at,
            metadata: metadata_map,
        })
    }

    async fn read_batch(&self, options: &ReadOptions) -> Result<RecordBatch> {
        let batches = self.read_batches(options).await?;
        let schema = self.read_schema().await?;
        table_utils::merge_batches(batches, schema)
    }

    async fn read_batches(&self, options: &ReadOptions) -> Result<Vec<RecordBatch>> {
        let table = self.open_table().await?;

        // Get target snapshot ID for time-travel
        let target_snapshot_id = self.get_target_snapshot_id(&table)?;

        // Build a table scan
        let mut scan_builder = table.scan();

        // Apply time-travel snapshot if specified
        if let Some(snapshot_id) = target_snapshot_id {
            scan_builder = scan_builder.snapshot_id(snapshot_id);
        }

        // Apply column selection if specified
        let scan_builder = if let Some(columns) = options.columns() {
            scan_builder.select(columns.to_vec())
        } else {
            scan_builder
        };

        // Execute the scan (build is now synchronous in 0.7)
        let scan = scan_builder
            .build()
            .map_err(|e| Error::General(format!("Failed to build Iceberg scan: {}", e)))?;

        let stream = scan
            .to_arrow()
            .await
            .map_err(|e| Error::General(format!("Failed to execute Iceberg scan: {}", e)))?;

        // Read all batches from the stream
        let mut batches = Vec::new();

        let mut stream = std::pin::pin!(stream);
        while let Some(batch_result) = stream.next().await {
            let batch =
                batch_result.map_err(|e| Error::General(format!("Failed to read batch: {}", e)))?;
            batches.push(batch);
        }

        // Apply pagination using table_utils
        let offset = options.offset().unwrap_or(0);
        let limit = options.limit().unwrap_or(usize::MAX);
        table_utils::apply_batch_pagination(batches, offset, limit)
    }

    async fn read_statistics(&self) -> Result<Vec<ColumnStats>> {
        use iceberg::spec::{ManifestContentType, ManifestList, ManifestStatus};
        use std::collections::{HashMap, HashSet};

        let table = self.open_table().await?;
        let metadata = table.metadata();
        let schema = metadata.current_schema();

        let total_records = metadata
            .current_snapshot()
            .and_then(|s| s.summary().additional_properties.get("total-records"))
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);

        // Initialize stats per field_id
        let mut null_counts: HashMap<i32, i64> = HashMap::new();
        let mut min_values: HashMap<i32, String> = HashMap::new();
        let mut max_values: HashMap<i32, String> = HashMap::new();

        for field in schema.as_struct().fields() {
            null_counts.insert(field.id, 0);
        }

        // Helper closure to build result
        let build_result = |nulls: &HashMap<i32, i64>,
                            mins: &HashMap<i32, String>,
                            maxs: &HashMap<i32, String>| {
            schema
                .as_struct()
                .fields()
                .iter()
                .enumerate()
                .map(|(idx, field)| ColumnStats {
                    name: field.name.clone(),
                    null_count: nulls.get(&field.id).copied(),
                    distinct_count: if idx == 0 { Some(total_records) } else { None },
                    min_value: mins.get(&field.id).cloned(),
                    max_value: maxs.get(&field.id).cloned(),
                    mean: None,
                    std_dev: None,
                })
                .collect::<Vec<_>>()
        };

        let current_snapshot = match metadata.current_snapshot() {
            Some(s) => s,
            None => return Ok(build_result(&null_counts, &min_values, &max_values)),
        };

        let file_io = Self::create_file_io(&self.path.to_string_lossy())?;

        let content = file_io
            .new_input(current_snapshot.manifest_list())
            .map_err(|e| Error::General(format!("Failed to open manifest list: {}", e)))?
            .read()
            .await
            .map_err(|e| Error::General(format!("Failed to read manifest list: {}", e)))?;

        let manifest_list =
            ManifestList::parse_with_version(&content, metadata.format_version())
                .map_err(|e| Error::General(format!("Failed to parse manifest list: {}", e)))?;

        // Filter to data manifests only
        let data_entries: Vec<_> = manifest_list
            .entries()
            .iter()
            .filter(|e| e.content == ManifestContentType::Data)
            .collect();

        // Load all manifests in parallel
        let manifest_futures: Vec<_> = data_entries
            .iter()
            .map(|entry| entry.load_manifest(&file_io))
            .collect();

        let manifests: Vec<_> = futures::future::join_all(manifest_futures)
            .await
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();

        // Single pass: collect deleted paths and aggregate stats
        let mut deleted: HashSet<String> = HashSet::new();

        // First collect all deleted file paths
        for manifest in &manifests {
            for entry in manifest.entries() {
                if entry.status() == ManifestStatus::Deleted {
                    deleted.insert(entry.data_file().file_path().to_string());
                }
            }
        }

        // Then aggregate stats from alive files
        for manifest in &manifests {
            for entry in manifest.entries() {
                if entry.status() == ManifestStatus::Deleted {
                    continue;
                }
                let df = entry.data_file();
                if deleted.contains(df.file_path()) {
                    continue;
                }

                for (&fid, &cnt) in df.null_value_counts() {
                    *null_counts.entry(fid).or_insert(0) += cnt as i64;
                }

                for (&fid, datum) in df.lower_bounds() {
                    let v = format!("{}", datum);
                    min_values
                        .entry(fid)
                        .and_modify(|cur| {
                            if v < *cur {
                                *cur = v.clone();
                            }
                        })
                        .or_insert(v);
                }

                for (&fid, datum) in df.upper_bounds() {
                    let v = format!("{}", datum);
                    max_values
                        .entry(fid)
                        .and_modify(|cur| {
                            if v > *cur {
                                *cur = v.clone();
                            }
                        })
                        .or_insert(v);
                }
            }
        }

        Ok(build_result(&null_counts, &min_values, &max_values))
    }

    async fn validate(&self, _quick: bool) -> Result<ValidationReport> {
        let mut report = ValidationReport::success();

        // Try to open the table
        match self.open_table().await {
            Ok(table) => {
                // Check if we can read the schema
                let metadata = table.metadata();
                if metadata.current_schema().as_struct().fields().is_empty() {
                    report
                        .errors
                        .push("Iceberg table has empty schema".to_string());
                    report.is_valid = false;
                }

                // Check if table has a current snapshot
                if metadata.current_snapshot().is_none() {
                    report
                        .warnings
                        .push("Iceberg table has no current snapshot (empty table)".to_string());
                }
            }
            Err(e) => {
                report
                    .errors
                    .push(format!("Failed to open Iceberg table: {}", e));
                report.is_valid = false;
            }
        }

        Ok(report)
    }

    async fn write(&self, data: Vec<RecordBatch>, _options: &WriteOptions) -> Result<()> {
        use iceberg::io::FileIOBuilder;
        use iceberg::spec::{
            DataContentType, DataFileBuilder, DataFileFormat, ManifestListWriter,
            ManifestWriterBuilder, Snapshot, Struct, Summary, TableMetadataBuilder,
        };
        use parquet::arrow::ArrowWriter;
        use parquet::file::properties::WriterProperties;
        use std::collections::HashMap;
        use std::fs::File;

        if data.is_empty() {
            return Ok(());
        }

        // Get table path
        let abs_path = if self.path.is_absolute() {
            self.path.clone()
        } else {
            std::env::current_dir()
                .map_err(|e| Error::General(format!("Failed to get current dir: {}", e)))?
                .join(&self.path)
        };
        let table_path = abs_path.to_string_lossy().to_string();

        // Open existing table to get metadata
        let table = self.open_table().await?;
        let old_metadata = table.metadata().clone();

        // Directories
        let data_dir = format!("{}/data", table_path);
        let metadata_dir = format!("{}/metadata", table_path);
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| Error::General(format!("Failed to create data dir: {}", e)))?;

        // Count total rows
        let total_rows: usize = data.iter().map(|b| b.num_rows()).sum();

        // Write parquet file - generate unique ID from timestamp and random
        let timestamp_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let file_id = format!("{:016x}", timestamp_nanos);
        let parquet_filename = format!("00000-0-{}.parquet", file_id);
        let parquet_path = format!("{}/{}", data_dir, parquet_filename);

        // Get schema and add Iceberg field IDs for parquet compatibility
        let input_schema = data[0].schema();
        let iceberg_schema = old_metadata.current_schema();

        // Create a new Arrow schema with field_id metadata from Iceberg schema
        let fields_with_ids: Vec<arrow::datatypes::Field> = input_schema
            .fields()
            .iter()
            .enumerate()
            .map(|(idx, field)| {
                // Find matching Iceberg field by name
                let field_id = iceberg_schema
                    .as_struct()
                    .fields()
                    .iter()
                    .find(|f| f.name == *field.name())
                    .map(|f| f.id)
                    .unwrap_or((idx + 1) as i32);

                // Add PARQUET:field_id metadata
                let mut metadata = field.metadata().clone();
                metadata.insert("PARQUET:field_id".to_string(), field_id.to_string());
                field.as_ref().clone().with_metadata(metadata)
            })
            .collect();

        let schema = Arc::new(arrow::datatypes::Schema::new(fields_with_ids));

        let file = File::create(&parquet_path)
            .map_err(|e| Error::General(format!("Failed to create parquet file: {}", e)))?;
        let props = WriterProperties::builder().build();
        let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props))
            .map_err(|e| Error::General(format!("Failed to create parquet writer: {}", e)))?;

        for batch in &data {
            writer
                .write(batch)
                .map_err(|e| Error::General(format!("Failed to write batch: {}", e)))?;
        }
        writer
            .close()
            .map_err(|e| Error::General(format!("Failed to close parquet writer: {}", e)))?;

        let file_size = std::fs::metadata(&parquet_path)
            .map_err(|e| Error::General(format!("Failed to get file size: {}", e)))?
            .len();

        // Build DataFile
        let partition_spec = old_metadata.default_partition_spec();
        let data_file = DataFileBuilder::default()
            .content(DataContentType::Data)
            .file_path(parquet_path.clone())
            .file_format(DataFileFormat::Parquet)
            .partition(Struct::empty())
            .partition_spec_id(partition_spec.spec_id())
            .record_count(total_rows as u64)
            .file_size_in_bytes(file_size)
            .build()
            .map_err(|e| Error::General(format!("Failed to build DataFile: {}", e)))?;

        // Create FileIO
        let file_io = FileIOBuilder::new_fs_io()
            .build()
            .map_err(|e| Error::General(format!("Failed to create FileIO: {}", e)))?;

        // Generate snapshot ID and sequence number
        let snapshot_id = chrono::Utc::now().timestamp_millis();
        let sequence_number = old_metadata
            .current_snapshot()
            .map(|s| s.sequence_number() + 1)
            .unwrap_or(1);

        // Write manifest file
        let manifest_filename = format!("{}-m0.avro", file_id);
        let manifest_path = format!("{}/{}", metadata_dir, manifest_filename);

        let output_file = file_io
            .new_output(&manifest_path)
            .map_err(|e| Error::General(format!("Failed to create manifest output: {}", e)))?;

        let mut manifest_writer = ManifestWriterBuilder::new(
            output_file,
            Some(snapshot_id),
            None,
            iceberg_schema.clone(),
            (**partition_spec).clone(),
        )
        .build_v2_data();

        manifest_writer
            .add_file(data_file, sequence_number)
            .map_err(|e| Error::General(format!("Failed to add file to manifest: {}", e)))?;

        let manifest_file = manifest_writer
            .write_manifest_file()
            .await
            .map_err(|e| Error::General(format!("Failed to write manifest: {}", e)))?;

        // Write manifest list
        let manifest_list_filename = format!("snap-{}-0-{}.avro", snapshot_id, file_id);
        let manifest_list_path = format!("{}/{}", metadata_dir, manifest_list_filename);

        let manifest_list_output = file_io
            .new_output(&manifest_list_path)
            .map_err(|e| Error::General(format!("Failed to create manifest list output: {}", e)))?;

        let mut manifest_list_writer = ManifestListWriter::v2(
            manifest_list_output,
            snapshot_id,
            Some(snapshot_id),
            sequence_number,
        );

        manifest_list_writer
            .add_manifests(vec![manifest_file].into_iter())
            .map_err(|e| Error::General(format!("Failed to add manifest to list: {}", e)))?;

        manifest_list_writer
            .close()
            .await
            .map_err(|e| Error::General(format!("Failed to close manifest list: {}", e)))?;

        // Build new snapshot
        let timestamp_ms = chrono::Utc::now().timestamp_millis();
        let summary = Summary {
            operation: iceberg::spec::Operation::Append,
            additional_properties: HashMap::from([
                ("added-files-size".to_string(), file_size.to_string()),
                ("added-data-files".to_string(), "1".to_string()),
                ("added-records".to_string(), total_rows.to_string()),
                ("total-records".to_string(), total_rows.to_string()),
                ("total-files-size".to_string(), file_size.to_string()),
                ("total-data-files".to_string(), "1".to_string()),
            ]),
        };

        let snapshot = Snapshot::builder()
            .with_snapshot_id(snapshot_id)
            .with_sequence_number(sequence_number)
            .with_timestamp_ms(timestamp_ms)
            .with_manifest_list(manifest_list_path)
            .with_summary(summary)
            .with_schema_id(iceberg_schema.schema_id())
            .build();

        // Build new metadata - need to dereference Arc
        let old_metadata_owned: iceberg::spec::TableMetadata = (*old_metadata).clone();

        // Get current metadata version for the metadata log path
        let current_version = get_current_version(&metadata_dir)?;
        let metadata_log_path = format!("v{}.metadata.json", current_version);

        let new_metadata =
            TableMetadataBuilder::new_from_metadata(old_metadata_owned, Some(metadata_log_path))
                .set_branch_snapshot(snapshot, iceberg::spec::MAIN_BRANCH)
                .map_err(|e| Error::General(format!("Failed to set branch snapshot: {}", e)))?
                .build()
                .map_err(|e| Error::General(format!("Failed to build metadata: {}", e)))?;

        let new_version = current_version + 1;

        // Write new metadata file
        let new_metadata_file = format!("{}/v{}.metadata.json", metadata_dir, new_version);
        let metadata_json = serde_json::to_string_pretty(&new_metadata.metadata)
            .map_err(|e| Error::General(format!("Failed to serialize metadata: {}", e)))?;
        std::fs::write(&new_metadata_file, metadata_json)
            .map_err(|e| Error::General(format!("Failed to write metadata file: {}", e)))?;

        // Update version hint
        let version_hint_file = format!("{}/version-hint.text", metadata_dir);
        std::fs::write(&version_hint_file, new_version.to_string())
            .map_err(|e| Error::General(format!("Failed to update version hint: {}", e)))?;

        Ok(())
    }

    fn has_native_statistics(&self) -> bool {
        true // Iceberg maintains statistics in manifest files
    }
}

/// Get current version from metadata directory
fn get_current_version(metadata_dir: &str) -> Result<i32> {
    // Try version-hint.text first
    let version_hint_path = format!("{}/version-hint.text", metadata_dir);
    if let Ok(content) = std::fs::read_to_string(&version_hint_path)
        && let Ok(version) = content.trim().parse::<i32>()
    {
        return Ok(version);
    }

    // Fallback: scan for v*.metadata.json files
    let mut max_version = 0;
    if let Ok(entries) = std::fs::read_dir(metadata_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('v')
                && name.ends_with(".metadata.json")
                && let Some(version_str) = name
                    .strip_prefix('v')
                    .and_then(|s| s.strip_suffix(".metadata.json"))
                && let Ok(v) = version_str.parse::<i32>()
            {
                max_version = max_version.max(v);
            }
        }
    }

    Ok(max_version)
}

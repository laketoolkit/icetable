//! Parquet physical layout inspector

use async_trait::async_trait;
use datafusion::parquet::basic::Type as PhysicalType;
use datafusion::parquet::file::metadata::FileMetaData;
use datafusion::parquet::file::reader::{FileReader, SerializedFileReader};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::core::storage::{GetOptions, StorageBackend};
use crate::error::{Error, Result};

use super::formatters::{format_number, get_file_name};
use super::registry::PhysicalInspectorFactory;
use super::traits::{
    ColumnChunkMetadata, ColumnInfo, ColumnStatistics, FileInfo, LayoutInfo, PhysicalInspectOptions,
    PhysicalInspector, PhysicalMetadata, RowGroupLayout, RowGroupMetadata, SchemaInfo,
    StatisticsInfo,
};

/// Parquet physical inspector
pub struct ParquetInspector {
    path: PathBuf,
    storage: Arc<dyn StorageBackend>,
}

impl ParquetInspector {
    /// Create a new Parquet inspector
    pub fn new(path: PathBuf, storage: Arc<dyn StorageBackend>) -> Self {
        Self { path, storage }
    }

    /// Extract file information from metadata
    fn extract_file_info(
        &self,
        metadata: &FileMetaData,
        num_row_groups: usize,
        file_size: u64,
    ) -> FileInfo {
        let mut file_metadata = HashMap::new();
        file_metadata.insert("num_rows".to_string(), format_number(metadata.num_rows()));
        file_metadata.insert("num_row_groups".to_string(), num_row_groups.to_string());
        file_metadata.insert(
            "num_columns".to_string(),
            metadata.schema_descr().num_columns().to_string(),
        );

        FileInfo {
            path: get_file_name(&self.path),
            file_size,
            format_version: format!("{}", metadata.version()),
            created_by: metadata.created_by().map(|s| {
                if s.len() > 50 {
                    format!("{}...", &s[..47])
                } else {
                    s.to_string()
                }
            }),
            metadata: file_metadata,
        }
    }

    /// Extract schema information
    fn extract_schema(&self, metadata: &FileMetaData) -> SchemaInfo {
        let schema = metadata.schema_descr();
        let columns = schema
            .columns()
            .iter()
            .enumerate()
            .map(|(idx, col)| {
                let col_type = format!("{:?}", col.physical_type());
                let logical_type = col
                    .logical_type()
                    .map(|lt| format!(" ({:?})", lt))
                    .unwrap_or_default();

                let full_type = if logical_type.len() > 40 {
                    format!("{}{}", col_type, &logical_type[..37])
                } else {
                    format!("{}{}", col_type, logical_type)
                };

                ColumnInfo {
                    name: col.name().to_string(),
                    column_type: full_type,
                    nullable: true, // Parquet columns are generally nullable by default
                    index: idx,
                }
            })
            .collect();

        SchemaInfo {
            num_columns: schema.num_columns(),
            columns,
        }
    }

    /// Extract row group layout information
    fn extract_layout(
        &self,
        metadata: &FileMetaData,
        row_groups: &[datafusion::parquet::file::metadata::RowGroupMetaData],
        _options: &PhysicalInspectOptions,
    ) -> RowGroupLayout {
        let schema = metadata.schema_descr();

        let row_group_metadata = row_groups
            .iter()
            .enumerate()
            .map(|(idx, rg)| {
                let columns = rg
                    .columns()
                    .iter()
                    .enumerate()
                    .map(|(col_idx, col_chunk)| {
                        let col_name = schema
                            .columns()
                            .get(col_idx)
                            .map(|c| c.name())
                            .unwrap_or("unknown");

                        let encodings = col_chunk
                            .encodings()
                            .iter()
                            .map(|e| format!("{:?}", e))
                            .collect::<Vec<_>>()
                            .join(", ");

                        ColumnChunkMetadata {
                            column_name: col_name.to_string(),
                            compression: format!("{:?}", col_chunk.compression()),
                            compressed_size: col_chunk.compressed_size(),
                            uncompressed_size: col_chunk.uncompressed_size(),
                            encoding: encodings,
                        }
                    })
                    .collect();

                // Calculate total uncompressed size for row group
                let total_uncompressed: i64 = rg
                    .columns()
                    .iter()
                    .map(|col| col.uncompressed_size())
                    .sum();

                RowGroupMetadata {
                    index: idx,
                    num_rows: rg.num_rows(),
                    total_compressed_size: rg.total_byte_size(),
                    total_uncompressed_size: total_uncompressed,
                    columns,
                }
            })
            .collect();

        RowGroupLayout {
            num_row_groups: row_groups.len(),
            row_groups: row_group_metadata,
        }
    }

    /// Extract statistics information
    fn extract_statistics(
        &self,
        metadata: &FileMetaData,
        row_groups: &[datafusion::parquet::file::metadata::RowGroupMetaData],
        _options: &PhysicalInspectOptions,
    ) -> StatisticsInfo {
        let total_compressed: i64 = row_groups.iter().map(|rg| rg.total_byte_size()).sum();

        let total_uncompressed: i64 = row_groups
            .iter()
            .flat_map(|rg| rg.columns())
            .map(|col| col.uncompressed_size())
            .sum();

        let schema = metadata.schema_descr();
        let column_stats = schema
            .columns()
            .iter()
            .enumerate()
            .map(|(col_idx, col_desc)| {
                let col_name = col_desc.name();
                let mut total_null_count: i64 = 0;
                let mut has_stats = false;
                let mut min_value: Option<String> = None;
                let mut max_value: Option<String> = None;
                let mut distinct_count: Option<i64> = None;

                for rg in row_groups {
                    if let Some(col_chunk) = rg.columns().get(col_idx) {
                        if let Some(stats) = col_chunk.statistics() {
                            if let Some(null_count) = stats.null_count_opt() {
                                total_null_count += null_count as i64;
                                has_stats = true;
                            }

                            // Extract min/max values (only from first row group for simplicity)
                            if min_value.is_none() {
                                let physical_type = col_desc.physical_type();
                                if let Some(min_bytes) = stats.min_bytes_opt() {
                                    min_value = Some(Self::format_stat_value(min_bytes, physical_type));
                                }
                                if let Some(max_bytes) = stats.max_bytes_opt() {
                                    max_value = Some(Self::format_stat_value(max_bytes, physical_type));
                                }
                            }

                            // Get distinct count if available
                            if let Some(dc) = stats.distinct_count_opt() {
                                distinct_count = Some(dc as i64);
                            }
                        }
                    }
                }

                ColumnStatistics {
                    column_name: col_name.to_string(),
                    null_count: if has_stats { Some(total_null_count) } else { None },
                    min_value,
                    max_value,
                    distinct_count,
                }
            })
            .collect();

        StatisticsInfo {
            total_rows: metadata.num_rows(),
            compressed_size: total_compressed as u64,
            uncompressed_size: total_uncompressed as u64,
            column_stats,
        }
    }

    /// Format a statistic value based on its physical type
    fn format_stat_value(bytes: &[u8], physical_type: PhysicalType) -> String {
        match physical_type {
            PhysicalType::INT32 => {
                if bytes.len() >= 4 {
                    let value = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    value.to_string()
                } else {
                    format!("<invalid i32: {} bytes>", bytes.len())
                }
            }
            PhysicalType::INT64 => {
                if bytes.len() >= 8 {
                    let value = i64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
                    ]);
                    value.to_string()
                } else {
                    format!("<invalid i64: {} bytes>", bytes.len())
                }
            }
            PhysicalType::FLOAT => {
                if bytes.len() >= 4 {
                    let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    format!("{:.2}", value)
                } else {
                    format!("<invalid f32: {} bytes>", bytes.len())
                }
            }
            PhysicalType::DOUBLE => {
                if bytes.len() >= 8 {
                    let value = f64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
                    ]);
                    format!("{:.2}", value)
                } else {
                    format!("<invalid f64: {} bytes>", bytes.len())
                }
            }
            PhysicalType::BYTE_ARRAY => {
                // Try to decode as UTF-8 string
                match String::from_utf8(bytes.to_vec()) {
                    Ok(s) => {
                        if s.len() > 30 {
                            format!("\"{}...\"", &s[..27])
                        } else {
                            format!("\"{}\"", s)
                        }
                    }
                    Err(_) => {
                        // Not valid UTF-8, show as hex
                        if bytes.len() > 16 {
                            format!("<binary: {} bytes>", bytes.len())
                        } else {
                            format!(
                                "<hex: {}>",
                                bytes
                                    .iter()
                                    .map(|b| format!("{:02x}", b))
                                    .collect::<String>()
                            )
                        }
                    }
                }
            }
            PhysicalType::BOOLEAN => {
                if !bytes.is_empty() {
                    if bytes[0] != 0 {
                        "true"
                    } else {
                        "false"
                    }
                    .to_string()
                } else {
                    "<invalid bool>".to_string()
                }
            }
            PhysicalType::FIXED_LEN_BYTE_ARRAY => {
                if bytes.len() > 16 {
                    format!("<fixed binary: {} bytes>", bytes.len())
                } else {
                    format!(
                        "<hex: {}>",
                        bytes
                            .iter()
                            .map(|b| format!("{:02x}", b))
                            .collect::<String>()
                    )
                }
            }
            PhysicalType::INT96 => {
                format!("<int96: {} bytes>", bytes.len())
            }
        }
    }
}

#[async_trait]
impl PhysicalInspector for ParquetInspector {
    async fn extract_metadata(
        &self,
        options: &PhysicalInspectOptions,
    ) -> Result<PhysicalMetadata> {
        // Read the file from storage
        let path_str = self
            .path
            .to_str()
            .ok_or_else(|| Error::General(format!("Invalid path: {}", self.path.display())))?;

        let data = self.storage.get(path_str, &GetOptions::default()).await?;
        let file_size = data.len() as u64;

        // Create a Parquet reader - Bytes implements ChunkReader
        let reader = SerializedFileReader::new(data.clone())
            .map_err(|e| Error::General(format!("Failed to read Parquet file: {}", e)))?;

        let metadata = reader.metadata().file_metadata();
        let row_groups = reader.metadata().row_groups();

        // Extract file information
        let file_info = self.extract_file_info(metadata, row_groups.len(), file_size);

        // Extract schema if requested
        let schema = if options.show_schema {
            Some(self.extract_schema(metadata))
        } else {
            None
        };

        // Extract layout if requested
        let layout = if options.show_layout {
            Some(LayoutInfo::RowGroupBased(
                self.extract_layout(metadata, row_groups, options),
            ))
        } else {
            None
        };

        // Extract statistics if requested
        let statistics = if options.show_stats {
            Some(self.extract_statistics(metadata, row_groups, options))
        } else {
            None
        };

        Ok(PhysicalMetadata {
            format_name: self.format_name().to_string(),
            file_info,
            schema,
            layout,
            statistics,
        })
    }

    fn format_name(&self) -> &str {
        "Apache Parquet"
    }

    fn can_inspect(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("parquet"))
            .unwrap_or(false)
    }
}

/// Factory for creating Parquet inspectors
pub struct ParquetInspectorFactory;

impl PhysicalInspectorFactory for ParquetInspectorFactory {
    fn create(
        &self,
        path: &Path,
        storage: Arc<dyn StorageBackend>,
    ) -> Result<Box<dyn PhysicalInspector>> {
        Ok(Box::new(ParquetInspector::new(
            path.to_path_buf(),
            storage,
        )))
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("parquet"))
            .unwrap_or(false)
    }

    fn priority(&self) -> i32 {
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::LocalBackend;

    #[test]
    fn test_can_inspect() {
        let storage = Arc::new(LocalBackend::new().unwrap());
        let inspector = ParquetInspector::new(PathBuf::from("test.parquet"), storage);

        assert!(inspector.can_inspect(Path::new("test.parquet")));
        assert!(inspector.can_inspect(Path::new("test.PARQUET")));
        assert!(!inspector.can_inspect(Path::new("test.csv")));
        assert!(!inspector.can_inspect(Path::new("test")));
    }

    #[test]
    fn test_format_name() {
        let storage = Arc::new(LocalBackend::new().unwrap());
        let inspector = ParquetInspector::new(PathBuf::from("test.parquet"), storage);

        assert_eq!(inspector.format_name(), "Apache Parquet");
    }

    #[test]
    fn test_factory_can_handle() {
        let factory = ParquetInspectorFactory;

        assert!(factory.can_handle(Path::new("test.parquet")));
        assert!(factory.can_handle(Path::new("test.PARQUET")));
        assert!(!factory.can_handle(Path::new("test.csv")));
        assert!(!factory.can_handle(Path::new("test.arrow")));
    }

    #[test]
    fn test_factory_priority() {
        let factory = ParquetInspectorFactory;
        assert_eq!(factory.priority(), 100);
    }
}

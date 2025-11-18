//! Physical inspection subsystem for table formats
//!
//! This module provides a clean separation between metadata extraction (core)
//! and presentation (CLI). It defines:
//!
//! - `PhysicalInspector` trait for format-specific metadata extraction
//! - `PhysicalMetadata` and related types for format-agnostic data
//! - `InspectionViewBuilder` for converting metadata to presentation-ready views
//! - `PhysicalInspectionService` for high-level orchestration
//! - `PhysicalInspectorRegistry` for dynamic inspector discovery

pub mod formatters;
pub mod parquet;
pub mod registry;
pub mod service;
pub mod traits;
pub mod view_builder;

// Re-export commonly used types
pub use formatters::*;
pub use registry::{PhysicalInspectorFactory, PhysicalInspectorRegistry};
pub use service::PhysicalInspectionService;
pub use traits::{
    BatchLayout, BatchMetadata, ColumnChunkMetadata, ColumnInfo, ColumnStatistics,
    FileBasedLayout, FileInfo, LayoutInfo, PhysicalInspectOptions, PhysicalInspector,
    PhysicalMetadata, RowGroupLayout, RowGroupMetadata, SchemaInfo, StatisticsInfo,
    UnstructuredLayout, VerbosityLevel,
};
pub use view_builder::{InspectionView, InspectionViewBuilder, ViewItem, ViewSection};

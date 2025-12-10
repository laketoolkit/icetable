//! Sistema de renderizado de cajas para terminal con soporte lazy

mod box_container;
mod box_item;
mod box_layout;
mod box_renderer;
mod box_section;
pub mod formatter;
mod icons;
mod inspect_formatter;
mod snapshot_formatter;

// Re-exports públicos
pub use box_container::Box;
pub use box_item::BoxItem;
pub use box_layout::{BoxLayout, BoxStyle};
pub use box_renderer::BoxRenderer;
pub use box_section::{BoxSection, SectionStyle};
pub use formatter::{OutputFormatter, create_styled_table, create_header_cell, create_header_cells, format_datetime_utc, format_timestamp_ms};
pub use icons::{SeverityIcon, StatusIcon};
pub use inspect_formatter::InspectionFormatter;
pub use snapshot_formatter::{SnapshotFormatter, SnapshotInfo};

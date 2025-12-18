//! Sistema de renderizado de cajas para terminal con soporte lazy

mod admin_formatter;
mod analyze_formatter;
mod box_container;
mod box_item;
mod box_layout;
mod box_renderer;
mod box_section;
mod cli_output_impls;
mod config_formatter;
mod diff_formatter;
mod doctor_formatter;
pub mod formatter;
mod history_formatter;
mod icons;
mod inspect_formatter;
mod ls_formatter;
mod optimize_formatter;
mod refs_formatter;
mod repair_formatter;
mod snapshot_formatter;
mod stats_formatter;
mod vacuum_formatter;

// Re-exports públicos
pub use admin_formatter::{AdminFormatter, AuthStatusInfo, WarehouseInfo};
pub use analyze_formatter::AnalyzeFormatter;
pub use box_container::Box;
pub use box_item::BoxItem;
pub use box_layout::{BoxLayout, BoxStyle};
pub use box_renderer::BoxRenderer;
pub use box_section::{BoxSection, SectionStyle};
pub use cli_output_impls::ListOutput;
pub use config_formatter::{ConfigCatalogInfo, ConfigFormatter, ConfigTableInfo};
pub use diff_formatter::DiffFormatter;
pub use doctor_formatter::DoctorFormatter;
pub use formatter::{
    CliOutput, OutputFormatter, create_header_cell, create_header_cells, create_styled_table,
    format_datetime_utc, format_timestamp_ms, output_result,
};
pub use history_formatter::{HistoryEntryInfo, HistoryFormatter};
pub use icons::{SeverityIcon, StatusIcon};
pub use inspect_formatter::InspectionFormatter;
pub use ls_formatter::{LsFormatter, LsRefInfo, LsSnapshotInfo};
pub use optimize_formatter::OptimizeFormatter;
pub use refs_formatter::{RefInfo as FormatterRefInfo, RefsFormatter};
pub use repair_formatter::RepairFormatter;
pub use snapshot_formatter::{LineageEntry, SnapshotFormatter, SnapshotInfo};
pub use stats_formatter::StatsFormatter;
pub use vacuum_formatter::{OrphanFileInfo, VacuumFormatter};

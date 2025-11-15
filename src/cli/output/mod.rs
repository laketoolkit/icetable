//! Sistema de renderizado de cajas para terminal con soporte lazy

mod box_container;
mod box_item;
mod box_layout;
mod box_renderer;
mod box_section;
pub mod formatter;

// Re-exports públicos
pub use box_container::Box;
pub use box_item::BoxItem;
pub use box_layout::{BoxLayout, BoxStyle};
pub use box_renderer::BoxRenderer;
pub use box_section::{BoxSection, SectionStyle};
pub use formatter::{OutputFormatter, SeverityIcon, StatusIcon};

//! Box container that holds sections for rendering
//!
//! A box is a visual container with an optional title and multiple sections.

use super::box_section::BoxSection;

/// A complete box with title and sections
#[derive(Debug)]
pub struct Box {
    /// Main box title (centered in top border)
    title: Option<String>,

    /// Sections within the box
    sections: Vec<BoxSection>,
}

impl Box {
    /// Creates a new box with title
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            sections: Vec::new(),
        }
    }

    /// Creates a box without title
    pub fn new() -> Self {
        Self {
            title: None,
            sections: Vec::new(),
        }
    }

    /// Adds a section
    pub fn section(mut self, section: BoxSection) -> Self {
        self.sections.push(section);
        self
    }

    /// Adds multiple sections
    pub fn sections(mut self, new_sections: impl IntoIterator<Item = BoxSection>) -> Self {
        self.sections.extend(new_sections);
        self
    }

    /// Gets the box title
    pub(crate) fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Consumes and returns the sections
    pub(crate) fn into_sections(self) -> Vec<BoxSection> {
        self.sections
    }
}

impl Default for Box {
    fn default() -> Self {
        Self::new()
    }
}

//! Box layout configuration for terminal rendering
//!
//! Defines the layout parameters (width, border style) for box rendering.

/// Layout configuration for box rendering
#[derive(Debug, Clone)]
pub struct BoxLayout {
    /// Total box width (including borders)
    total_width: usize,
    /// Border character style
    style: BoxStyle,
}

/// Box border style options
#[derive(Debug, Clone, Copy)]
pub enum BoxStyle {
    /// Rounded borders (╭─╮│╰╯)
    Rounded,
    /// Square borders (┌─┐│└┘)
    Square,
}

impl BoxLayout {
    /// Creates a new layout with specified width
    pub fn new(total_width: usize) -> Self {
        Self {
            total_width,
            style: BoxStyle::Rounded,
        }
    }

    /// Sets the border style
    pub fn with_style(mut self, style: BoxStyle) -> Self {
        self.style = style;
        self
    }

    /// Total width including borders
    pub fn total_width(&self) -> usize {
        self.total_width
    }

    /// Available width for content (without borders)
    /// total_width - 2 (border chars: │ left + │ right)
    pub fn content_width(&self) -> usize {
        self.total_width.saturating_sub(2)
    }

    /// Available width for lines with internal padding
    /// total_width - 3 (borders + leading space)
    pub fn line_width(&self) -> usize {
        self.total_width.saturating_sub(3)
    }

    /// Gets border characters according to style
    pub(crate) fn chars(&self) -> BoxChars {
        match self.style {
            BoxStyle::Rounded => BoxChars {
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
                horizontal: '─',
                vertical: '│',
                left_t: '├',
                right_t: '┤',
            },
            BoxStyle::Square => BoxChars {
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
                horizontal: '─',
                vertical: '│',
                left_t: '├',
                right_t: '┤',
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BoxChars {
    pub(crate) top_left: char,
    pub(crate) top_right: char,
    pub(crate) bottom_left: char,
    pub(crate) bottom_right: char,
    pub(crate) horizontal: char,
    pub(crate) vertical: char,
    pub(crate) left_t: char,
    pub(crate) right_t: char,
}

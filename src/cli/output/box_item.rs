//! Box item types for content within sections
//!
//! Items are the individual content elements (text, key-value pairs, separators)
//! that appear within box sections.

/// An individual element in a box section
#[derive(Debug, Clone)]
pub enum BoxItem {
    /// Simple text line (may include ANSI codes)
    Text(String),

    /// Key-value pair with alignment
    KeyValue {
        /// The key portion of the key-value pair
        key: String,
        /// The value portion of the key-value pair
        value: String,
        /// Key alignment width (None = no extra padding)
        key_width: Option<usize>,
    },

    /// Horizontal separator
    Separator,

    /// Empty line
    Empty,
}

impl BoxItem {
    /// Creates a simple text item
    pub fn text(s: impl Into<String>) -> Self {
        BoxItem::Text(s.into())
    }

    /// Creates a key-value item without specific alignment
    pub fn kv(key: impl Into<String>, value: impl Into<String>) -> Self {
        BoxItem::KeyValue {
            key: key.into(),
            value: value.into(),
            key_width: None,
        }
    }

    /// Creates a key-value item with specific key width for alignment
    pub fn kv_aligned(key: impl Into<String>, value: impl Into<String>, key_width: usize) -> Self {
        BoxItem::KeyValue {
            key: key.into(),
            value: value.into(),
            key_width: Some(key_width),
        }
    }

    /// Creates a horizontal separator
    pub fn separator() -> Self {
        BoxItem::Separator
    }

    /// Creates an empty line
    pub fn empty() -> Self {
        BoxItem::Empty
    }
}

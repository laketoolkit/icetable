//! Box sections for grouping related content
//!
//! Sections organize items within a box and can have their own title and style.

use super::box_item::BoxItem;

/// Item source for a section
enum ItemSource {
    /// Items evaluated eagerly (in memory)
    Eager(Vec<BoxItem>),
    /// Lazy items (via iterator)
    Lazy(Box<dyn Iterator<Item = BoxItem>>),
}

impl std::fmt::Debug for ItemSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ItemSource::Eager(items) => f.debug_tuple("Eager").field(items).finish(),
            ItemSource::Lazy(_) => f.debug_tuple("Lazy").field(&"<iterator>").finish(),
        }
    }
}

/// Color style for sections
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionStyle {
    /// No special style
    Default,
    /// Bold title
    Bold,
    /// Dimmed/gray title
    Dimmed,
    /// Info style (cyan)
    Info,
    /// Success state (green)
    Success,
    /// Warning state (yellow)
    Warning,
    /// Error state (red)
    Error,
}

/// A section within a box with optional header
#[derive(Debug)]
pub struct BoxSection {
    /// Section title (None = no title)
    title: Option<String>,

    /// Items in this section
    source: ItemSource,

    /// Whether to include separator after title
    separator_after_title: bool,

    /// Section title style
    title_style: SectionStyle,
}

impl BoxSection {
    /// Creates a section with title
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            source: ItemSource::Eager(Vec::new()),
            separator_after_title: true,
            title_style: SectionStyle::Bold,
        }
    }

    /// Creates a section without title
    pub fn untitled() -> Self {
        Self {
            title: None,
            source: ItemSource::Eager(Vec::new()),
            separator_after_title: false,
            title_style: SectionStyle::Default,
        }
    }

    /// Adds an item to the section (eager mode only)
    ///
    /// If the section is lazy, converts to eager first.
    pub fn item(mut self, item: BoxItem) -> Self {
        // Convert lazy to eager if needed
        if let ItemSource::Lazy(iter) =
            std::mem::replace(&mut self.source, ItemSource::Eager(Vec::new()))
        {
            self.source = ItemSource::Eager(iter.collect());
        }
        if let ItemSource::Eager(items) = &mut self.source {
            items.push(item);
        }
        self
    }

    /// Adds multiple items eagerly
    ///
    /// If the section is lazy, converts to eager first.
    pub fn items(mut self, new_items: impl IntoIterator<Item = BoxItem>) -> Self {
        // Convert lazy to eager if needed
        if let ItemSource::Lazy(iter) =
            std::mem::replace(&mut self.source, ItemSource::Eager(Vec::new()))
        {
            self.source = ItemSource::Eager(iter.collect());
        }
        if let ItemSource::Eager(items) = &mut self.source {
            items.extend(new_items);
        }
        self
    }

    /// Sets items using a lazy iterator
    pub fn items_iter<I>(mut self, iter: I) -> Self
    where
        I: Iterator<Item = BoxItem> + 'static,
    {
        self.source = ItemSource::Lazy(Box::new(iter));
        self
    }

    /// Sets whether to show separator after title
    pub fn separator_after_title(mut self, value: bool) -> Self {
        self.separator_after_title = value;
        self
    }

    /// Sets the title style
    pub fn style(mut self, style: SectionStyle) -> Self {
        self.title_style = style;
        self
    }

    /// Gets the section title
    pub(crate) fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Gets the title style
    pub(crate) fn title_style(&self) -> SectionStyle {
        self.title_style
    }

    /// Checks if separator should be shown after title
    pub(crate) fn has_separator_after_title(&self) -> bool {
        self.separator_after_title
    }

    /// Converts to item iterator (consumes section)
    pub(crate) fn into_items(self) -> Box<dyn Iterator<Item = BoxItem>> {
        match self.source {
            ItemSource::Eager(items) => Box::new(items.into_iter()),
            ItemSource::Lazy(iter) => iter,
        }
    }
}

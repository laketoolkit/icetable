//! Box rendering engine for terminal output
//!
//! Handles the rendering of box components with proper ANSI code handling,
//! Unicode width calculations, and text wrapping.

use super::box_container::Box;
use super::box_item::BoxItem;
use super::box_layout::{BoxChars, BoxLayout};
use super::box_section::SectionStyle;
use colored::Colorize;
use unicode_width::UnicodeWidthStr;

/// Color constants for key-value rendering (Tailwind color palette)
mod colors {
    /// Teal color for keys (Tailwind teal-400)
    pub const KEY_COLOR: (u8, u8, u8) = (45, 212, 191);
    /// Slate color for values (Tailwind slate-300)
    pub const VALUE_COLOR: (u8, u8, u8) = (203, 213, 225);
}

/// Pre-computed key formatting parts for efficient rendering
struct KeyParts {
    /// Plain text version (for width calculation)
    plain: String,
    /// Colored version (for display)
    colored: String,
    /// Indent for continuation lines (aligned with value start)
    value_indent: usize,
}

/// Box renderer with ANSI code handling
pub struct BoxRenderer {
    layout: BoxLayout,
}

impl BoxRenderer {
    /// Creates a new renderer with the given layout
    pub fn new(layout: BoxLayout) -> Self {
        Self { layout }
    }

    /// Renders a complete box
    pub fn render(&self, boxed: Box) -> String {
        let mut lines = Vec::new();
        let chars = self.layout.chars();

        // Top border with title
        if let Some(title) = boxed.title() {
            lines.push(self.render_title_border(title, chars));
        } else {
            lines.push(self.render_top_border(chars));
        }

        // Render sections
        let sections = boxed.into_sections();
        let num_sections = sections.len();
        for (idx, section) in sections.into_iter().enumerate() {
            // Section title if present
            if let Some(title) = section.title() {
                let styled = Self::apply_style(title.to_string(), section.title_style());
                lines.push(self.render_line(&format!(" {}", styled), chars));

                if section.has_separator_after_title() {
                    lines.push(self.render_separator(chars));
                }
            }

            // Items
            for item in section.into_items() {
                match item {
                    BoxItem::Text(text) => {
                        lines.push(self.render_line(&format!(" {}", text), chars));
                    }
                    BoxItem::KeyValue {
                        key,
                        value,
                        key_width,
                    } => {
                        let kv_lines = self.render_key_value(&key, &value, key_width, chars);
                        lines.push(kv_lines);
                    }
                    BoxItem::Separator => {
                        lines.push(self.render_separator(chars));
                    }
                    BoxItem::Empty => {
                        lines.push(self.render_empty_line(chars));
                    }
                }
            }

            // Separator between sections (except last)
            if idx < num_sections - 1 {
                lines.push(self.render_separator(chars));
            }
        }

        // Bottom border
        lines.push(self.render_bottom_border(chars));

        lines.join("\n")
    }

    /// Renders a box as a line iterator
    /// For TUI - allows consuming line by line
    pub fn render_iter(&self, boxed: Box) -> impl Iterator<Item = String> {
        self.render(boxed)
            .lines()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// Renders top border with centered title
    fn render_title_border(&self, title: &str, chars: BoxChars) -> String {
        let title_with_spaces = format!(" {} ", title);
        let title_width = Self::visual_width(&title_with_spaces);
        let content_width = self.layout.content_width();

        let padding_total = content_width.saturating_sub(title_width);
        let padding_left = padding_total / 2;
        let padding_right = padding_total - padding_left;

        format!(
            "{}{}{}{}{}",
            chars.top_left,
            chars.horizontal.to_string().repeat(padding_left),
            title_with_spaces,
            chars.horizontal.to_string().repeat(padding_right),
            chars.top_right
        )
    }

    /// Renders top border without title
    fn render_top_border(&self, chars: BoxChars) -> String {
        format!(
            "{}{}{}",
            chars.top_left,
            chars
                .horizontal
                .to_string()
                .repeat(self.layout.content_width()),
            chars.top_right
        )
    }

    /// Renders bottom border
    fn render_bottom_border(&self, chars: BoxChars) -> String {
        format!(
            "{}{}{}",
            chars.bottom_left,
            chars
                .horizontal
                .to_string()
                .repeat(self.layout.content_width()),
            chars.bottom_right
        )
    }

    /// Renders horizontal separator
    fn render_separator(&self, chars: BoxChars) -> String {
        format!(
            "{}{}{}",
            chars.left_t,
            chars
                .horizontal
                .to_string()
                .repeat(self.layout.content_width()),
            chars.right_t
        )
    }

    /// Renders a key-value pair with value-aligned wrapping
    fn render_key_value(
        &self,
        key: &str,
        value: &str,
        key_width: Option<usize>,
        chars: BoxChars,
    ) -> String {
        let available_width = self.layout.content_width();
        let key_parts = Self::format_key_parts(key, key_width);

        // Check if single line fits
        let first_line_plain = format!("{}{}", key_parts.plain, value);
        if Self::visual_width(&first_line_plain) <= available_width {
            return self.render_single_line_kv(&key_parts, value, available_width, chars);
        }

        // Need wrapping
        self.render_wrapped_kv(&key_parts, value, available_width, chars)
    }

    /// Formats key with optional width, returns both plain and colored versions
    fn format_key_parts(key: &str, key_width: Option<usize>) -> KeyParts {
        let (r, g, b) = colors::KEY_COLOR;
        let (plain, colored) = if let Some(width) = key_width {
            let plain = format!(" {:<width$} ", key, width = width);
            let colored_key = format!("{:<width$}", key, width = width).truecolor(r, g, b);
            let colored = format!(" {} ", colored_key);
            (plain, colored)
        } else {
            let plain = format!(" {}: ", key);
            let colored_key = format!("{}:", key).truecolor(r, g, b);
            let colored = format!(" {} ", colored_key);
            (plain, colored)
        };

        let actual_width = key_width.unwrap_or_else(|| {
            // Strip the leading space and trailing space to get key width
            plain.trim().len()
        });

        KeyParts {
            plain,
            colored,
            value_indent: 1 + actual_width + 1,
        }
    }

    /// Renders key-value that fits on a single line
    fn render_single_line_kv(
        &self,
        key_parts: &KeyParts,
        value: &str,
        available_width: usize,
        chars: BoxChars,
    ) -> String {
        let (r, g, b) = colors::VALUE_COLOR;
        let colored_value = value.truecolor(r, g, b);
        let line_colored = format!("{}{}", key_parts.colored, colored_value);
        let line_plain = format!("{}{}", key_parts.plain, value);
        let padding = available_width.saturating_sub(Self::visual_width(&line_plain));

        format!(
            "{}{}{}{}",
            chars.vertical,
            line_colored,
            " ".repeat(padding),
            chars.vertical
        )
    }

    /// Renders key-value with wrapped value lines
    fn render_wrapped_kv(
        &self,
        key_parts: &KeyParts,
        value: &str,
        available_width: usize,
        chars: BoxChars,
    ) -> String {
        let key_part_width = Self::visual_width(&key_parts.plain);
        let value_space = available_width.saturating_sub(key_part_width);
        let continuation_width = available_width.saturating_sub(key_parts.value_indent);

        let value_lines = Self::wrap_value(value, value_space, continuation_width);
        let (r, g, b) = colors::VALUE_COLOR;

        let mut result_lines = Vec::with_capacity(value_lines.len());

        for (i, val_line) in value_lines.iter().enumerate() {
            let colored_val = val_line.truecolor(r, g, b);

            let (line, line_plain) = if i == 0 {
                // First line: key + value
                (
                    format!("{}{}", key_parts.colored, colored_val),
                    format!("{}{}", key_parts.plain, val_line),
                )
            } else {
                // Continuation: indent + value
                let indent = " ".repeat(key_parts.value_indent);
                (
                    format!("{}{}", indent, colored_val),
                    format!("{}{}", indent, val_line),
                )
            };

            let padding = available_width.saturating_sub(Self::visual_width(&line_plain));
            result_lines.push(format!(
                "{}{}{}{}",
                chars.vertical,
                line,
                " ".repeat(padding),
                chars.vertical
            ));
        }

        result_lines.join("\n")
    }

    /// Wrap a value string, with different width for first line vs continuation
    fn wrap_value(value: &str, first_line_width: usize, continuation_width: usize) -> Vec<String> {
        if Self::visual_width(value) <= first_line_width {
            return vec![value.to_string()];
        }

        let mut lines = Vec::new();
        let mut current_line = String::new();
        let mut current_width = 0;
        let mut is_first_line = true;

        let max_width = |first: bool| {
            if first {
                first_line_width
            } else {
                continuation_width
            }
        };

        for ch in value.chars() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);

            if current_width + ch_width > max_width(is_first_line) && !current_line.is_empty() {
                lines.push(current_line);
                current_line = String::new();
                current_width = 0;
                is_first_line = false;
            }

            current_line.push(ch);
            current_width += ch_width;
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        lines
    }

    /// Renders a content line with padding, wrapping if necessary
    fn render_line(&self, content: &str, chars: BoxChars) -> String {
        let available_width = self.layout.content_width();
        let wrapped_lines = Self::wrap_text(content, available_width);

        wrapped_lines
            .into_iter()
            .map(|line| {
                let line_width = Self::visual_width(&line);
                let padding = available_width.saturating_sub(line_width);
                format!(
                    "{}{}{}{}",
                    chars.vertical,
                    line,
                    " ".repeat(padding),
                    chars.vertical
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Wrap text to fit within max_width, preserving leading spaces for continuation
    fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
        let text_width = Self::visual_width(text);
        if text_width <= max_width {
            return vec![text.to_string()];
        }

        // Find leading whitespace to preserve indentation on wrapped lines
        let leading_spaces = text.len() - text.trim_start().len();
        let indent = " ".repeat(leading_spaces.min(20)); // Cap indent at 20

        let mut lines = Vec::new();
        let mut current_line = String::new();
        let mut current_width = 0;
        let mut is_first_line = true;

        for ch in text.chars() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);

            if current_width + ch_width > max_width && !current_line.is_empty() {
                lines.push(current_line);
                current_line = if is_first_line {
                    is_first_line = false;
                    indent.clone()
                } else {
                    indent.clone()
                };
                current_width = Self::visual_width(&current_line);
            }

            current_line.push(ch);
            current_width += ch_width;
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        lines
    }

    /// Renders an empty line
    fn render_empty_line(&self, chars: BoxChars) -> String {
        format!(
            "{}{}{}",
            chars.vertical,
            " ".repeat(self.layout.content_width()),
            chars.vertical
        )
    }

    /// Calculates visual width handling ANSI codes
    fn visual_width(s: &str) -> usize {
        let stripped_bytes = strip_ansi_escapes::strip(s);
        let stripped = String::from_utf8_lossy(&stripped_bytes);
        UnicodeWidthStr::width(stripped.as_ref())
    }

    /// Applies style to a string according to SectionStyle
    fn apply_style(s: String, style: SectionStyle) -> String {
        match style {
            SectionStyle::Default => s,
            SectionStyle::Bold => s.bold().to_string(),
            SectionStyle::Dimmed => s.dimmed().to_string(),
            SectionStyle::Info => s.cyan().to_string(),
            SectionStyle::Success => s.green().to_string(),
            SectionStyle::Warning => s.yellow().to_string(),
            SectionStyle::Error => s.red().to_string(),
        }
    }
}

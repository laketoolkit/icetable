use super::box_container::Box;
use super::box_item::BoxItem;
use super::box_layout::{BoxChars, BoxLayout};
use super::box_section::SectionStyle;
use colored::{ColoredString, Colorize};
use unicode_width::UnicodeWidthStr;

/// Renderizador de cajas con manejo de ANSI codes
pub struct BoxRenderer {
    layout: BoxLayout,
}

impl BoxRenderer {
    /// Crea un nuevo renderer con layout dado
    pub fn new(layout: BoxLayout) -> Self {
        Self { layout }
    }

    /// Renderiza una caja completa
    pub fn render(&self, boxed: Box) -> String {
        let mut lines = Vec::new();
        let chars = self.layout.chars();

        // Top border con título
        if let Some(title) = boxed.title() {
            lines.push(self.render_title_border(title, chars));
        } else {
            lines.push(self.render_top_border(chars));
        }

        // Renderizar secciones
        let sections = boxed.into_sections();
        let num_sections = sections.len();
        for (idx, section) in sections.into_iter().enumerate() {
            // Título de sección si existe
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

            // Separador entre secciones (excepto última)
            if idx < num_sections - 1 {
                lines.push(self.render_separator(chars));
            }
        }

        // Bottom border
        lines.push(self.render_bottom_border(chars));

        lines.join("\n")
    }

    /// Renderiza una caja como iterador de líneas
    /// Para TUI - permite consumir línea por línea
    pub fn render_iter(&self, boxed: Box) -> impl Iterator<Item = String> {
        self.render(boxed)
            .lines()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// Renderiza borde superior con título centrado
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

    /// Renderiza borde superior sin título
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

    /// Renderiza borde inferior
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

    /// Renderiza separador horizontal
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

    /// Renderiza un key-value con wrap alineado al valor
    fn render_key_value(
        &self,
        key: &str,
        value: &str,
        key_width: Option<usize>,
        chars: BoxChars,
    ) -> String {
        let available_width = self.layout.content_width();

        // Apply colors: key in teal (45, 212, 191), value in slate (148, 163, 184)
        let colored_key: ColoredString = if let Some(width) = key_width {
            format!("{:<width$}", key, width = width).truecolor(45, 212, 191)
        } else {
            format!("{}:", key).truecolor(45, 212, 191)
        };

        // Calculate the indent for continuation lines (aligned with value start)
        // Format: " {key:<width>} {value}" -> indent = 1 + width + 1
        let actual_key_width = key_width.unwrap_or_else(|| Self::visual_width(key) + 1); // +1 for ":"
        let value_indent = 1 + actual_key_width + 1; // leading space + key + space before value

        // First line with key (for width calculation, use uncolored)
        let first_line_plain = if let Some(width) = key_width {
            format!(" {:<width$} {}", key, value, width = width)
        } else {
            format!(" {}: {}", key, value)
        };

        let first_line_width = Self::visual_width(&first_line_plain);

        // If it fits, just render normally
        if first_line_width <= available_width {
            // Slate color (203, 213, 225) - Tailwind slate-300
            let colored_value = value.truecolor(203, 213, 225);
            let first_line_colored = format!(" {} {}", colored_key, colored_value);
            let padding = available_width.saturating_sub(first_line_width);
            return format!(
                "{}{}{}{}",
                chars.vertical,
                first_line_colored,
                " ".repeat(padding),
                chars.vertical
            );
        }

        // Need to wrap - calculate how much of value fits on first line
        let key_part_plain = if let Some(width) = key_width {
            format!(" {:<width$} ", key, width = width)
        } else {
            format!(" {}: ", key)
        };
        let key_part_colored = format!(" {} ", colored_key);
        let key_part_width = Self::visual_width(&key_part_plain);
        let value_space = available_width.saturating_sub(key_part_width);

        // Wrap the value
        let value_lines = Self::wrap_value(value, value_space, available_width - value_indent);

        let mut result_lines = Vec::new();

        for (i, val_line) in value_lines.iter().enumerate() {
            // Slate color (203, 213, 225) - Tailwind slate-300
            let colored_val_line = val_line.truecolor(203, 213, 225);
            if i == 0 {
                // First line: key + value
                let line = format!("{}{}", key_part_colored, colored_val_line);
                let line_plain = format!("{}{}", key_part_plain, val_line);
                let line_width = Self::visual_width(&line_plain);
                let padding = available_width.saturating_sub(line_width);
                result_lines.push(format!(
                    "{}{}{}{}",
                    chars.vertical,
                    line,
                    " ".repeat(padding),
                    chars.vertical
                ));
            } else {
                // Continuation: indent + value
                let line = format!("{}{}", " ".repeat(value_indent), colored_val_line);
                let line_width =
                    Self::visual_width(&format!("{}{}", " ".repeat(value_indent), val_line));
                let padding = available_width.saturating_sub(line_width);
                result_lines.push(format!(
                    "{}{}{}{}",
                    chars.vertical,
                    line,
                    " ".repeat(padding),
                    chars.vertical
                ));
            }
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

    /// Renderiza una línea de contenido con padding, haciendo wrap si es necesario
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

    /// Renderiza línea vacía
    fn render_empty_line(&self, chars: BoxChars) -> String {
        format!(
            "{}{}{}",
            chars.vertical,
            " ".repeat(self.layout.content_width()),
            chars.vertical
        )
    }

    /// Calcula ancho visual manejando ANSI codes
    fn visual_width(s: &str) -> usize {
        let stripped_bytes = strip_ansi_escapes::strip(s);
        let stripped = String::from_utf8_lossy(&stripped_bytes);
        UnicodeWidthStr::width(stripped.as_ref())
    }

    /// Aplica estilo a un string según el SectionStyle
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

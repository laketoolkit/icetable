use super::box_container::Box;
use super::box_item::BoxItem;
use super::box_layout::{BoxChars, BoxLayout};
use super::box_section::SectionStyle;
use colored::Colorize;
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
                        let rendered = if let Some(width) = key_width {
                            format!(" {:<width$} {}", key, value, width = width)
                        } else {
                            format!(" {}: {}", key, value)
                        };
                        lines.push(self.render_line(&rendered, chars));
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

    /// Renderiza una línea de contenido con padding
    fn render_line(&self, content: &str, chars: BoxChars) -> String {
        let content_visual_width = Self::visual_width(content);
        let available_width = self.layout.content_width();
        let padding = available_width.saturating_sub(content_visual_width);

        format!(
            "{}{}{}{}",
            chars.vertical,
            content,
            " ".repeat(padding),
            chars.vertical
        )
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

/// Configuración de diseño para renderizado de cajas
#[derive(Debug, Clone)]
pub struct BoxLayout {
    /// Ancho total de la caja (incluyendo bordes)
    total_width: usize,
    /// Estilo de caracteres para bordes
    style: BoxStyle,
}

/// Box border style options
#[derive(Debug, Clone, Copy)]
pub enum BoxStyle {
    /// Bordes redondeados (╭─╮│╰╯)
    Rounded,
    /// Bordes rectos (┌─┐│└┘)
    Square,
}

impl BoxLayout {
    /// Crea un nuevo layout con ancho especificado
    pub fn new(total_width: usize) -> Self {
        Self {
            total_width,
            style: BoxStyle::Rounded,
        }
    }

    /// Establece el estilo de bordes
    pub fn with_style(mut self, style: BoxStyle) -> Self {
        self.style = style;
        self
    }

    /// Ancho total incluyendo bordes
    pub fn total_width(&self) -> usize {
        self.total_width
    }

    /// Ancho disponible para contenido (sin bordes)
    /// total_width - 2 (border chars: │ left + │ right)
    pub fn content_width(&self) -> usize {
        self.total_width.saturating_sub(2)
    }

    /// Ancho disponible para líneas con padding interno
    /// total_width - 3 (borders + leading space)
    pub fn line_width(&self) -> usize {
        self.total_width.saturating_sub(3)
    }

    /// Obtiene caracteres de borde según el estilo
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

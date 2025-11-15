use super::box_item::BoxItem;

/// Fuente de items para una sección
enum ItemSource {
    /// Items evaluados eagerly (en memoria)
    Eager(Vec<BoxItem>),
    /// Items lazy (via iterador)
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

/// Estilo de color para secciones
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionStyle {
    /// Sin estilo especial
    Default,
    /// Título en negrita
    Bold,
    /// Título en dimmed/gris
    Dimmed,
    /// Color personalizado
    Info, // Cyan
    Success, // Green
    Warning, // Yellow
    Error,   // Red
}

/// Una sección dentro de una caja con header opcional
#[derive(Debug)]
pub struct BoxSection {
    /// Título de la sección (None = sin título)
    title: Option<String>,

    /// Items en esta sección
    source: ItemSource,

    /// Si debe incluir separador después del título
    separator_after_title: bool,

    /// Estilo del título de la sección
    title_style: SectionStyle,
}

impl BoxSection {
    /// Crea una sección con título
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            source: ItemSource::Eager(Vec::new()),
            separator_after_title: true,
            title_style: SectionStyle::Bold,
        }
    }

    /// Crea una sección sin título
    pub fn untitled() -> Self {
        Self {
            title: None,
            source: ItemSource::Eager(Vec::new()),
            separator_after_title: false,
            title_style: SectionStyle::Default,
        }
    }

    /// Agrega un item a la sección (solo para modo eager)
    pub fn item(mut self, item: BoxItem) -> Self {
        match &mut self.source {
            ItemSource::Eager(items) => {
                items.push(item);
            }
            ItemSource::Lazy(_) => {
                panic!("Cannot add individual items to a lazy section");
            }
        }
        self
    }

    /// Agrega múltiples items eagerly
    pub fn items(mut self, new_items: impl IntoIterator<Item = BoxItem>) -> Self {
        match &mut self.source {
            ItemSource::Eager(items) => {
                items.extend(new_items);
            }
            ItemSource::Lazy(_) => {
                panic!("Cannot add items to a lazy section");
            }
        }
        self
    }

    /// Establece los items usando un iterador lazy
    pub fn items_iter<I>(mut self, iter: I) -> Self
    where
        I: Iterator<Item = BoxItem> + 'static,
    {
        self.source = ItemSource::Lazy(Box::new(iter));
        self
    }

    /// Establece si debe haber separador después del título
    pub fn separator_after_title(mut self, value: bool) -> Self {
        self.separator_after_title = value;
        self
    }

    /// Establece el estilo del título
    pub fn style(mut self, style: SectionStyle) -> Self {
        self.title_style = style;
        self
    }

    /// Obtiene el título de la sección
    pub(crate) fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Obtiene el estilo del título
    pub(crate) fn title_style(&self) -> SectionStyle {
        self.title_style
    }

    /// Verifica si debe mostrar separador después del título
    pub(crate) fn has_separator_after_title(&self) -> bool {
        self.separator_after_title
    }

    /// Convierte a iterador de items (consume la sección)
    pub(crate) fn into_items(self) -> Box<dyn Iterator<Item = BoxItem>> {
        match self.source {
            ItemSource::Eager(items) => Box::new(items.into_iter()),
            ItemSource::Lazy(iter) => iter,
        }
    }
}

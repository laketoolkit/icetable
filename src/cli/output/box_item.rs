/// Un elemento individual en una sección de caja
#[derive(Debug, Clone)]
pub enum BoxItem {
    /// Línea de texto simple (puede incluir ANSI codes)
    Text(String),

    /// Par clave-valor con alineación
    KeyValue {
        /// The key portion of the key-value pair
        key: String,
        /// The value portion of the key-value pair
        value: String,
        /// Ancho de alineación para la clave (None = sin padding extra)
        key_width: Option<usize>,
    },

    /// Separador horizontal
    Separator,

    /// Línea vacía
    Empty,
}

impl BoxItem {
    /// Crea un item de texto simple
    pub fn text(s: impl Into<String>) -> Self {
        BoxItem::Text(s.into())
    }

    /// Crea un item key-value sin alineación específica
    pub fn kv(key: impl Into<String>, value: impl Into<String>) -> Self {
        BoxItem::KeyValue {
            key: key.into(),
            value: value.into(),
            key_width: None,
        }
    }

    /// Crea un item key-value con ancho de clave específico para alineación
    pub fn kv_aligned(key: impl Into<String>, value: impl Into<String>, key_width: usize) -> Self {
        BoxItem::KeyValue {
            key: key.into(),
            value: value.into(),
            key_width: Some(key_width),
        }
    }

    /// Crea un separador horizontal
    pub fn separator() -> Self {
        BoxItem::Separator
    }

    /// Crea una línea vacía
    pub fn empty() -> Self {
        BoxItem::Empty
    }
}

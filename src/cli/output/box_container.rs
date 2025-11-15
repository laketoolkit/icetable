use super::box_section::BoxSection;

/// Una caja completa con título y secciones
#[derive(Debug)]
pub struct Box {
    /// Título principal de la caja (centrado en borde superior)
    title: Option<String>,

    /// Secciones dentro de la caja
    sections: Vec<BoxSection>,
}

impl Box {
    /// Crea una caja nueva con título
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            sections: Vec::new(),
        }
    }

    /// Crea una caja sin título
    pub fn new() -> Self {
        Self {
            title: None,
            sections: Vec::new(),
        }
    }

    /// Agrega una sección
    pub fn section(mut self, section: BoxSection) -> Self {
        self.sections.push(section);
        self
    }

    /// Agrega múltiples secciones
    pub fn sections(mut self, new_sections: impl IntoIterator<Item = BoxSection>) -> Self {
        self.sections.extend(new_sections);
        self
    }

    /// Obtiene el título de la caja
    pub(crate) fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Consume y retorna las secciones
    pub(crate) fn into_sections(self) -> Vec<BoxSection> {
        self.sections
    }
}

impl Default for Box {
    fn default() -> Self {
        Self::new()
    }
}

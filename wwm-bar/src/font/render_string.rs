use super::{loader::FontEncodedChunk, FontDrawer};

#[derive(Debug, Clone)]
pub struct RenderString {
    pub chunks: Vec<FontEncodedChunk>,
    pub width: i16,
    pub height: u16,
    pub vpad: u16,
    pub hpad: u16,
}

impl RenderString {
    pub fn new(drawer: &FontDrawer, text: impl ToString) -> Self {
        let text = text.to_string();
        let (width, height) = drawer.font.geometry(&text);
        let chunks = drawer.font.encode(&text, width - 1);
        Self {
            chunks,
            width,
            height,
            vpad: 0,
            hpad: 0,
        }
    }

    pub fn pad(mut self, pad: u16) -> Self {
        self.hpad = pad;
        self.vpad = pad;
        self
    }

    pub fn box_dimensions(&self) -> (u16, u16) {
        (
            self.width as u16 + (self.hpad * 2),
            self.height + (self.vpad * 2),
        )
    }
}

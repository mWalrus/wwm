use super::{color, primitives::WRect};
use x11rb::protocol::render::Color;

#[derive(Default, Debug, Clone, Copy)]
pub struct WBarOptions {
    pub rect: WRect,
    pub padding: u16,
    pub section_padding: i16,
    pub tag_count: usize,
    pub tag_width: u16,
    pub colors: WBarColors,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct WBarColors {
    pub fg: (u32, Color),
    pub bg: (u32, Color),
    pub selected_fg: (u32, Color),
    pub selected_bg: (u32, Color),
}

impl WBarColors {
    pub fn new(fg: u32, bg: u32, selected_fg: u32, selected_bg: u32) -> Self {
        Self {
            fg: (fg, color::hex_to_rgba(fg)),
            bg: (bg, color::hex_to_rgba(bg)),
            selected_fg: (selected_fg, color::hex_to_rgba(selected_fg)),
            selected_bg: (selected_bg, color::hex_to_rgba(selected_bg)),
        }
    }
}

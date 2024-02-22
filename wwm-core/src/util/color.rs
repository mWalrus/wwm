use x11rb::protocol::render::Color;

pub fn hex_to_rgba(hex: u32) -> Color {
    Color {
        red: ((hex >> 16 & 0xff) as u16) << 8,
        green: ((hex >> 8 & 0xff) as u16) << 8,
        blue: ((hex & 0xff) as u16) << 8,
        // NOTE: no transparency support
        alpha: 0xffff,
    }
}

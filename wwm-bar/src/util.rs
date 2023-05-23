use x11rb::protocol::{render::Color, xproto::Rectangle};

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn new(x: i16, y: i16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }

    pub fn has_pointer(&self, px: i16, py: i16) -> bool {
        let has_x = px >= self.x && px <= self.x + self.w as i16;
        let has_y = py >= self.y && py <= self.y + self.h as i16;
        has_x && has_y
    }
}

impl From<Rect> for Rectangle {
    fn from(r: Rect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
        }
    }
}

impl From<Rectangle> for Rect {
    fn from(r: Rectangle) -> Self {
        Rect::new(r.x, r.y, r.width, r.height)
    }
}

pub fn hex_to_rgba_color(hex: u32) -> Color {
    let red = ((hex >> 16 & 0xff) as u16) << 8;
    let green = ((hex >> 8 & 0xff) as u16) << 8;
    let blue = ((hex & 0xff) as u16) << 8;

    Color {
        red,
        green,
        blue,
        alpha: 0xffff,
    }
}

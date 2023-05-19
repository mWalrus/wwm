use x11rb::protocol::xproto::Rectangle;

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

use x11rb::protocol::xproto::{ConfigureWindowAux, GetGeometryReply, MotionNotifyEvent, Rectangle};

#[derive(Default, Debug, Clone, Copy)]
pub struct WRect {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

impl From<&GetGeometryReply> for WRect {
    fn from(g: &GetGeometryReply) -> Self {
        Self {
            x: g.x,
            y: g.y,
            w: g.width,
            h: g.height,
        }
    }
}

impl From<WRect> for ConfigureWindowAux {
    fn from(cr: WRect) -> Self {
        ConfigureWindowAux::new()
            .x(cr.x as i32)
            .y(cr.y as i32)
            .width(cr.w as u32)
            .height(cr.h as u32)
    }
}

impl From<WRect> for Rectangle {
    fn from(r: WRect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
        }
    }
}

impl From<Rectangle> for WRect {
    fn from(r: Rectangle) -> Self {
        Self::new(r.x, r.y, r.width, r.height)
    }
}

impl WRect {
    pub fn new(x: i16, y: i16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            w: width,
            h: height,
        }
    }

    pub fn has_pointer(&self, px: i16, py: i16) -> bool {
        let has_x = px >= self.x && px <= self.x + self.w as i16;
        let has_y = py >= self.y && py <= self.y + self.h as i16;
        has_x && has_y
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct WSize {
    pub w: u16,
    pub h: u16,
}

impl WSize {
    pub fn from(size_hint: Option<(i32, i32)>) -> Option<Self> {
        if let Some((w, h)) = size_hint {
            return Some(Self {
                w: w as u16,
                h: h as u16,
            });
        }
        None
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WPos {
    pub x: i16,
    pub y: i16,
}

impl From<WRect> for WPos {
    fn from(value: WRect) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

impl From<&MotionNotifyEvent> for WPos {
    fn from(value: &MotionNotifyEvent) -> Self {
        Self {
            x: value.event_x,
            y: value.event_y,
        }
    }
}

impl WPos {
    pub fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }
}

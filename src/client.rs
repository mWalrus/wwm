use x11rb::protocol::xproto::{ConfigureWindowAux, GetGeometryReply, Window};

use crate::monitor::Monitor;

#[derive(Debug)]
pub struct ClientRect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl From<&GetGeometryReply> for ClientRect {
    fn from(g: &GetGeometryReply) -> Self {
        Self {
            x: g.x,
            y: g.y,
            width: g.width,
            height: g.height,
        }
    }
}

impl From<&Monitor> for ClientRect {
    fn from(m: &Monitor) -> Self {
        Self {
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
        }
    }
}

impl From<ClientRect> for ConfigureWindowAux {
    fn from(cr: ClientRect) -> Self {
        ConfigureWindowAux::new()
            .x(cr.x as i32)
            .y(cr.y as i32)
            .width(cr.width as u32)
            .height(cr.height as u32)
    }
}

impl ClientRect {
    pub fn new(x: i16, y: i16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug)]
pub struct ClientState {
    pub window: Window,
    pub frame: Window,
    pub rect: ClientRect,
    pub border_width: u16,
    pub has_focus: bool,
}

impl ClientState {
    pub fn new(window: Window, frame: Window, geom: &GetGeometryReply) -> Self {
        Self {
            window,
            frame,
            rect: geom.into(),
            border_width: 1,
            has_focus: false,
        }
    }
}

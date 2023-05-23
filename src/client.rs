use x11rb::{
    connection::Connection,
    protocol::xproto::{ConfigureWindowAux, GetGeometryReply, Window},
};

use crate::monitor::WMonitor;

#[derive(Default, Debug, Clone, Copy)]
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

impl<C: Connection> From<&WMonitor<'_, C>> for ClientRect {
    fn from(m: &WMonitor<C>) -> Self {
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

#[derive(Default, Debug, Clone, Copy)]
pub struct WClientState {
    pub window: Window,
    pub rect: ClientRect,
    pub is_floating: bool,
    pub is_fullscreen: bool,
}

impl WClientState {
    pub fn new(window: Window, rect: ClientRect, is_floating: bool, is_fullscreen: bool) -> Self {
        Self {
            window,
            rect,
            is_floating,
            is_fullscreen,
        }
    }
}

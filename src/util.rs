use crate::config;

use thiserror::Error;
use x11rb::protocol::xproto::{ConfigureWindowAux, GetGeometryReply, MotionNotifyEvent};

#[derive(Default, Debug, Clone, Copy)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

impl From<&GetGeometryReply> for Rect {
    fn from(g: &GetGeometryReply) -> Self {
        Self {
            x: g.x,
            y: g.y,
            w: g.width,
            h: g.height,
        }
    }
}

impl From<Rect> for ConfigureWindowAux {
    fn from(cr: Rect) -> Self {
        ConfigureWindowAux::new()
            .x(cr.x as i32)
            .y(cr.y as i32)
            .width(cr.w as u32)
            .height(cr.h as u32)
    }
}

impl Rect {
    pub fn new(x: i16, y: i16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            w: width,
            h: height,
        }
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Size {
    pub w: u16,
    pub h: u16,
}

impl From<(i32, i32)> for Size {
    fn from((w, h): (i32, i32)) -> Self {
        Self {
            w: w as u16,
            h: h as u16,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Pos {
    pub x: i16,
    pub y: i16,
}

impl From<Rect> for Pos {
    fn from(value: Rect) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

impl From<&MotionNotifyEvent> for Pos {
    fn from(value: &MotionNotifyEvent) -> Self {
        Self {
            x: value.event_x,
            y: value.event_y,
        }
    }
}

impl Pos {
    pub fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum WDirection {
    Prev,
    Next,
}

#[derive(Error, Debug)]
pub enum StateError {
    #[error("{0} is out of bounds")]
    Bounds(usize),
}

pub fn cmd_bits(cmd: &'static [&'static str]) -> Option<(&'static str, &'static [&'static str])> {
    if cmd.is_empty() {
        return None;
    }

    if cmd.len() == 1 {
        return Some((cmd[0], &[]));
    }

    let (cmd, args) = cmd.split_at(1);
    Some((cmd[0], args))
}

pub const fn bar_height() -> u16 {
    config::theme::bar::FONT_SIZE as u16 + (config::theme::bar::PADDING * 2)
}

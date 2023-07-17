use x11rb::protocol::xproto::Window;

use crate::{
    config::theme::window::BORDER_WIDTH,
    util::{Rect, Size},
};

#[derive(Default, Debug, Clone, Copy)]
pub struct WClientState {
    pub window: Window,
    pub rect: Rect,
    pub is_floating: bool,
    pub is_fullscreen: bool,
    pub is_fixed: bool,
    pub hints_valid: bool,
    pub bw: u16,
    pub base_size: Size,
    pub min_size: Size,
    pub max_size: Size,
    pub inc_size: Size,
    pub maxa: f32,
    pub mina: f32,
}

impl WClientState {
    pub fn new(window: Window, rect: Rect, is_floating: bool, is_fullscreen: bool) -> Self {
        Self {
            window,
            rect,
            is_floating,
            is_fullscreen,
            is_fixed: false,
            hints_valid: false,
            bw: BORDER_WIDTH,
            base_size: Size::default(),
            min_size: Size::default(),
            max_size: Size::default(),
            inc_size: Size::default(),
            maxa: 0f32,
            mina: 0f32,
        }
    }

    pub fn width(&self) -> u16 {
        self.rect.w + self.bw * 2
    }

    pub fn height(&self) -> u16 {
        self.rect.h + self.bw * 2
    }
}

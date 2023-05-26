use x11rb::protocol::xproto::Window;

use crate::util::Rect;

#[derive(Default, Debug, Clone, Copy)]
pub struct WClientState {
    pub window: Window,
    pub rect: Rect,
    pub is_floating: bool,
    pub is_fullscreen: bool,
}

impl WClientState {
    pub fn new(window: Window, rect: Rect, is_floating: bool, is_fullscreen: bool) -> Self {
        Self {
            window,
            rect,
            is_floating,
            is_fullscreen,
        }
    }
}

pub mod theme {
    pub static WINDOW_BORDER_FOCUSED: u32 = 0xca9ee6;
    pub static WINDOW_BORDER_UNFOCUSED: u32 = 0x3b3b3b;
}

pub mod keymap {
    use x11rb::protocol::xproto::Button;

    pub static DRAG_BUTTON: Button = 1;
}

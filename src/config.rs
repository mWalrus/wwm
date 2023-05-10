pub mod theme {
    pub static WINDOW_BORDER_FOCUSED: u32 = 0xca9ee6;
    pub static WINDOW_BORDER_UNFOCUSED: u32 = 0x3b3b3b;
}

pub mod window {
    pub static MAIN_CLIENT_WIDTH_PERCENTAGE: f32 = 0.55;
}

pub mod workspaces {
    pub const WORKSPACE_CAP: usize = 9;
}

pub mod mouse {
    use x11rb::protocol::xproto::Button;

    pub static DRAG_BUTTON: Button = 1;
}

pub mod commands {
    use crate::keyboard::keybind::WCommand;
    use crate::keyboard::keybind::WKeybind;
    use x11rb::protocol::xproto::ModMask;
    use xkbcommon::xkb::keysyms as ks;

    pub const MOD: ModMask = ModMask::M1;
    pub const SHIFT: ModMask = ModMask::SHIFT;

    // spawn commands
    static TERM_CMD: &[&str] = &["alacritty"];

    #[rustfmt::skip]
    pub fn setup_keybinds() -> Vec<WKeybind> {
        vec![
            WKeybind::new(MOD | SHIFT, ks::KEY_Return, WCommand::Spawn(TERM_CMD)),
            WKeybind::new(MOD | SHIFT, ks::KEY_k,      WCommand::MoveClientPrev),
            WKeybind::new(MOD | SHIFT, ks::KEY_j,      WCommand::MoveClientNext),
            WKeybind::new(MOD | SHIFT, ks::KEY_q,      WCommand::Destroy),
            WKeybind::new(MOD,         ks::KEY_j,      WCommand::FocusClientNext),
            WKeybind::new(MOD,         ks::KEY_k,      WCommand::FocusClientPrev),
            WKeybind::new(MOD,         ks::KEY_h,      WCommand::FocusMonitorPrev),
            WKeybind::new(MOD,         ks::KEY_l,      WCommand::FocusMonitorNext),
            WKeybind::new(MOD,         ks::KEY_q,      WCommand::Exit),
        ]
    }
}

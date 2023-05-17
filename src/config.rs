pub mod theme {
    pub static WINDOW_BORDER_FOCUSED: u32 = 0xca9ee6;
    pub static WINDOW_BORDER_UNFOCUSED: u32 = 0x51576d;
}

pub mod workspaces {
    pub const WORKSPACE_CAP: usize = 9;
    pub const MAIN_CLIENT_WIDTH_PERCENTAGE: f32 = 0.55;
    pub const WIDTH_ADJUSTMENT_FACTOR: f32 = 0.02;
    pub const CLIENT_BORDER_WIDTH: u16 = 1;
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
    static CHATTERINO_CMD: &[&str] = &["chatterino"];
    static XEYES_CMD: &[&str] = &["xeyes"];

    #[rustfmt::skip]
    pub fn setup_keybinds() -> Vec<WKeybind> {
        vec![
            WKeybind::new(MOD | SHIFT, ks::KEY_Return, WCommand::Spawn(TERM_CMD)),
            WKeybind::new(MOD,         ks::KEY_c,      WCommand::Spawn(CHATTERINO_CMD)),
            WKeybind::new(MOD,         ks::KEY_x,      WCommand::Spawn(XEYES_CMD)),
            WKeybind::new(MOD | SHIFT, ks::KEY_k,      WCommand::MoveClientPrev),
            WKeybind::new(MOD | SHIFT, ks::KEY_j,      WCommand::MoveClientNext),
            WKeybind::new(MOD | SHIFT, ks::KEY_q,      WCommand::Destroy),
            WKeybind::new(MOD | SHIFT, ks::KEY_h,      WCommand::DecreaseMainWidth),
            WKeybind::new(MOD | SHIFT, ks::KEY_l,      WCommand::IncreaseMainWidth),
            WKeybind::new(MOD,         ks::KEY_j,      WCommand::FocusClientNext),
            WKeybind::new(MOD,         ks::KEY_k,      WCommand::FocusClientPrev),
            WKeybind::new(MOD,         ks::KEY_h,      WCommand::FocusMonitorPrev),
            WKeybind::new(MOD,         ks::KEY_l,      WCommand::FocusMonitorNext),
            WKeybind::new(MOD,         ks::KEY_q,      WCommand::Exit),
            // workspace keybinds
            WKeybind::new(MOD,         ks::KEY_1,      WCommand::SelectWorkspace(0)),
            WKeybind::new(MOD,         ks::KEY_2,      WCommand::SelectWorkspace(1)),
            WKeybind::new(MOD,         ks::KEY_3,      WCommand::SelectWorkspace(2)),
            WKeybind::new(MOD,         ks::KEY_4,      WCommand::SelectWorkspace(3)),
            WKeybind::new(MOD,         ks::KEY_5,      WCommand::SelectWorkspace(4)),
            WKeybind::new(MOD,         ks::KEY_6,      WCommand::SelectWorkspace(5)),
            WKeybind::new(MOD,         ks::KEY_7,      WCommand::SelectWorkspace(6)),
            WKeybind::new(MOD,         ks::KEY_8,      WCommand::SelectWorkspace(7)),
            WKeybind::new(MOD,         ks::KEY_9,      WCommand::SelectWorkspace(8)),
        ]
    }
}

pub mod auto_start {
    #[rustfmt::skip]
    pub static AUTO_START_COMMANDS: &[&[&str]] = &[
        &["feh", "--bg-scale", "/usr/share/dwm/wallpaper.png"]
    ];
}

pub mod theme {
    pub mod window {
        pub const BORDER_FOCUSED: u32 = 0xca9ee6;
        pub const BORDER_UNFOCUSED: u32 = 0x51576d;
        pub const BORDER_WIDTH: u16 = 1;
    }

    pub mod bar {
        pub const BG_SELECTED: u32 = 0xca9ee6;
        pub const BG: u32 = 0x232634;
        pub const FG_SELECTED: u32 = 0x232634;
        pub const FG: u32 = 0xc6d0f5;
        pub const FONT_SIZE: f32 = 13.;
        pub const PADDING: u16 = 5;
        pub const FONT: &str = "monospace";
    }
}

pub mod workspaces {
    pub const WORKSPACE_CAP: usize = 9;
    pub const MAIN_CLIENT_WIDTH_PERCENTAGE: f32 = 0.55;
    pub const WIDTH_ADJUSTMENT_FACTOR: f32 = 0.02;
}

pub mod mouse {
    use x11rb::protocol::xproto::Button;

    pub static DRAG_BUTTON: Button = 1;
}

pub mod commands {
    use crate::keyboard::keybind::WCommand;
    use crate::keyboard::keybind::WKeybind;
    use crate::layouts::WLayout;
    use crate::util::StackDirection;
    use x11rb::protocol::xproto::ModMask;
    use xkbcommon::xkb::keysyms as ks;

    const MOD: ModMask = ModMask::M1;
    const SHIFT: ModMask = ModMask::SHIFT;
    const NONE: u16 = 0;

    // spawn commands
    static TERM_CMD: &[&str] = &["alacritty"];
    static CHATTERINO_CMD: &[&str] = &["chatterino"];
    static XEYES_CMD: &[&str] = &["xeyes"];
    static FLAMESHOT_CMD: &[&str] = &["flameshot", "gui"];

    #[rustfmt::skip]
    pub fn setup_keybinds() -> Vec<WKeybind> {
        vec![
            WKeybind::new(MOD | SHIFT, ks::KEY_Return, WCommand::Spawn(TERM_CMD)),
            WKeybind::new(MOD,         ks::KEY_c,      WCommand::Spawn(CHATTERINO_CMD)),
            WKeybind::new(MOD,         ks::KEY_x,      WCommand::Spawn(XEYES_CMD)),
            WKeybind::new(NONE,        ks::KEY_Print,  WCommand::Spawn(FLAMESHOT_CMD)),
            WKeybind::new(MOD | SHIFT, ks::KEY_k,      WCommand::MoveClientPrev),
            WKeybind::new(MOD | SHIFT, ks::KEY_j,      WCommand::MoveClientNext),
            WKeybind::new(MOD | SHIFT, ks::KEY_q,      WCommand::Destroy),
            WKeybind::new(MOD | SHIFT, ks::KEY_h,      WCommand::DecreaseMainWidth),
            WKeybind::new(MOD | SHIFT, ks::KEY_l,      WCommand::IncreaseMainWidth),
            WKeybind::new(MOD | SHIFT, ks::KEY_t,      WCommand::Layout(WLayout::Tile)),
            WKeybind::new(MOD | SHIFT, ks::KEY_c,      WCommand::Layout(WLayout::Column)),
            WKeybind::new(MOD | SHIFT, ks::KEY_comma,  WCommand::MoveClientToMonitor(StackDirection::Prev)),
            WKeybind::new(MOD | SHIFT, ks::KEY_period, WCommand::MoveClientToMonitor(StackDirection::Next)),
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
            // move to workspace keybinds
            WKeybind::new(MOD | SHIFT, ks::KEY_1,      WCommand::MoveClientToWorkspace(0)),
            WKeybind::new(MOD | SHIFT, ks::KEY_2,      WCommand::MoveClientToWorkspace(1)),
            WKeybind::new(MOD | SHIFT, ks::KEY_3,      WCommand::MoveClientToWorkspace(2)),
            WKeybind::new(MOD | SHIFT, ks::KEY_4,      WCommand::MoveClientToWorkspace(3)),
            WKeybind::new(MOD | SHIFT, ks::KEY_5,      WCommand::MoveClientToWorkspace(4)),
            WKeybind::new(MOD | SHIFT, ks::KEY_6,      WCommand::MoveClientToWorkspace(5)),
            WKeybind::new(MOD | SHIFT, ks::KEY_7,      WCommand::MoveClientToWorkspace(6)),
            WKeybind::new(MOD | SHIFT, ks::KEY_8,      WCommand::MoveClientToWorkspace(7)),
            WKeybind::new(MOD | SHIFT, ks::KEY_9,      WCommand::MoveClientToWorkspace(8)),
        ]
    }
}

pub mod auto_start {
    #[rustfmt::skip]
    pub static AUTO_START_COMMANDS: &[&[&str]] = &[
        &["feh", "--bg-scale", "/usr/share/wwm/wallpaper.png"]
    ];
}

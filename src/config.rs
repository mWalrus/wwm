pub mod theme {
    pub mod window {
        pub const BORDER_FOCUSED: u32 = 0xca9ee6;
        pub const BORDER_UNFOCUSED: u32 = 0x51576d;
        pub const BORDER_WIDTH: u16 = 1;
    }

    pub mod bar {
        // these selected colors are used for workspace tags in the bar
        pub const BG_SELECTED: u32 = 0xca9ee6;
        pub const FG_SELECTED: u32 = 0x232634;
        // these colors are the default fore-/background colors used across the entire bar
        pub const BG: u32 = 0x232634;
        pub const FG: u32 = 0xc6d0f5;
        // font size in pixels
        pub const FONT_SIZE: f32 = 13.0;
        // padding between sections in the bar in pixels.
        // so: tags and layout indicator will have 10px between them
        pub const SECTION_PADDING: i16 = 10;
        // top/bottom padding around the text in the bar in pixles
        pub const PADDING: u16 = 5;
        // Only the name of the font family is required. Wwm uses fontconfig to discover
        // a monospaced font in that family and uses that for drawing text.
        pub const FONT: &str = "";
    }
}

pub mod workspaces {
    pub const WORKSPACE_CAP: usize = 9;
    // how much of the monitor width the main client occupies in the main-stack layout
    pub const MAIN_CLIENT_WIDTH_PERCENTAGE: f32 = 0.55;
    // how much the main client's width is adjusted when resizing with keybinds
    pub const WIDTH_ADJUSTMENT_FACTOR: f32 = 0.02;
}

pub mod mouse {
    use x11rb::protocol::xproto::{ButtonIndex, ModMask};

    use crate::{command::WMouseCommand, mouse::WMouseBind};

    const MOD: ModMask = ModMask::M1;

    pub const DRAG_BUTTON: ButtonIndex = ButtonIndex::M1; // left mouse button
    pub const RESIZE_BUTTON: ButtonIndex = ButtonIndex::M3; // right mouse button

    pub fn setup_mousebinds() -> Vec<WMouseBind> {
        vec![
            WMouseBind::new(MOD, DRAG_BUTTON, WMouseCommand::DragClient),
            WMouseBind::new(MOD, RESIZE_BUTTON, WMouseCommand::ResizeClient),
        ]
    }
}

pub mod commands {
    use crate::command::WKeyCommand;
    use crate::keyboard::keybind::WKeybind;
    use crate::layouts::WLayout;
    use crate::util::WDirection;
    use x11rb::protocol::xproto::ModMask;
    use xkbcommon::xkb::keysyms as ks;

    const MOD: ModMask = ModMask::M1;
    const SHIFT: ModMask = ModMask::SHIFT;
    const NONE: u16 = 0;

    // spawn commands
    static TERM_CMD: &[&str] = &["alacritty"];
    static CHATTERINO_CMD: &[&str] = &["chatterino"];
    static FLAMESHOT_CMD: &[&str] = &["flameshot", "gui"];

    #[rustfmt::skip]
    pub fn setup_keybinds() -> Vec<WKeybind> {
        vec![
            WKeybind::new(MOD | SHIFT, ks::KEY_Return, WKeyCommand::Spawn(TERM_CMD)),
            WKeybind::new(MOD,         ks::KEY_c,      WKeyCommand::Spawn(CHATTERINO_CMD)),
            WKeybind::new(NONE,        ks::KEY_Print,  WKeyCommand::Spawn(FLAMESHOT_CMD)),
            WKeybind::new(MOD | SHIFT, ks::KEY_k,      WKeyCommand::MoveClient(WDirection::Prev)),
            WKeybind::new(MOD | SHIFT, ks::KEY_j,      WKeyCommand::MoveClient(WDirection::Next)),
            WKeybind::new(MOD | SHIFT, ks::KEY_q,      WKeyCommand::Destroy),
            WKeybind::new(MOD | SHIFT, ks::KEY_h,      WKeyCommand::AdjustMainWidth(WDirection::Prev)),
            WKeybind::new(MOD | SHIFT, ks::KEY_l,      WKeyCommand::AdjustMainWidth(WDirection::Next)),
            WKeybind::new(MOD | SHIFT, ks::KEY_t,      WKeyCommand::Layout(WLayout::MainStack)),
            WKeybind::new(MOD | SHIFT, ks::KEY_c,      WKeyCommand::Layout(WLayout::Column)),
            WKeybind::new(MOD | SHIFT, ks::KEY_comma,  WKeyCommand::MoveClientToMonitor(WDirection::Prev)),
            WKeybind::new(MOD | SHIFT, ks::KEY_period, WKeyCommand::MoveClientToMonitor(WDirection::Next)),
            WKeybind::new(MOD,         ks::KEY_j,      WKeyCommand::FocusClient(WDirection::Next)),
            WKeybind::new(MOD,         ks::KEY_k,      WKeyCommand::FocusClient(WDirection::Prev)),
            WKeybind::new(MOD,         ks::KEY_h,      WKeyCommand::FocusMonitor(WDirection::Prev)),
            WKeybind::new(MOD,         ks::KEY_l,      WKeyCommand::FocusMonitor(WDirection::Next)),
            WKeybind::new(MOD | SHIFT, ks::KEY_space,  WKeyCommand::UnFloat),
            WKeybind::new(MOD,         ks::KEY_q,      WKeyCommand::Exit),
            // BEGIN: workspace keybinds
            WKeybind::new(MOD,         ks::KEY_1,      WKeyCommand::SelectWorkspace(0)),
            WKeybind::new(MOD,         ks::KEY_2,      WKeyCommand::SelectWorkspace(1)),
            WKeybind::new(MOD,         ks::KEY_3,      WKeyCommand::SelectWorkspace(2)),
            WKeybind::new(MOD,         ks::KEY_4,      WKeyCommand::SelectWorkspace(3)),
            WKeybind::new(MOD,         ks::KEY_5,      WKeyCommand::SelectWorkspace(4)),
            WKeybind::new(MOD,         ks::KEY_6,      WKeyCommand::SelectWorkspace(5)),
            WKeybind::new(MOD,         ks::KEY_7,      WKeyCommand::SelectWorkspace(6)),
            WKeybind::new(MOD,         ks::KEY_8,      WKeyCommand::SelectWorkspace(7)),
            WKeybind::new(MOD,         ks::KEY_9,      WKeyCommand::SelectWorkspace(8)),
            WKeybind::new(MOD | SHIFT, ks::KEY_1,      WKeyCommand::MoveClientToWorkspace(0)),
            WKeybind::new(MOD | SHIFT, ks::KEY_2,      WKeyCommand::MoveClientToWorkspace(1)),
            WKeybind::new(MOD | SHIFT, ks::KEY_3,      WKeyCommand::MoveClientToWorkspace(2)),
            WKeybind::new(MOD | SHIFT, ks::KEY_4,      WKeyCommand::MoveClientToWorkspace(3)),
            WKeybind::new(MOD | SHIFT, ks::KEY_5,      WKeyCommand::MoveClientToWorkspace(4)),
            WKeybind::new(MOD | SHIFT, ks::KEY_6,      WKeyCommand::MoveClientToWorkspace(5)),
            WKeybind::new(MOD | SHIFT, ks::KEY_7,      WKeyCommand::MoveClientToWorkspace(6)),
            WKeybind::new(MOD | SHIFT, ks::KEY_8,      WKeyCommand::MoveClientToWorkspace(7)),
            WKeybind::new(MOD | SHIFT, ks::KEY_9,      WKeyCommand::MoveClientToWorkspace(8)),
            // END: workspace keybinds
        ]
    }
}

pub mod auto_start {
    #[rustfmt::skip]
    pub static AUTO_START_COMMANDS: &[&[&str]] = &[
        &["feh", "--bg-scale", "/usr/share/wwm/wallpaper.png"]
    ];
}

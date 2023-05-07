pub mod theme {
    pub static WINDOW_BORDER_FOCUSED: u32 = 0xca9ee6;
    pub static WINDOW_BORDER_UNFOCUSED: u32 = 0x3b3b3b;
}

pub mod window {
    pub static MAIN_CLIENT_WIDTH_PERCENTAGE: f32 = 0.55;
}

pub mod mouse {
    use x11rb::protocol::xproto::Button;

    pub static DRAG_BUTTON: Button = 1;
}

pub mod commands {
    use x11rb::protocol::xproto::{KeyButMask, ModMask};
    use xkbcommon::xkb::keysyms as ks;

    pub const MOD: ModMask = ModMask::M1;
    pub const SHIFT: ModMask = ModMask::SHIFT;

    type CommandSeq = &'static [&'static str];

    static TERM_CMD: CommandSeq = &["alacritty"];

    #[derive(Debug)]
    pub struct WKeybind {
        pub mods: ModMask,
        pub keysym: u32,
        pub action: WCommand,
    }

    impl WKeybind {
        pub fn new(mods: ModMask, keysym: u32, action: WCommand) -> Self {
            Self {
                mods,
                keysym,
                action,
            }
        }

        pub fn mods_as_key_but_mask(&self) -> KeyButMask {
            KeyButMask::from(u16::from(self.mods))
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub enum WCommand {
        Spawn(&'static [&'static str]),
        Destroy,
        FocusUp,
        FocusDown,
        MoveUp,
        MoveDown,
        Exit,
        PassThrough,
    }

    #[rustfmt::skip]
    pub fn setup_keybinds() -> Vec<WKeybind> {
        vec![
            WKeybind::new(MOD | SHIFT, ks::KEY_Return, WCommand::Spawn(TERM_CMD)),
            WKeybind::new(MOD | SHIFT, ks::KEY_k,      WCommand::MoveUp),
            WKeybind::new(MOD | SHIFT, ks::KEY_j,      WCommand::MoveDown),
            WKeybind::new(MOD | SHIFT, ks::KEY_q,      WCommand::Destroy),
            WKeybind::new(MOD,         ks::KEY_j,      WCommand::FocusDown),
            WKeybind::new(MOD,         ks::KEY_k,      WCommand::FocusUp),
            WKeybind::new(MOD,         ks::KEY_q,      WCommand::Exit),
        ]
    }
}

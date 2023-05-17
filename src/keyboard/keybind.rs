use x11rb::protocol::xproto::{KeyButMask, ModMask};

#[derive(Debug)]
pub struct WKeybind {
    pub mods: ModMask,
    pub keysym: u32,
    pub action: WCommand,
}

impl WKeybind {
    pub fn new<M: Into<ModMask>>(mods: M, keysym: u32, action: WCommand) -> Self {
        Self {
            mods: mods.into(),
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
    Destroy,
    Exit,
    FocusClientNext,
    FocusClientPrev,
    MoveClientNext,
    MoveClientPrev,
    FocusMonitorNext,
    FocusMonitorPrev,
    Idle,
    IncreaseMainWidth,
    DecreaseMainWidth,
    SelectWorkspace(usize),
    Spawn(&'static [&'static str]),
}

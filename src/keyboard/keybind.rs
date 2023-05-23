use x11rb::protocol::xproto::{KeyButMask, ModMask};

use crate::command::WKeyCommand;

#[derive(Debug)]
pub struct WKeybind {
    pub mods: ModMask,
    pub keysym: u32,
    pub action: WKeyCommand,
}

impl WKeybind {
    pub fn new<M: Into<ModMask>>(mods: M, keysym: u32, action: WKeyCommand) -> Self {
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

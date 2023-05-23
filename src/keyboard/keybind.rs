use x11rb::protocol::xproto::{KeyButMask, ModMask};

use crate::{layouts::WLayout, util::WDirection};

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

#[derive(Debug)]
pub struct WMouseBind {
    pub mods: ModMask,
    pub button: u8,
    pub action: WCommand,
}

impl WMouseBind {
    pub fn new<M: Into<ModMask>>(mods: M, button: impl Into<u8>, action: WCommand) -> Self {
        Self {
            mods: mods.into(),
            button: button.into(),
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
    FocusClient(WDirection),
    MoveClient(WDirection),
    FocusMonitor(WDirection),
    DragClient,
    ResizeClient,
    Idle,
    AdjustMainWidth(WDirection),
    Layout(WLayout),
    SelectWorkspace(usize),
    Spawn(&'static [&'static str]),
    MoveClientToWorkspace(usize),
    MoveClientToMonitor(WDirection),
}

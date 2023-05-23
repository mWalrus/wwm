use crate::{command::WMouseCommand, config};
use x11rb::{
    connection::Connection,
    cursor::Handle as CursorHandle,
    protocol::xproto::{ButtonIndex, ConnectionExt, EventMask, GrabMode, KeyButMask, ModMask},
    resource_manager::new_from_default,
};

pub struct WCursors {
    pub normal: u32,
    pub resize: u32,
    pub r#move: u32,
}

impl WCursors {
    pub fn new<C: Connection>(conn: &C, screen_num: usize) -> Self {
        let resource_db = new_from_default(conn).unwrap();
        let cursor_handle = CursorHandle::new(conn, screen_num, &resource_db).unwrap();
        let cursor_handle = cursor_handle.reply().unwrap();
        Self {
            normal: cursor_handle.load_cursor(conn, "left_ptr").unwrap(),
            resize: cursor_handle.load_cursor(conn, "sizing").unwrap(),
            r#move: cursor_handle.load_cursor(conn, "fleur").unwrap(),
        }
    }
}

pub struct WMouse {
    pub binds: Vec<WMouseBind>,
    pub cursors: WCursors,
}

impl WMouse {
    pub fn new<'a, C: Connection>(conn: &'a C, screen_num: usize) -> Self {
        let screen = &conn.setup().roots[screen_num];

        let cursors = WCursors::new(conn, screen_num);

        let binds = config::mouse::setup_mousebinds();
        for bind in &binds {
            let cur = match bind.action {
                WMouseCommand::DragClient => cursors.r#move,
                WMouseCommand::ResizeClient => cursors.resize,
                _ => 0, // invalid
            };

            conn.grab_button(
                true,
                screen.root,
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                0u32,
                cur,
                ButtonIndex::from(bind.button),
                bind.mods,
            )
            .unwrap();
        }

        Self { binds, cursors }
    }
}

#[derive(Debug)]
pub struct WMouseBind {
    pub mods: ModMask,
    pub button: u8,
    pub action: WMouseCommand,
}

impl WMouseBind {
    pub fn new<M: Into<ModMask>>(mods: M, button: impl Into<u8>, action: WMouseCommand) -> Self {
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

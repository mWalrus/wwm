use x11rb::{
    connection::Connection,
    protocol::xproto::{ConfigureWindowAux, ConnectionExt, Window},
    xcb_ffi::ReplyOrIdError,
};

use crate::{
    client::WClientState,
    config::workspaces::MAIN_CLIENT_WIDTH_PERCENTAGE,
    layouts::WLayout,
    util::{ClientCell, WVec},
};

#[derive(Default, Debug)]
pub struct WWorkspace {
    pub clients: WVec<WClientState>,
    pub width_factor: f32,
    pub layout: WLayout,
}

pub enum StackDirection {
    Prev,
    Next,
}

impl WWorkspace {
    pub fn new() -> Self {
        Self {
            width_factor: MAIN_CLIENT_WIDTH_PERCENTAGE,
            ..Default::default()
        }
    }

    pub fn client_from_direction(&mut self, dir: StackDirection) -> Option<ClientCell> {
        let idx = match dir {
            StackDirection::Prev => self.clients.prev_index(true, true),
            StackDirection::Next => self.clients.next_index(true, true),
        };

        if idx.is_none() {
            return None;
        }

        self.focused_client()
    }

    pub fn find_client_by_win(&self, win: Window) -> Option<ClientCell> {
        self.clients.find(|c| {
            let c = c.borrow();
            c.window == win
        })
    }

    pub fn focus_from_win(&mut self, win: Window) -> Option<ClientCell> {
        self.clients.find_and_select(|c| c.borrow().window == win);
        self.clients.selected()
    }

    pub fn focus_neighbor(&mut self, dir: StackDirection) {
        if self.clients.is_empty() {
            return;
        }

        match dir {
            StackDirection::Prev => self.clients.prev_index(true, true),
            StackDirection::Next => self.clients.next_index(true, true),
        };
    }

    pub fn focused_client(&self) -> Option<ClientCell> {
        self.clients.selected()
    }

    pub fn has_client(&self, win: Window) -> bool {
        self.find_client_by_win(win).is_some()
    }

    pub fn hide_clients<C: Connection>(&self, conn: &C) -> Result<(), ReplyOrIdError> {
        for c in self.clients.inner().iter() {
            let c = c.borrow();
            let aux = ConfigureWindowAux::new().x(c.rect.width as i32 * -2);
            conn.configure_window(c.window, &aux)?;
        }
        conn.flush()?;
        Ok(())
    }

    pub fn push_client(&mut self, c: WClientState) {
        self.clients.push_and_select(c);
    }

    pub fn remove_focused(&mut self) {
        self.clients.remove_current()
    }

    pub fn swap_with_neighbor(&mut self, dir: StackDirection) {
        let idx = match dir {
            StackDirection::Prev => self.clients.prev_index(true, false),
            StackDirection::Next => self.clients.next_index(true, false),
        };

        if let Some(idx) = idx {
            self.clients.swap(idx).unwrap();
        }
    }
}

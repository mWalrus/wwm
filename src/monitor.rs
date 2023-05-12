use std::{cell::RefCell, rc::Rc};

use x11rb::protocol::{randr::MonitorInfo, xproto::Window};

use crate::{
    client::WClientState,
    config::workspaces::WORKSPACE_CAP,
    layouts::WLayout,
    util::{ClientCell, WVec},
};

#[derive(Default)]
pub struct WMonitor {
    pub primary: bool,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub workspaces: WVec<WWorkspace>,
}

impl From<&MonitorInfo> for WMonitor {
    fn from(mi: &MonitorInfo) -> Self {
        println!("Monitor: {mi:?}");
        let mut workspaces = Vec::with_capacity(WORKSPACE_CAP);
        for _ in 0..WORKSPACE_CAP {
            workspaces.push(WWorkspace::default());
        }
        let workspaces = WVec::new(workspaces, 0);
        Self {
            primary: mi.primary,
            x: mi.x,
            y: mi.y,
            width: mi.width,
            height: mi.height,
            workspaces,
        }
    }
}

impl WMonitor {
    pub fn next_workspace(&mut self, dir: StackDirection) -> Rc<RefCell<WWorkspace>> {
        match dir {
            StackDirection::Prev => self.workspaces.prev_index(true, true).unwrap(),
            StackDirection::Next => self.workspaces.next_index(true, true).unwrap(),
        };
        self.focused_workspace()
    }

    pub fn focused_workspace(&self) -> Rc<RefCell<WWorkspace>> {
        self.workspaces.selected().unwrap()
    }

    pub fn width_from_percentage(&self, p: f32) -> u16 {
        (self.width as f32 * p) as u16
    }

    pub fn client_height(&self, client_count: usize) -> u16 {
        self.height / client_count as u16
    }
}

#[derive(Default, Debug)]
pub struct WWorkspace {
    pub clients: WVec<WClientState>,
    pub layout: WLayout,
}

pub enum StackDirection {
    Prev,
    Next,
}

impl WWorkspace {
    pub fn has_client(&self, win: Window) -> bool {
        self.find_client_by_win(win).is_some()
    }
    pub fn find_client_by_win(&self, win: Window) -> Option<ClientCell> {
        self.clients.find(|c| {
            let c = c.borrow();
            c.frame == win || c.window == win
        })
    }
    pub fn focus_from_frame(&mut self, frame: Window) -> Option<ClientCell> {
        self.clients.find_and_select(|c| c.borrow().frame == frame);
        self.clients.selected()
    }

    pub fn focused_client(&self) -> Option<ClientCell> {
        self.clients.selected()
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

    pub fn swap_with_neighbor(&mut self, dir: StackDirection) {
        let idx = match dir {
            StackDirection::Prev => self.clients.prev_index(true, false),
            StackDirection::Next => self.clients.next_index(true, false),
        };

        if let Some(idx) = idx {
            self.clients.swap(idx).unwrap();
        }
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

    pub fn push_client(&mut self, c: WClientState) {
        self.clients.push_and_select(c);
    }

    pub fn remove_focused(&mut self) {
        self.clients.remove_current()
    }
}

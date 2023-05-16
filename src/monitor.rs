use std::{cell::RefCell, rc::Rc};

use x11rb::protocol::randr::MonitorInfo;

use crate::{
    config::workspaces::WORKSPACE_CAP,
    util::{StateError, WVec},
    workspace::{StackDirection, WWorkspace},
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
            workspaces.push(WWorkspace::new());
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

    pub fn is_focused_workspace(&self, idx: usize) -> bool {
        self.workspaces.index() == idx
    }

    pub fn focus_workspace_from_index(&mut self, idx: usize) -> Result<(), StateError> {
        self.workspaces.select(idx)
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

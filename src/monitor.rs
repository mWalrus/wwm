use x11rb::protocol::{randr::MonitorInfo, xproto::Window};

use crate::{client::ClientState, config::workspaces::WORKSPACE_CAP, layouts::WLayout};

#[derive(Default)]
pub struct WMonitor {
    pub primary: bool,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub workspaces: Vec<WWorkspace>,
    pub focused_workspace: usize,
}

impl From<&MonitorInfo> for WMonitor {
    fn from(mi: &MonitorInfo) -> Self {
        let mut workspaces = Vec::with_capacity(WORKSPACE_CAP);
        for _ in 0..WORKSPACE_CAP {
            workspaces.push(WWorkspace::default());
        }
        Self {
            primary: mi.primary,
            x: mi.x,
            y: mi.y,
            width: mi.width,
            height: mi.height,
            workspaces,
            focused_workspace: 0,
        }
    }
}

impl WMonitor {
    pub fn width_from_percentage(&self, p: f32) -> u16 {
        (self.width as f32 * p) as u16
    }

    pub fn client_height(&self, client_count: usize) -> u16 {
        self.height / client_count as u16
    }
}

#[derive(Default, Debug)]
pub struct WWorkspace {
    pub clients: Vec<ClientState>,
    pub focused_client: Option<usize>,
    pub layout: WLayout,
}

pub enum StackDirection {
    Prev,
    Next,
}

impl WWorkspace {
    pub fn set_focus_from_frame(&mut self, frame: Window) -> bool {
        if let Some(idx) = self.clients.iter().position(|c| c.frame == frame) {
            println!("found frame {frame} at client index {idx}");
            self.focused_client = Some(idx);
            return true;
        }
        false
    }

    pub fn focused_frame_and_idx(&self) -> Option<(Window, usize)> {
        if let Some(idx) = self.focused_client {
            return Some((self.clients[idx].frame, idx));
        }
        return None;
    }

    pub fn idx_from_direction(&self, idx: usize, dir: StackDirection) -> usize {
        match dir {
            StackDirection::Prev => {
                if idx == 0 {
                    self.clients.len() - 1
                } else {
                    idx - 1
                }
            }
            StackDirection::Next => {
                if idx == self.clients.len() - 1 {
                    0
                } else {
                    idx + 1
                }
            }
        }
    }

    pub fn swap(&mut self, first: usize, second: usize) {
        self.clients.swap(first, second);
    }

    pub fn set_focus(&mut self, idx: usize) {
        self.focused_client = Some(idx);
    }

    pub fn correct_focus(&mut self) {
        if self.clients.is_empty() {
            self.focused_client = None;
            return;
        }

        if let Some(idx) = self.focused_client {
            self.focused_client = Some(idx.min(self.clients.len().saturating_sub(1)))
        } else if !self.clients.is_empty() {
            // focus the first client if focused client is not set
            self.focused_client = Some(0);
        } else {
            self.focused_client = None;
        }
    }

    pub fn remove_focused(&mut self) {
        if let Some(idx) = self.focused_client {
            println!("focused client index: {idx}");
            println!("client count: {}", self.clients.len());
            self.clients.remove(idx);
            if self.clients.is_empty() {
                self.focused_client = None;
            } else if idx >= self.clients.len() {
                self.focused_client = Some(self.clients.len().saturating_sub(1));
            }
            println!("new focused client index: {:?}", self.focused_client);
        }
    }
}

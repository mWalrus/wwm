use std::{borrow::BorrowMut, cell::RefCell, rc::Rc};

use wwm_bar::{font::FontDrawer, visual::RenderVisualInfo, WBar};
use x11rb::{
    connection::Connection,
    protocol::{randr::MonitorInfo, xproto::Rectangle},
};

use crate::{
    client::WClientState,
    config::{theme, workspaces::WORKSPACE_CAP},
    util::{Pos, StateError, WDirection, WVec},
    workspace::WWorkspace,
};

pub struct WMonitor<'a, C: Connection> {
    pub conn: &'a C,
    pub bar: WBar,
    pub primary: bool,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub workspaces: WVec<WWorkspace>,
}

impl<'a, C: Connection> WMonitor<'a, C> {
    pub fn new(
        mi: &MonitorInfo,
        conn: &'a C,
        font_drawer: Rc<FontDrawer>,
        vis_info: Rc<RenderVisualInfo>,
    ) -> Self {
        let mut workspaces = Vec::with_capacity(WORKSPACE_CAP);
        for _ in 0..WORKSPACE_CAP {
            workspaces.push(WWorkspace::new());
        }
        let workspaces = WVec::new(workspaces, 0);
        let layout_symbol = workspaces.selected().unwrap().borrow().layout.to_string();

        let bar_rect = Rectangle {
            x: mi.x,
            y: mi.y,
            width: mi.width,
            height: theme::bar::FONT_SIZE as u16 + (theme::bar::PADDING * 2),
        };

        let y = bar_rect.y + bar_rect.height as i16;
        let height = mi.height - bar_rect.height;
        let bar = WBar::new(
            conn,
            font_drawer,
            vis_info,
            bar_rect,
            theme::bar::PADDING,
            theme::bar::SECTION_PADDING,
            WORKSPACE_CAP,
            layout_symbol,
            "",
            [
                theme::bar::FG,
                theme::bar::BG,
                theme::bar::BG_SELECTED,
                theme::bar::FG_SELECTED,
            ],
        );

        Self {
            conn,
            bar,
            primary: mi.primary,
            x: mi.x,
            y,
            width: mi.width,
            height,
            workspaces,
        }
    }

    pub fn client_height(&self, client_count: usize) -> u16 {
        self.height / client_count as u16
    }

    pub fn focus_workspace_from_index(&mut self, idx: usize) -> Result<(), StateError> {
        self.workspaces.select(idx)
    }

    pub fn focused_workspace(&self) -> Rc<RefCell<WWorkspace>> {
        self.workspaces.selected().unwrap()
    }

    pub fn has_pos(&self, p: Pos) -> bool {
        let has_x = p.x >= self.x && p.x <= self.x + self.width as i16;
        let has_y = p.y >= self.y && p.y <= self.y + self.height as i16;
        has_x && has_y
    }

    pub fn find_adjacent_monitor(&self, p: Pos) -> Option<WDirection> {
        if p.x < self.x {
            return Some(WDirection::Prev);
        } else if p.x > self.x + self.width as i16 {
            return Some(WDirection::Next);
        }
        None
    }

    pub fn is_focused_workspace(&self, idx: usize) -> bool {
        self.workspaces.index() == idx
    }

    pub fn next_workspace(&mut self, dir: WDirection) -> Rc<RefCell<WWorkspace>> {
        match dir {
            WDirection::Prev => self.workspaces.prev_index(true, true).unwrap(),
            WDirection::Next => self.workspaces.next_index(true, true).unwrap(),
        };
        self.focused_workspace()
    }

    pub fn add_client_to_workspace(&mut self, ws_idx: usize, client: WClientState) {
        if let Some(mut ws) = self.workspaces.get_mut(ws_idx) {
            let ws = ws.borrow_mut();
            ws.push_client(client);
            ws.hide_clients(self.conn).unwrap();
        }
    }

    pub fn width_from_percentage(&self, p: f32) -> u16 {
        (self.width as f32 * p) as u16
    }
}

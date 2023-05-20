use std::{cell::RefCell, rc::Rc};

use wwm_bar::{font::FontDrawer, visual::RenderVisualInfo, WBar};
use x11rb::{
    connection::Connection,
    protocol::{
        randr::MonitorInfo,
        xproto::{MotionNotifyEvent, Rectangle},
    },
};

use crate::{
    config::{theme, workspaces::WORKSPACE_CAP},
    util::{StateError, WVec},
    workspace::{StackDirection, WWorkspace},
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

    pub fn has_pointer(&self, e: &MotionNotifyEvent) -> bool {
        let has_x = e.event_x >= self.x && e.event_x <= self.x + self.width as i16;
        let has_y = e.event_y >= self.y && e.event_y <= self.y + self.height as i16;
        has_x && has_y
    }

    pub fn is_focused_workspace(&self, idx: usize) -> bool {
        self.workspaces.index() == idx
    }

    pub fn next_workspace(&mut self, dir: StackDirection) -> Rc<RefCell<WWorkspace>> {
        match dir {
            StackDirection::Prev => self.workspaces.prev_index(true, true).unwrap(),
            StackDirection::Next => self.workspaces.next_index(true, true).unwrap(),
        };
        self.focused_workspace()
    }

    pub fn width_from_percentage(&self, p: f32) -> u16 {
        (self.width as f32 * p) as u16
    }
}

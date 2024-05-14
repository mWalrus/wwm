use std::rc::Rc;

use thiserror::Error;
use wwm_bar::WBar;
use wwm_core::{
    text::TextRenderer,
    util::{
        bar::{WBarColors, WBarOptions},
        WConfigWindow, WLayout,
    },
};
use x11rb::{
    connection::Connection,
    protocol::{
        randr::MonitorInfo,
        xproto::{
            ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt, InputFocus, MotionNotifyEvent,
        },
    },
    xcb_ffi::{ReplyOrIdError, XCBConnection},
    CURRENT_TIME, NONE,
};

use crate::{
    client::WClientState,
    config::{
        tags::{MAIN_CLIENT_WIDTH_PERCENTAGE, TAG_CAP},
        theme,
    },
    X_HANDLE,
};
use crate::{command::WDirection, layouts::layout_clients};
use wwm_core::util::primitives::{WPos, WRect};

#[derive(Error, Debug)]
pub enum StateError {
    #[error("{0} is out of bounds")]
    Bounds(usize),
}

pub struct WMonitor<'a> {
    pub bar: WBar<'a, XCBConnection>,
    pub primary: bool,
    pub rect: WRect,
    pub clients: Vec<WClientState>,
    pub client: Option<usize>,
    pub layout: WLayout,
    pub tag: usize,
    pub width_factor: f32,
}

impl<'a> WMonitor<'a> {
    pub fn new(mi: &MonitorInfo, text_renderer: Rc<TextRenderer<'a, XCBConnection>>) -> Self {
        let layout = WLayout::MainStack;

        let bar_rect = WRect {
            x: mi.x,
            y: mi.y,
            w: mi.width,
            h: theme::bar::FONT_SIZE as u16 + (theme::bar::PADDING * 2),
        };

        let y = bar_rect.y + bar_rect.h as i16;
        let height = mi.height - bar_rect.h;

        let colors = WBarColors::new(
            theme::bar::FG,
            theme::bar::BG,
            theme::bar::FG_SELECTED,
            theme::bar::BG_SELECTED,
        );

        let bar_options = WBarOptions {
            rect: bar_rect,
            padding: theme::bar::PADDING,
            section_padding: theme::bar::SECTION_PADDING,
            tag_count: TAG_CAP,
            tag_width: theme::bar::TAG_WIDTH,
            colors,
        };

        let bar = WBar::new(
            &X_HANDLE.conn,
            text_renderer,
            bar_options,
            *theme::bar::MODULE_MASK,
            theme::bar::STATUS_INTERVAL,
        );

        Self {
            bar,
            primary: mi.primary,
            rect: WRect::new(mi.x, y, mi.width, height),
            clients: Vec::new(),
            client: None,
            layout,
            tag: 0,
            width_factor: MAIN_CLIENT_WIDTH_PERCENTAGE,
        }
    }

    pub fn has_pos(&self, p: &WPos) -> bool {
        let has_x = p.x >= self.rect.x && p.x <= self.rect.x + self.rect.w as i16;
        let has_y = p.y >= self.rect.y && p.y <= self.rect.y + self.rect.h as i16;
        has_x && has_y
    }

    pub fn find_client_move_direction(&self, p: WPos) -> Option<WDirection> {
        if p.x < self.rect.x {
            return Some(WDirection::Prev);
        } else if p.x > self.rect.x + self.rect.w as i16 {
            return Some(WDirection::Next);
        }
        None
    }

    pub fn set_tag(&mut self, new_tag: usize) -> Result<(), StateError> {
        if new_tag > TAG_CAP - 1 {
            return Err(StateError::Bounds(new_tag));
        }
        let clients = self.clients_in_tag(new_tag);
        if clients.is_empty() {
            self.client = None;
        } else if let Some(i) = clients.last() {
            self.client = Some(*i);
        }
        self.tag = new_tag;
        Ok(())
    }

    pub fn select_adjacent(&mut self, dir: WDirection) {
        if let Some(i) = self.client {
            match dir {
                WDirection::Prev => {
                    if let Some(i) = self.clients[i].prev {
                        self.client = Some(i);
                    }
                }
                WDirection::Next => {
                    if let Some(i) = self.clients[i].next {
                        self.client = Some(i);
                    }
                }
            }
        }
    }

    pub fn hide_clients(&self, tag: usize) -> Result<(), ReplyOrIdError> {
        let clients = self.clients_in_tag(tag);
        for i in clients.iter() {
            let c = self.clients[*i];
            let aux = ConfigureWindowAux::new().x(c.rect.w as i32 * -2);
            X_HANDLE.conn.configure_window(c.window, &aux)?;
        }
        X_HANDLE.conn.flush()?;
        Ok(())
    }

    pub fn set_layout(&mut self, layout: WLayout) -> bool {
        if self.layout == layout {
            return false;
        }
        self.layout = layout;
        true
    }

    pub fn clients_in_tag(&self, tag: usize) -> Vec<usize> {
        (0..self.clients.len())
            .into_iter()
            .filter(|i| self.clients[*i].tag == tag)
            .collect()
    }

    pub fn move_client_to_tag(&mut self, new_tag: usize) -> Result<(), ReplyOrIdError> {
        if self.client.is_none() || self.tag == new_tag {
            return Ok(());
        }

        self.unfocus_current_client()?;
        self.client_to_tag(new_tag)?;
        self.focus_current_client(true)?;
        Ok(())
    }

    pub fn swap_clients(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        if let Some(ci) = self.client {
            let adj_idx = match dir {
                WDirection::Prev => {
                    let curr = &mut self.clients[ci];
                    // early return since we have nothing to update
                    if curr.prev.is_none() {
                        return Ok(());
                    }
                    curr.prev
                }
                WDirection::Next => {
                    let curr = &mut self.clients[ci];
                    // early return since we have nothing to update
                    if curr.next.is_none() {
                        return Ok(());
                    }
                    curr.next
                }
            };

            let adj_idx = adj_idx.unwrap();
            self.clients.swap(adj_idx, ci);
            self.relink_clients_in_tag(self.tag)?;
            self.client = Some(adj_idx);
        }
        Ok(())
    }

    pub fn client_to_tag(&mut self, tag: usize) -> Result<(), ReplyOrIdError> {
        if let Some(curr_idx) = self.client {
            self.clients[curr_idx].tag = tag;
            self.relink_clients_in_tag(self.tag)?;
            self.relink_clients_in_tag(tag)?;

            self.bar
                .set_has_clients(self.tag, !self.clients_in_tag(self.tag).is_empty());
            self.bar.set_has_clients(tag, true);

            self.hide_clients(tag)?;
        }
        Ok(())
    }

    pub fn push_and_focus_client(
        &mut self,
        mut client: WClientState,
        monitor_index: usize,
    ) -> Result<(), ReplyOrIdError> {
        client.monitor = monitor_index;
        client.tag = self.tag;

        let clients = self.clients_in_tag(self.tag);

        if !clients.is_empty() {
            if let Some(i) = clients.last() {
                self.clients[*i].next = Some(self.clients.len());
                client.prev = Some(*i);
            }

            if let Some(i) = clients.first() {
                self.clients[*i].prev = Some(self.clients.len());
                client.next = Some(*i);
            }
        }

        self.clients.push(client);
        self.bar.set_has_clients(self.tag, true);
        self.client = Some(self.clients.len() - 1);

        self.recompute_layout()?;
        self.focus_current_client(true)?;

        Ok(())
    }

    pub fn recompute_layout(&mut self) -> Result<(), ReplyOrIdError> {
        let clients: Vec<_> = self
            .clients
            .iter_mut()
            .filter(|c| c.tag == self.tag && !c.is_floating)
            .collect();

        let rects = layout_clients(&self.layout, self.width_factor, &self.rect, clients.len());

        let rects = if let Some(rects) = rects {
            rects
        } else {
            return Ok(());
        };

        for (c, rect) in clients.into_iter().zip(rects) {
            c.resize(&self.rect, rect, false)?;
        }
        Ok(())
    }

    pub fn mouse_move(
        &mut self,
        (oc_pos, op_pos, last_move): (WPos, WPos, u32),
        ev: MotionNotifyEvent,
    ) -> Result<(), ReplyOrIdError> {
        if let Some(ci) = self.client {
            let c = &mut self.clients[ci];
            if c.is_fullscreen || ev.time - last_move <= (1000 / 60) {
                return Ok(());
            }

            let pdx = ev.root_x - op_pos.x;
            let pdy = ev.root_y - op_pos.y;
            let nx = oc_pos.x + pdx;
            let ny = oc_pos.y + pdy;

            c.rect.x = nx;
            c.rect.y = ny;

            let (nx, ny) = (nx as i32, ny as i32);
            X_HANDLE
                .conn
                .configure_window(c.window, &ConfigureWindowAux::new().x(nx).y(ny))?;
            X_HANDLE.conn.flush()?;
        }
        Ok(())
    }

    pub fn destroy_current_client(&mut self) -> Result<(), ReplyOrIdError> {
        if let Some(client_idx) = self.client {
            let client_state = self.remove_client(client_idx)?;
            self.recompute_layout()?;
            self.focus_current_client(true)?;
            client_state.delete_window()?;
        }
        Ok(())
    }

    pub fn focus(&mut self) -> Result<(), ReplyOrIdError> {
        self.bar.set_is_focused(true);
        self.focus_current_client(true)?;
        Ok(())
    }

    pub fn unfloat_focused_client(&mut self) -> Result<Option<WDirection>, ReplyOrIdError> {
        let mut direction = None;
        if let Some(ci) = self.client {
            if let Some(pos) = self.clients[ci].unfloat() {
                direction = self.find_client_move_direction(pos);
                self.recompute_layout()?;
                self.warp_pointer()?;
            }
        }
        Ok(direction)
    }

    pub fn focus_current_client(&mut self, warp_pointer: bool) -> Result<(), ReplyOrIdError> {
        if let Some(ci) = self.client {
            let c = &mut self.clients[ci];
            let title = c.get_window_title()?;
            self.bar.update_title(title);

            c.set_focus()?;

            if warp_pointer {
                self.warp_pointer()?;
            }
        } else {
            self.bar.update_title("");
            X_HANDLE.conn.set_input_focus(
                InputFocus::POINTER_ROOT,
                X_HANDLE.screen().root,
                CURRENT_TIME,
            )?;
        }

        Ok(())
    }

    pub fn unfocus(&mut self) -> Result<(), ReplyOrIdError> {
        self.bar.set_is_focused(false);
        self.unfocus_current_client()?;
        Ok(())
    }

    pub fn unfocus_current_client(&mut self) -> Result<(), ReplyOrIdError> {
        if let Some(idx) = self.client {
            self.clients[idx].unfocus()?;
        }
        Ok(())
    }

    pub fn set_current_client(&mut self, index: usize) -> Result<(), ReplyOrIdError> {
        self.client = Some(index);
        self.focus_current_client(true)?;
        Ok(())
    }

    pub fn warp_pointer(&self) -> Result<(), ReplyOrIdError> {
        if let Some(ci) = self.client {
            self.clients[ci].warp_pointer_to_center()?;
        } else {
            X_HANDLE.conn.warp_pointer(
                NONE,
                X_HANDLE.screen().root,
                0,
                0,
                0,
                0,
                self.rect.x + (self.rect.w as i16 / 2),
                self.rect.y + (self.rect.h as i16 / 2),
            )?;
        }
        Ok(())
    }

    pub fn mouse_resize_client(
        &mut self,
        last_resize: u32,
        ev: MotionNotifyEvent,
    ) -> Result<(), ReplyOrIdError> {
        if let Some(i) = self.client {
            self.clients[i].mouse_resize(&self.rect, ev, last_resize)?;
        }
        Ok(())
    }

    pub fn fullscreen_focused_client(&mut self) -> Result<(), ReplyOrIdError> {
        if let Some(client_index) = self.client {
            let c = &mut self.clients[client_index];

            if !c.is_fullscreen {
                c.fullscreen(&self.rect)?;
            } else {
                c.exit_fullscreen(&self.rect)?;
            }

            self.recompute_layout()?;
        }
        Ok(())
    }

    pub fn handle_configure_request(
        &mut self,
        client_idx: usize,
        evt: ConfigureRequestEvent,
        is_current_monitor: bool,
    ) -> Result<(), ReplyOrIdError> {
        let c = &mut self.clients[client_idx];
        let value_mask = WConfigWindow::from(evt.value_mask);

        c.apply_configure_request(&self.rect, evt, value_mask, is_current_monitor)?;

        Ok(())
    }

    fn relink_clients_in_tag(&mut self, tag: usize) -> Result<(), ReplyOrIdError> {
        let tag_clients = self.clients_in_tag(tag);

        if tag_clients.is_empty() {
            return Ok(());
        }

        if tag_clients.len() == 1 {
            self.clients[tag_clients[0]].prev = None;
            self.clients[tag_clients[0]].next = None;
            self.client = if tag == self.tag { Some(0) } else { None };
            return Ok(());
        }

        let first_idx = *tag_clients.first().unwrap();
        let last_idx = *tag_clients.last().unwrap();

        for (i, client_idx) in tag_clients.iter().enumerate() {
            let prev = if *client_idx == first_idx {
                last_idx
            } else {
                tag_clients[i - 1]
            };

            let next = if *client_idx == last_idx {
                first_idx
            } else {
                tag_clients[i + 1]
            };

            self.clients[*client_idx].prev = Some(prev);
            self.clients[*client_idx].next = Some(next);
        }

        if tag == self.tag {
            self.client = Some(self.client.unwrap().min(last_idx));
        }

        // since the client makeup for this tag has changed, we want to recompute the layout
        self.recompute_layout()?;

        Ok(())
    }

    pub fn remove_client(&mut self, idx: usize) -> Result<WClientState, ReplyOrIdError> {
        let c = self.clients.remove(idx);
        let clients_in_current_tag = self.clients_in_tag(self.tag);
        if clients_in_current_tag.is_empty() {
            self.client = None;
        } else {
            for t in 0..TAG_CAP {
                self.relink_clients_in_tag(t)?;
            }
        }

        self.bar
            .set_has_clients(self.tag, !clients_in_current_tag.is_empty());
        self.recompute_layout()?;
        self.focus_current_client(true)?;
        Ok(c)
    }
}

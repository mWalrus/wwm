use std::rc::Rc;

use thiserror::Error;
use wwm_bar::WBar;
use wwm_core::{
    text::TextRenderer,
    util::{
        bar::{WBarColors, WBarOptions},
        WLayout,
    },
};
use x11rb::{
    connection::Connection,
    protocol::{
        randr::MonitorInfo,
        xproto::{ConfigureWindowAux, ConnectionExt},
    },
    xcb_ffi::ReplyOrIdError,
};

use crate::command::WDirection;
use crate::{
    client::WClientState,
    config::{
        tags::{MAIN_CLIENT_WIDTH_PERCENTAGE, TAG_CAP},
        theme,
    },
};
use wwm_core::util::primitives::{WPos, WRect};

#[derive(Error, Debug)]
pub enum StateError {
    #[error("{0} is out of bounds")]
    Bounds(usize),
}

pub struct WMonitor<'a, C: Connection> {
    pub conn: &'a C,
    pub bar: WBar<'a, C>,
    pub primary: bool,
    pub rect: WRect,
    pub clients: Vec<WClientState>,
    pub client: Option<usize>,
    pub layout: WLayout,
    pub tag: usize,
    pub width_factor: f32,
}

impl<'a, C: Connection> WMonitor<'a, C> {
    pub fn new(mi: &MonitorInfo, conn: &'a C, text_renderer: Rc<TextRenderer<'a, C>>) -> Self {
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
            conn,
            text_renderer,
            bar_options,
            *theme::bar::MODULE_MASK,
            theme::bar::STATUS_INTERVAL,
        );

        Self {
            conn,
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

    pub fn has_pos(&self, p: WPos) -> bool {
        let has_x = p.x >= self.rect.x && p.x <= self.rect.x + self.rect.w as i16;
        let has_y = p.y >= self.rect.y && p.y <= self.rect.y + self.rect.h as i16;
        has_x && has_y
    }

    pub fn find_adjacent_monitor(&self, p: WPos) -> Option<WDirection> {
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

    pub fn hide_clients(&self, conn: &C, tag: usize) -> Result<(), ReplyOrIdError> {
        let clients = self.clients_in_tag(tag);
        for i in clients.iter() {
            let c = self.clients[*i];
            let aux = ConfigureWindowAux::new().x(c.rect.w as i32 * -2);
            conn.configure_window(c.window, &aux)?;
        }
        conn.flush()?;
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

    pub fn swap_clients(&mut self, dir: WDirection) {
        if let Some(ci) = self.client {
            let adj_idx = match dir {
                WDirection::Prev => {
                    let curr = &mut self.clients[ci];
                    // early return since we have nothing to update
                    if curr.prev.is_none() {
                        return;
                    }
                    curr.prev
                }
                WDirection::Next => {
                    let curr = &mut self.clients[ci];
                    // early return since we have nothing to update
                    if curr.next.is_none() {
                        return;
                    }
                    curr.next
                }
            };

            let adj_idx = adj_idx.unwrap();
            self.clients.swap(adj_idx, ci);
            self.relink_clients_in_tag(self.tag);
            self.client = Some(adj_idx);
        }
    }

    pub fn client_to_tag(&mut self, conn: &C, tag: usize) -> Result<(), ReplyOrIdError> {
        if let Some(curr_idx) = self.client {
            self.clients[curr_idx].tag = tag;
            self.relink_clients_in_tag(self.tag);
            self.relink_clients_in_tag(tag);

            self.bar
                .set_has_clients(self.tag, !self.clients_in_tag(self.tag).is_empty());
            self.bar.set_has_clients(tag, true);

            self.hide_clients(conn, tag)?;
        }
        Ok(())
    }

    pub fn push_client(&mut self, mut client: WClientState) {
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
    }

    fn relink_clients_in_tag(&mut self, tag: usize) {
        let tag_clients = self.clients_in_tag(tag);

        if tag_clients.is_empty() {
            return;
        }

        if tag_clients.len() == 1 {
            self.clients[tag_clients[0]].prev = None;
            self.clients[tag_clients[0]].next = None;
            self.client = if tag == self.tag { Some(0) } else { None };
            return;
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
    }

    pub fn remove_client(&mut self, idx: usize) -> WClientState {
        let c = self.clients.remove(idx);
        let clients_in_current_tag = self.clients_in_tag(self.tag);
        if clients_in_current_tag.is_empty() {
            self.client = None;
        } else {
            for t in 0..TAG_CAP {
                self.relink_clients_in_tag(t);
            }
        }

        self.bar
            .set_has_clients(self.tag, !clients_in_current_tag.is_empty());
        c
    }

    pub fn width_from_percentage(&self, p: f32) -> u16 {
        (self.rect.w as f32 * p) as u16
    }
}

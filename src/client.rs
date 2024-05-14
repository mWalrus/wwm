use x11rb::{
    errors::{ReplyError, ReplyOrIdError},
    properties::WmSizeHints,
    protocol::xproto::{
        ChangeWindowAttributesAux, ClientMessageEvent, CloseDown, ConfigureNotifyEvent,
        ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt, EventMask, InputFocus,
        MotionNotifyEvent, PropMode, StackMode, Window,
    },
    wrapper::ConnectionExt as _,
    CURRENT_TIME, NONE,
};

use crate::{
    config::{
        bar_height,
        theme::{self, window::BORDER_WIDTH},
    },
    X_HANDLE,
};
use wwm_core::util::{
    primitives::{WPos, WRect, WSize},
    WConfigWindow,
};

#[repr(u8)]
pub enum WindowState {
    Withdrawn,
    Normal,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct WClientState {
    pub window: Window,
    pub rect: WRect,
    pub old_rect: WRect,
    pub is_floating: bool,
    pub is_fullscreen: bool,
    pub is_fixed: bool,
    pub hints_valid: bool,
    pub bw: u16,
    pub base_size: Option<WSize>,
    pub min_size: Option<WSize>,
    pub max_size: Option<WSize>,
    pub inc_size: Option<WSize>,
    pub maxa: Option<f32>,
    pub mina: Option<f32>,
    pub tag: usize,
    pub monitor: usize,
    pub old_state: bool,
    pub old_bw: u16,
    pub prev: Option<usize>,
    pub next: Option<usize>,
}

impl WClientState {
    pub fn new(
        window: Window,
        rect: WRect,
        old_rect: WRect,
        is_floating: bool,
        is_fullscreen: bool,
        tag: usize,
        monitor: usize,
    ) -> Self {
        println!("managing new client with size: {rect:#?}");
        Self {
            window,
            rect,
            old_rect,
            is_floating,
            is_fullscreen,
            is_fixed: false,
            hints_valid: false,
            bw: BORDER_WIDTH,
            base_size: None,
            min_size: None,
            max_size: None,
            inc_size: None,
            maxa: None,
            mina: None,
            tag,
            monitor,
            old_state: false,
            old_bw: 0,
            prev: None,
            next: None,
        }
    }

    pub fn apply_configure_request(
        &mut self,
        mr: &WRect,
        evt: ConfigureRequestEvent,
        value_mask: WConfigWindow,
        is_current_monitor: bool,
    ) -> Result<(), ReplyOrIdError> {
        if value_mask & WConfigWindow::BORDER_WIDTH {
            self.bw = evt.border_width;
            return Ok(());
        }

        if self.is_floating {
            if value_mask & WConfigWindow::X {
                self.old_rect.x = self.rect.x;
                self.rect.x = mr.x + evt.x;
            }

            if value_mask & WConfigWindow::Y {
                self.old_rect.y = self.rect.y;
                self.rect.y = mr.y + evt.y;
            }

            if value_mask & WConfigWindow::WIDTH {
                self.old_rect.w = self.rect.w;
                self.rect.w = evt.width;
            }

            if value_mask & WConfigWindow::HEIGHT {
                self.old_rect.h = self.rect.h;
                self.rect.h = evt.height;
            }

            if self.rect.x + self.rect.w as i16 > mr.x + mr.w as i16 && self.is_floating {
                self.rect.x = mr.x + (mr.w as i16 / 2 - (self.rect.w + 2 * self.bw) as i16 / 2)
            }
            if self.rect.y + self.rect.h as i16 > mr.y + mr.h as i16 && self.is_floating {
                self.rect.y = mr.y + (mr.h as i16 / 2 - (self.rect.h + 2 * self.bw) as i16 / 2)
            }

            // FIXME: do we not want to resize no matter what monitor the client is currently on?
            //        One thought would be to only resize the client if it is visible but monitor
            //        should not dictate this.
            if is_current_monitor {
                self.resize(mr, self.rect, false)?;
            } else if value_mask & (WConfigWindow::X | WConfigWindow::Y)
                && !(value_mask & (WConfigWindow::WIDTH | WConfigWindow::HEIGHT))
            {
                println!("apply configure");
                self.update_client_size(self.rect)?;
            }
            return Ok(());
        }
        Ok(())
    }

    pub fn resize(
        &mut self,
        mon_size: &WRect,
        mut new_size: WRect,
        interact: bool,
    ) -> Result<(), ReplyOrIdError> {
        if self
            .apply_size_hints(mon_size, &mut new_size, interact)
            .unwrap_or_default()
        {
            self.apply_resize(new_size)?;
        }
        Ok(())
    }

    fn apply_resize(&mut self, new_size: WRect) -> Result<(), ReplyOrIdError> {
        X_HANDLE.conn.configure_window(
            self.window,
            &ConfigureWindowAux::new()
                .x(new_size.x as i32)
                .y(new_size.y as i32)
                .width(new_size.w as u32)
                .height(new_size.h as u32)
                .border_width(self.bw as u32),
        )?;

        println!("apply resize");
        self.update_client_size(new_size)?;
        Ok(())
    }

    pub fn update_client_size(&mut self, rect: WRect) -> Result<(), ReplyOrIdError> {
        println!("updating client size to: {rect:#?}");
        let mut ce = ConfigureNotifyEvent::default();

        ce.response_type = 22; // ConfigureNotify
        ce.event = self.window;
        ce.window = self.window;
        ce.x = rect.x;
        ce.y = rect.y;
        ce.width = rect.w;
        ce.height = rect.h;
        ce.border_width = self.bw;
        ce.override_redirect = false;

        X_HANDLE
            .conn
            .send_event(false, self.window, EventMask::STRUCTURE_NOTIFY, ce)?;

        Ok(())
    }

    pub fn set_state(&mut self, state: WindowState) -> Result<(), ReplyOrIdError> {
        X_HANDLE.conn.change_property(
            PropMode::REPLACE,
            self.window,
            X_HANDLE.atoms.WM_STATE,
            X_HANDLE.atoms.WM_STATE,
            8,
            2,
            &[state as u8, 0],
        )?;
        Ok(())
    }

    pub fn set_initial_window_attributes(&self) -> Result<(), ReplyOrIdError> {
        let change_aux = ChangeWindowAttributesAux::new()
            .border_pixel(theme::window::BORDER_UNFOCUSED)
            .event_mask(
                EventMask::ENTER_WINDOW
                    | EventMask::FOCUS_CHANGE
                    | EventMask::PROPERTY_CHANGE
                    | EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::STRUCTURE_NOTIFY,
            );

        X_HANDLE
            .conn
            .change_window_attributes(self.window, &change_aux)?;
        Ok(())
    }

    pub fn mouse_resize(
        &mut self,
        mon_rect: &WRect,
        ev: MotionNotifyEvent,
        last_resize: u32,
    ) -> Result<(), ReplyOrIdError> {
        if self.is_fullscreen || ev.time - last_resize <= (1000 / 60) {
            return Ok(());
        }

        if self.is_floating {
            let nw = 1.max(ev.root_x - self.rect.x - (2 * BORDER_WIDTH as i16) + 1) as u16;
            let nh = 1.max(ev.root_y - self.rect.y - (2 * BORDER_WIDTH as i16) + 1) as u16;

            // copy before move
            let x = self.rect.x;
            let y = self.rect.y;

            let rect = WRect::new(x, y, nw, nh);

            self.resize(mon_rect, rect, true)?;
        }
        Ok(())
    }

    fn apply_size_hints(
        &mut self,
        mon_rect: &WRect,
        new_size: &mut WRect,
        interact: bool,
    ) -> Result<bool, ReplyOrIdError> {
        new_size.w = new_size.w.max(1);
        new_size.h = new_size.h.max(1);

        let screen = X_HANDLE.screen();

        let (sw, sh) = (
            screen.width_in_pixels as i16,
            screen.height_in_pixels as i16,
        );

        if interact {
            if new_size.x > sw {
                new_size.x = sw - (self.rect.w + 2 * self.bw) as i16;
            }
            if new_size.y > sh {
                new_size.y = sh - (self.rect.h + 2 * self.bw) as i16;
            }
            if (new_size.x + new_size.w as i16 + 2 * self.bw as i16) < 0 {
                new_size.x = 0;
            }
            if (new_size.y + new_size.h as i16 + 2 * self.bw as i16) < 0 {
                new_size.y = 0;
            }
        } else {
            if new_size.x >= mon_rect.x + mon_rect.w as i16 {
                new_size.x = mon_rect.x + mon_rect.w as i16 - (self.rect.w + 2 * self.bw) as i16;
            }
            if new_size.y >= mon_rect.y + mon_rect.h as i16 {
                new_size.y = mon_rect.y + mon_rect.h as i16 - (self.rect.h + 2 * self.bw) as i16;
            }
            if (new_size.x + new_size.w as i16 + 2 * self.bw as i16) <= mon_rect.x {
                new_size.x = mon_rect.x;
            }
            if (new_size.y + new_size.h as i16 + 2 * self.bw as i16) <= mon_rect.y {
                new_size.y = mon_rect.y;
            }
        }

        let bh = bar_height();
        if new_size.h < bh {
            new_size.h = bh;
        }
        if new_size.w < bh {
            new_size.w = bh;
        }
        if self.is_floating {
            if !self.hints_valid {
                self.apply_normal_hints()?;
            }

            (new_size.w, new_size.h) = self.adjust_aspect_ratio(new_size.w, new_size.h);
        }

        Ok(new_size.x != self.rect.x
            || new_size.y != self.rect.y
            || new_size.w != self.rect.w
            || new_size.h != self.rect.h)
    }

    pub fn warp_pointer_to_center(&self) -> Result<(), ReplyOrIdError> {
        if let Ok(pointer_reply) = X_HANDLE.conn.query_pointer(self.window) {
            if let Ok(pointer) = pointer_reply.reply() {
                if !pointer.same_screen {
                    return Ok(());
                }
                X_HANDLE.conn.warp_pointer(
                    NONE,
                    self.window,
                    0,
                    0,
                    0,
                    0,
                    self.rect.w as i16 / 2,
                    self.rect.h as i16 / 2,
                )?;
            }
        }
        Ok(())
    }

    pub fn delete_window(self) -> Result<(), ReplyOrIdError> {
        // figure out if WM_PROTOCOLS includes the WM_DELETE_WINDOW atom
        // NOTE: https://tronche.com/gui/x/icccm/sec-4.html#s-4.2.8.1
        let reply = X_HANDLE
            .conn
            .get_property(
                false,
                self.window,
                X_HANDLE.atoms.WM_PROTOCOLS,
                X_HANDLE.atoms.ATOM,
                0,
                1,
            )?
            .reply()?;

        let delete_exists = if let Some(mut iter) = reply.value32() {
            iter.next()
                .map(|atom| atom == X_HANDLE.atoms.WM_DELETE_WINDOW)
                .unwrap_or(false)
        } else {
            false
        };

        if delete_exists {
            self.send_wm_protocols_event(X_HANDLE.atoms.WM_DELETE_WINDOW)?;
        } else {
            X_HANDLE.conn.grab_server()?;
            X_HANDLE.conn.set_close_down_mode(CloseDown::DESTROY_ALL)?;
            X_HANDLE.conn.kill_client(self.window)?;
            X_HANDLE.conn.sync()?;
            X_HANDLE.conn.ungrab_server()?;
        }
        Ok(())
    }

    pub fn get_normal_hints(&self) -> Result<WmSizeHints, ReplyOrIdError> {
        match WmSizeHints::get_normal_hints(&X_HANDLE.conn, self.window) {
            Ok(r) => match r.reply() {
                Ok(hints) => Ok(hints),
                Err(e) => Err(e)?,
            },
            Err(e) => Err(e)?,
        }
    }

    pub fn get_window_title(&self) -> Result<String, ReplyOrIdError> {
        if let Ok(reply) = X_HANDLE.conn.get_property(
            false,
            self.window,
            X_HANDLE.atoms._NET_WM_NAME,
            X_HANDLE.atoms.UTF8_STRING,
            0,
            8,
        ) {
            if let Ok(reply) = reply.reply() {
                if let Some(text) = reply.value8() {
                    let text = String::from_utf8(text.collect()).unwrap();
                    return Ok(text);
                }
            }
        }
        Ok(String::new())
    }

    pub fn set_withdrawn(&mut self) -> Result<(), ReplyOrIdError> {
        X_HANDLE.conn.grab_server()?;
        X_HANDLE
            .conn
            .set_input_focus(InputFocus::POINTER_ROOT, self.window, CURRENT_TIME)?;
        self.set_state(WindowState::Withdrawn)?;
        X_HANDLE.conn.sync()?;
        X_HANDLE.conn.ungrab_server()?;
        Ok(())
    }

    pub fn set_focus(&mut self) -> Result<(), ReplyOrIdError> {
        X_HANDLE
            .conn
            .set_input_focus(InputFocus::POINTER_ROOT, self.window, CURRENT_TIME)?;

        self.send_wm_protocols_event(X_HANDLE.atoms.WM_TAKE_FOCUS)?;

        X_HANDLE.conn.change_property(
            PropMode::REPLACE,
            X_HANDLE.screen().root,
            X_HANDLE.atoms._NET_ACTIVE_WINDOW,
            X_HANDLE.atoms.WINDOW,
            32,
            1,
            &self.window.to_ne_bytes(),
        )?;

        let focus_aux =
            ChangeWindowAttributesAux::new().border_pixel(theme::window::BORDER_FOCUSED);
        X_HANDLE
            .conn
            .change_window_attributes(self.window, &focus_aux)?;

        Ok(())
    }

    pub fn unfocus(&mut self) -> Result<(), ReplyOrIdError> {
        let unfocus_aux =
            ChangeWindowAttributesAux::new().border_pixel(theme::window::BORDER_UNFOCUSED);
        X_HANDLE
            .conn
            .change_window_attributes(self.window, &unfocus_aux)?;
        X_HANDLE
            .conn
            .delete_property(self.window, X_HANDLE.atoms._NET_ACTIVE_WINDOW)?;
        Ok(())
    }

    pub fn float(&mut self) -> Result<(), ReplyOrIdError> {
        X_HANDLE.conn.configure_window(
            self.window,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;
        Ok(())
    }

    pub fn unfloat(&mut self) -> Option<WPos> {
        if !self.is_floating {
            return None;
        }

        self.is_floating = false;

        Some(WPos::new(
            self.rect.x + (self.rect.w as i16 / 2),
            self.rect.y,
        ))
    }

    fn send_wm_protocols_event(&self, proto: u32) -> Result<(), ReplyError> {
        let event = ClientMessageEvent::new(
            32,
            self.window,
            X_HANDLE.atoms.WM_PROTOCOLS,
            [proto, CURRENT_TIME, 0, 0, 0],
        );
        X_HANDLE
            .conn
            .send_event(false, self.window, EventMask::NO_EVENT, event)?;
        Ok(())
    }

    pub fn fullscreen(&mut self, monitor_rect: &WRect) -> Result<(), ReplyOrIdError> {
        X_HANDLE.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            X_HANDLE.atoms._NET_WM_STATE,
            X_HANDLE.atoms.ATOM,
            &[X_HANDLE.atoms._NET_WM_STATE_FULLSCREEN],
        )?;

        self.is_fullscreen = true;
        self.old_state = self.is_floating;
        self.old_bw = self.bw;
        self.bw = 0;
        self.is_floating = true;

        let bh = bar_height();

        let client_rect = WRect::new(
            monitor_rect.x,
            monitor_rect.y - bh as i16,
            monitor_rect.w,
            monitor_rect.h + bh,
        );

        self.resize(monitor_rect, client_rect, false)?;

        Ok(())
    }

    pub fn exit_fullscreen(&mut self, monitor_rect: &WRect) -> Result<(), ReplyOrIdError> {
        X_HANDLE.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            X_HANDLE.atoms._NET_WM_STATE,
            X_HANDLE.atoms.ATOM,
            &[0],
        )?;

        self.is_fullscreen = false;
        self.is_floating = self.old_state;
        self.bw = self.old_bw;

        self.resize(monitor_rect, self.old_rect, false)?;

        Ok(())
    }

    pub fn apply_normal_hints(&mut self) -> Result<(), ReplyOrIdError> {
        let hints = self.get_normal_hints()?;
        if hints.base_size.is_some() {
            self.base_size = WSize::from(hints.base_size);
        } else if hints.min_size.is_some() {
            self.base_size = WSize::from(hints.min_size);
        } else {
            self.base_size = None;
        }

        if hints.size_increment.is_some() {
            self.inc_size = WSize::from(hints.size_increment);
        } else {
            self.inc_size = None;
        }

        if hints.max_size.is_some() {
            self.max_size = WSize::from(hints.max_size);
        } else {
            self.max_size = None;
        }

        if hints.min_size.is_some() {
            self.min_size = WSize::from(hints.min_size);
        } else {
            self.min_size = None;
        }

        if let Some((min_a, max_a)) = hints.aspect {
            self.mina = Some(min_a.numerator as f32 / min_a.denominator as f32);
            self.maxa = Some(max_a.numerator as f32 / max_a.denominator as f32);
        } else {
            self.mina = None;
            self.maxa = None;
        }

        self.is_fixed = false;
        if let Some(max_size) = self.max_size {
            if let Some(min_size) = self.min_size {
                self.is_fixed = max_size.w > 0
                    && max_size.h > 0
                    && max_size.w == min_size.w
                    && max_size.h == min_size.h;
            }
        }
        self.hints_valid = true;

        Ok(())
    }

    pub fn adjust_aspect_ratio(&self, mut w: u16, mut h: u16) -> (u16, u16) {
        // ICCCM 4.1.2.3
        let mut base_is_min = false;
        let mut base_exists = false;
        let mut min_exists = false;
        if let Some(base_size) = self.base_size {
            base_exists = true;
            if let Some(min_size) = self.min_size {
                min_exists = true;
                base_is_min = base_size.w == min_size.w && base_size.h == min_size.h;
            }
        }

        if base_is_min && base_exists {
            let base = self.base_size.unwrap();
            w -= base.w;
            h -= base.h;
        }

        if let Some(maxa) = self.maxa {
            if maxa < w as f32 / h as f32 {
                w = (h as f32 * maxa + 0.5) as u16;
            }
        }

        if let Some(mina) = self.mina {
            if mina < h as f32 / w as f32 {
                h = (w as f32 * mina + 0.5) as u16;
            }
        }

        if base_is_min && base_exists {
            let base = self.base_size.unwrap();
            w -= base.w;
            h -= base.h;
        }

        if let Some(inc_size) = self.inc_size {
            w -= w % inc_size.w;
            h -= h % inc_size.h;
        }

        if min_exists && base_exists {
            let base = self.base_size.unwrap();
            let min = self.min_size.unwrap();
            w = min.w.max(w + base.w);
            h = min.h.max(h + base.h);
        }

        if let Some(max_size) = self.max_size {
            w = w.min(max_size.w);
            h = h.min(max_size.h);
        }
        (w, h)
    }
}

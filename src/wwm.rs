use crate::{
    client::WClientState,
    command::{WKeyCommand, WMouseCommand},
    config::{
        auto_start::AUTO_START_COMMANDS,
        mouse::{DRAG_BUTTON, RESIZE_BUTTON},
        theme::{self, window::BORDER_WIDTH},
        workspaces::WIDTH_ADJUSTMENT_FACTOR,
    },
    keyboard::WKeyboard,
    layouts::{layout_clients, WLayout},
    monitor::WMonitor,
    mouse::WMouse,
    util::{self, Pos, Rect, WConfigWindow, WDirection},
    AtomCollection,
};
use std::{
    collections::HashSet,
    process::{exit, Command},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    thread,
    time::Duration,
};
use wwm_bar::{
    font::{loader::X11Font, FontDrawer},
    visual::RenderVisualInfo,
};
use x11rb::{
    connection::Connection,
    properties::WmSizeHints,
    protocol::{
        randr::ConnectionExt as _,
        xproto::{
            ButtonPressEvent, ButtonReleaseEvent, ChangeWindowAttributesAux, ClientMessageEvent,
            CloseDown, ConfigureNotifyEvent, ConfigureRequestEvent, ConfigureWindowAux,
            ConnectionExt, DestroyNotifyEvent, EnterNotifyEvent, EventMask, ExposeEvent,
            GetGeometryReply, InputFocus, KeyPressEvent, MapRequestEvent, MapState,
            MotionNotifyEvent, PropMode, PropertyNotifyEvent, Screen, StackMode, UnmapNotifyEvent,
            Window,
        },
        ErrorKind, Event,
    },
    rust_connection::{ReplyError, ReplyOrIdError},
    wrapper::ConnectionExt as _,
    CURRENT_TIME, NONE,
};

#[repr(u8)]
enum WindowState {
    Withdrawn,
    Normal,
}

#[repr(u8)]
enum NotifyMode {
    Normal,
    Inferior,
}

pub struct WinMan<'a, C: Connection> {
    conn: &'a C,
    screen: &'a Screen,
    #[allow(dead_code)]
    font_drawer: Rc<FontDrawer>,
    monitors: Vec<WMonitor<'a, C>>,
    selmon: usize,
    pending_exposure: HashSet<Window>,
    drag_window: Option<(Pos, Pos, u32)>,
    resize_window: Option<u32>,
    keyboard: WKeyboard,
    mouse: WMouse,
    atoms: AtomCollection,
    ignore_enter: bool,
    should_exit: Arc<AtomicBool>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum ShouldExit {
    Yes,
    No,
}

impl<'a, C: Connection> WinMan<'a, C> {
    pub fn init(
        conn: &'a C,
        screen_num: usize,
        keyboard: WKeyboard,
        mouse: WMouse,
        atoms: AtomCollection,
    ) -> Result<Self, ReplyOrIdError> {
        // TODO: error handling
        let screen = &conn.setup().roots[screen_num];

        conn.flush()?;

        Self::become_wm(conn, screen, mouse.cursors.normal)?;
        Self::run_auto_start_commands().unwrap();

        let vis_info = Rc::new(RenderVisualInfo::new(conn, screen).unwrap());
        let font = X11Font::new(
            conn,
            vis_info.render.pict_format,
            theme::bar::FONT,
            theme::bar::FONT_SIZE,
        )
        .unwrap();
        let font_drawer = Rc::new(FontDrawer::new(font));

        let mut monitors: Vec<WMonitor<'a, C>> =
            Self::get_monitors(conn, screen, &font_drawer, &vis_info)?.into();

        let selmon = monitors.iter().position(|m| m.primary).unwrap_or(0);
        monitors[selmon].bar.set_is_focused(true);

        let mut wwm = Self {
            conn,
            screen,
            font_drawer,
            monitors,
            selmon,
            pending_exposure: Default::default(),
            drag_window: None,
            resize_window: None,
            keyboard,
            mouse,
            atoms,
            ignore_enter: false,
            should_exit: Arc::new(AtomicBool::new(false)),
        };
        wwm.warp_pointer_to_focused_monitor()?;

        // take care of potentially unmanaged windows
        wwm.scan_windows()?;
        Ok(wwm)
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        'eventloop: loop {
            // self.refresh();
            self.conn.flush()?;

            loop {
                if let Ok(Some(event)) = self.conn.poll_for_event() {
                    if self.handle_event(event)? == ShouldExit::Yes {
                        break 'eventloop;
                    }

                    self.conn.flush()?;
                }
                for m in self.monitors.iter_mut() {
                    m.bar.draw(self.conn);
                }
            }
        }
        Ok(())
    }

    fn become_wm(conn: &'a C, screen: &Screen, cursor: u32) -> Result<(), ReplyError> {
        let change = ChangeWindowAttributesAux::default()
            .event_mask(
                EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::POINTER_MOTION
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::PROPERTY_CHANGE,
            )
            .cursor(cursor);

        let res = conn
            .change_window_attributes(screen.root, &change)
            .unwrap()
            .check();

        if let Err(ReplyError::X11Error(ref error)) = res {
            if error.error_kind == ErrorKind::Access {
                eprintln!("ERROR: Another WM is already running.");
                exit(1);
            }
        }

        conn.sync()?;

        res
    }

    fn destroy_window(&mut self) -> Result<(), ReplyOrIdError> {
        let win = if let Some(cidx) = self.monitors[self.selmon].client {
            self.monitors[self.selmon].clients[cidx].window
        } else {
            return Ok(());
        };

        self.detach(win, self.selmon);

        let delete_exists = self.window_property_exists(
            win,
            self.atoms.WM_DELETE_WINDOW,
            self.atoms.WM_PROTOCOLS,
            self.atoms.ATOM,
        )?;

        if delete_exists {
            self.send_event(win, self.atoms.WM_DELETE_WINDOW)?;
        } else {
            self.conn.grab_server()?;
            self.conn.set_close_down_mode(CloseDown::DESTROY_ALL)?;
            self.conn.kill_client(win)?;
            self.conn.sync()?;
            self.conn.ungrab_server()?;
        }
        self.recompute_layout(self.selmon)?;
        self.focus()?;
        self.warp_pointer_to_focused_client()?;

        Ok(())
    }

    fn detach(&mut self, win: Window, monitor: usize) {
        let m = &mut self.monitors[monitor];

        let remove_idx = m.clients.iter().position(|c| c.window == win);
        if let Some(i) = remove_idx {
            m.remove_client(i);
        }
    }

    fn get_window_title(&mut self, window: Window) -> Result<String, ReplyOrIdError> {
        if let Ok(reply) = self.conn.get_property(
            false,
            window,
            self.atoms._NET_WM_NAME,
            self.atoms.UTF8_STRING,
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

    fn focus(&mut self) -> Result<(), ReplyOrIdError> {
        let (win, mon) = {
            let m = &mut self.monitors[self.selmon];
            if let Some(c) = m.selected_client() {
                (c.window, c.monitor)
            } else {
                m.bar.update_title(self.conn, "");
                self.conn.set_input_focus(
                    InputFocus::POINTER_ROOT,
                    self.screen.root,
                    CURRENT_TIME,
                )?;
                return Ok(());
            }
        };

        self.conn.sync()?;

        let name = self.get_window_title(win)?;
        self.monitors[mon].bar.update_title(self.conn, name);

        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, win, CURRENT_TIME)?;

        self.send_event(win, self.atoms.WM_TAKE_FOCUS)?;
        self.conn.change_property(
            PropMode::REPLACE,
            self.screen.root,
            self.atoms._NET_ACTIVE_WINDOW,
            self.atoms.WINDOW,
            32,
            1,
            &win.to_ne_bytes(),
        )?;

        let focus_aux =
            ChangeWindowAttributesAux::new().border_pixel(theme::window::BORDER_FOCUSED);
        self.conn.change_window_attributes(win, &focus_aux)?;

        Ok(())
    }

    fn focus_adjacent(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        self.unfocus(self.selmon)?;
        let m = &mut self.monitors[self.selmon];
        m.select_adjacent(dir);
        self.focus()?;
        self.warp_pointer_to_focused_client()?;
        Ok(())
    }

    fn focus_adjacent_monitor(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        self.unfocus(self.selmon)?;

        let selmon = match dir {
            WDirection::Prev if self.selmon == 0 => self.monitors.len() - 1,
            WDirection::Prev => self.selmon - 1,
            WDirection::Next if self.selmon == self.monitors.len() - 1 => 0,
            WDirection::Next => self.selmon + 1,
        };

        // swap bar focus
        self.monitors[self.selmon].bar.set_is_focused(false);
        self.monitors[selmon].bar.set_is_focused(true);
        self.selmon = selmon;

        self.focus()?;

        self.warp_pointer_to_focused_monitor()?;
        self.warp_pointer_to_focused_client()?;
        Ok(())
    }

    fn focus_at_pointer(&mut self, evt: &MotionNotifyEvent) -> Result<(), ReplyOrIdError> {
        for (i, m) in self.monitors.iter().enumerate() {
            if m.has_pos(Pos::from(evt)) && i != self.selmon {
                self.unfocus(self.selmon)?;
                self.selmon = i;
                self.focus()?;
                break;
            }
        }
        Ok(())
    }

    fn for_all_clients<F: Fn(&WClientState) -> bool>(&self, cb: F) -> bool {
        let mut success = false;
        for mon in self.monitors.iter() {
            for c in mon.clients.iter() {
                if cb(c) {
                    success = true;
                }
            }
        }
        success
    }

    fn get_monitors(
        conn: &'a C,
        screen: &Screen,
        font_drawer: &Rc<FontDrawer>,
        vis_info: &Rc<RenderVisualInfo>,
    ) -> Result<Vec<WMonitor<'a, C>>, ReplyError> {
        let monitors = conn.randr_get_monitors(screen.root, true)?.reply()?;
        let monitors: Vec<WMonitor<C>> = monitors
            .monitors
            .iter()
            .map(|m| WMonitor::new(m, conn, Rc::clone(font_drawer), Rc::clone(vis_info)))
            .collect();
        Ok(monitors)
    }

    fn handle_button_press(&mut self, evt: ButtonPressEvent) -> Result<(), ReplyOrIdError> {
        let m = &mut self.monitors[self.selmon];
        if m.bar.has_pointer(evt.root_x, evt.root_y) {
            if let Some(idx) = m.bar.select_tag_at_pos(evt.event_x, evt.event_y) {
                self.select_tag(idx, false)?;
            }
            return Ok(());
        }

        let mut action = WMouseCommand::Idle;
        for bind in &self.mouse.binds {
            if u8::from(bind.button) == evt.detail && bind.mods_as_key_but_mask() == evt.state {
                action = bind.action;
                break;
            }
        }

        self.manipulate_client_dims(evt, action)?;

        Ok(())
    }

    fn manipulate_client_dims(
        &mut self,
        evt: ButtonPressEvent,
        action: WMouseCommand,
    ) -> Result<(), ReplyOrIdError> {
        let m = &mut self.monitors[self.selmon];
        if let Some(c) = m.selected_client_mut() {
            // is outside
            if evt.root_x > c.rect.x.max(c.rect.x + c.rect.w as i16) {
                return Ok(());
            }

            let mut should_recompute_layout = false;
            match action {
                WMouseCommand::DragClient if self.drag_window.is_none() => {
                    self.drag_window = Some((
                        Pos::from(c.rect),
                        Pos::new(evt.root_x, evt.root_y),
                        evt.time,
                    ));
                    should_recompute_layout = true;
                }
                WMouseCommand::ResizeClient if self.resize_window.is_none() => {
                    self.conn.warp_pointer(
                        NONE,
                        c.window,
                        0,
                        0,
                        0,
                        0,
                        (c.rect.w + BORDER_WIDTH - 1) as i16,
                        (c.rect.h + BORDER_WIDTH - 1) as i16,
                    )?;
                    self.resize_window = Some(evt.time);
                    should_recompute_layout = true;
                }
                _ => {}
            }

            if !should_recompute_layout {
                return Ok(());
            }

            // bring window to front
            self.conn.configure_window(
                c.window,
                &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            )?;

            c.is_floating = true;
            self.recompute_layout(self.selmon).unwrap();
        }
        Ok(())
    }

    fn apply_size_hints(
        &mut self,
        c_idx: usize,
        mon_idx: usize,
        mut x: i16,
        mut y: i16,
        mut w: u16,
        mut h: u16,
        interact: bool,
    ) -> Result<bool, ReplyOrIdError> {
        let m = &self.monitors[mon_idx];
        let mon_rect = m.rect;
        let c = &mut self.monitors[mon_idx].clients[c_idx];

        w = w.min(1);
        h = h.min(1);

        let (sw, sh) = (
            self.screen.width_in_pixels as i16,
            self.screen.height_in_pixels as i16,
        );

        if interact {
            if x > sw {
                x = sw - (c.rect.w + 2 * c.bw) as i16;
            }
            if y > sh {
                y = sh - (c.rect.h + 2 * c.bw) as i16;
            }
            if (x + w as i16 + 2 * c.bw as i16) < 0 {
                x = 0;
            }
            if (y + h as i16 + 2 * c.bw as i16) < 0 {
                y = 0;
            }
        } else {
            if x >= mon_rect.x + mon_rect.w as i16 {
                x = mon_rect.x + mon_rect.w as i16 - (c.rect.w + 2 * c.bw) as i16;
            }
            if y >= mon_rect.y + mon_rect.h as i16 {
                y = mon_rect.y + mon_rect.h as i16 - (c.rect.h + 2 * c.bw) as i16;
            }
            if (x + w as i16 + 2 * c.bw as i16) <= mon_rect.x {
                x = mon_rect.x;
            }
            if (y + h as i16 + 2 * c.bw as i16) <= mon_rect.y {
                y = mon_rect.y;
            }
        }
        let bh = util::bar_height();
        if h < bh {
            h = bh;
        }
        if w < bh {
            w = bh;
        }
        if c.is_floating {
            if !c.hints_valid {
                if let Ok(hints) = Self::get_normal_hints(&self.conn, c.window) {
                    c.apply_size_hints(hints);
                }
            }

            (w, h) = c.adjust_aspect_ratio(w, h);
        }

        Ok(x != c.rect.x || y != c.rect.y || w != c.rect.w || h != c.rect.h)
    }

    fn get_normal_hints(conn: &C, win: Window) -> Result<WmSizeHints, ReplyOrIdError> {
        match WmSizeHints::get_normal_hints(conn, win) {
            Ok(r) => match r.reply() {
                Ok(hints) => Ok(hints),
                Err(e) => Err(e)?,
            },
            Err(e) => Err(e)?,
        }
    }

    fn handle_button_release(&mut self, evt: ButtonReleaseEvent) -> Result<(), ReplyError> {
        if evt.detail == u8::from(DRAG_BUTTON) {
            self.drag_window = None;
        } else if evt.detail == u8::from(RESIZE_BUTTON) {
            self.resize_window = None;
        }
        Ok(())
    }

    fn handle_configure_request(
        &mut self,
        evt: ConfigureRequestEvent,
    ) -> Result<(), ReplyOrIdError> {
        if let Some((mon_idx, client_idx)) = self.win_to_client(evt.window) {
            let mr = self.monitors[mon_idx].rect;
            let WClientState {
                mut rect,
                mut old_rect,
                is_floating,
                bw,
                monitor,
                ..
            } = self.monitors[mon_idx].clients[client_idx];
            let value_mask = WConfigWindow::from(evt.value_mask);
            if value_mask & WConfigWindow::BORDER_WIDTH {
                self.monitors[mon_idx].clients[client_idx].bw = evt.border_width;
            } else if is_floating {
                if value_mask & WConfigWindow::X {
                    old_rect.x = rect.x;
                    rect.x = mr.x + evt.x;
                }

                if value_mask & WConfigWindow::Y {
                    old_rect.y = rect.y;
                    rect.y = mr.y + evt.y;
                }

                if value_mask & WConfigWindow::WIDTH {
                    old_rect.w = rect.w;
                    rect.w = evt.width;
                }

                if value_mask & WConfigWindow::HEIGHT {
                    old_rect.h = rect.h;
                    rect.h = evt.height;
                }

                if rect.x + rect.w as i16 > mr.x + mr.w as i16 && is_floating {
                    rect.x = mr.x + (mr.w as i16 / 2 - (rect.w + 2 * bw) as i16 / 2)
                }
                if rect.y + rect.h as i16 > mr.y + mr.h as i16 && is_floating {
                    rect.y = mr.y + (mr.h as i16 / 2 - (rect.h + 2 * bw) as i16 / 2)
                }

                {
                    let c = &mut self.monitors[mon_idx].clients[client_idx];
                    c.bw = bw;
                    c.rect = rect;
                    c.old_rect = old_rect;
                }
                if value_mask & (WConfigWindow::X | WConfigWindow::Y)
                    && !(value_mask & (WConfigWindow::WIDTH | WConfigWindow::HEIGHT))
                {
                    let WClientState {
                        rect, bw, window, ..
                    } = self.monitors[mon_idx].clients[client_idx];
                    self.configure_client(window, rect, bw)?;
                }
                if monitor == self.selmon {
                    self.resize_client(client_idx, mon_idx, rect.x, rect.y, rect.w, rect.h)?;
                }
            } else {
                let WClientState {
                    rect, bw, window, ..
                } = self.monitors[mon_idx].clients[client_idx];
                self.configure_client(window, rect, bw)?;
            }
        } else if evt.window == self.screen.root {
            let aux = ConfigureWindowAux::from_configure_request(&evt)
                .sibling(None)
                .stack_mode(None);
            self.conn.configure_window(evt.window, &aux)?;
        } else {
            let conf_aux = ConfigureWindowAux::from_configure_request(&evt);
            self.conn.configure_window(evt.window, &conf_aux)?;
        }
        self.conn.sync()?;
        Ok(())
    }

    fn configure_client(&mut self, win: Window, rect: Rect, bw: u16) -> Result<(), ReplyOrIdError> {
        let mut ce = ConfigureNotifyEvent::default();
        ce.response_type = 22; // ConfigureNotify
        ce.event = win;
        ce.window = win;
        ce.x = rect.x;
        ce.y = rect.y;
        ce.width = rect.w;
        ce.height = rect.h;
        ce.border_width = bw;
        ce.override_redirect = false;
        self.conn
            .send_event(false, win, EventMask::STRUCTURE_NOTIFY, ce)?;

        Ok(())
    }

    fn handle_destroy(&mut self, evt: DestroyNotifyEvent) -> Result<(), ReplyOrIdError> {
        if self.win_to_client(evt.window).is_some() {
            self.unmanage(evt.window, true)?;
        }
        Ok(())
    }

    fn handle_enter(&mut self, evt: EnterNotifyEvent) -> Result<(), ReplyOrIdError> {
        if self.ignore_enter {
            self.ignore_enter = false;
            return Ok(());
        }

        if self.resize_window.is_some() || self.drag_window.is_some() {
            return Ok(());
        }

        let mode = u8::from(evt.mode);
        let detail = u8::from(evt.detail);
        let entered_win = evt.event;

        if (mode != NotifyMode::Normal as u8 || detail == NotifyMode::Inferior as u8)
            && entered_win != self.screen.root
        {
            return Ok(());
        }

        if let Some((mon_idx, client_idx)) = self.win_to_client(entered_win) {
            self.unfocus(self.selmon)?;
            self.selmon = mon_idx;
            self.monitors[mon_idx].client = Some(client_idx);
            self.focus()?;
        }

        Ok(())
    }

    fn handle_event(&mut self, evt: Event) -> Result<ShouldExit, ReplyOrIdError> {
        match evt {
            Event::UnmapNotify(e) => self.handle_unmap_notify(e)?,
            Event::ConfigureRequest(e) => self.handle_configure_request(e)?,
            Event::MapRequest(e) => self.handle_map_request(e)?,
            Event::Expose(e) => self.handle_expose(e),
            Event::EnterNotify(e) => self.handle_enter(e)?,
            Event::DestroyNotify(e) => self.handle_destroy(e)?,
            Event::ButtonPress(e) => self.handle_button_press(e)?,
            Event::ButtonRelease(e) => self.handle_button_release(e)?,
            Event::MotionNotify(e) => self.handle_motion_notify(e)?,
            Event::KeyPress(e) => self.handle_key_press(e)?,
            Event::PropertyNotify(e) => self.handle_property_notify(e)?,
            Event::ClientMessage(e) => self.handle_client_message(e)?,
            Event::Error(e) => eprintln!("ERROR: {e:#?}"),
            _ => {}
        }

        Ok(ShouldExit::No)
    }

    fn handle_client_message(&mut self, evt: ClientMessageEvent) -> Result<(), ReplyOrIdError> {
        if evt.type_ == self.atoms._NET_WM_STATE {
            let data = evt.data.as_data32();
            if data[1] == self.atoms._NET_WM_STATE_FULLSCREEN
                || data[2] == self.atoms._NET_WM_STATE_FULLSCREEN
            {
                if let Some((mon_idx, client_idx)) = self.win_to_client(evt.window) {
                    let c = &self.monitors[mon_idx].clients[client_idx];
                    let fullscreen = data[0] == self.atoms._NET_WM_STATE_ADD
                        || (data[0] == self.atoms._NET_WM_STATE_TOGGLE && !c.is_fullscreen);
                    self.fullscreen(mon_idx, fullscreen)?;
                }
            }
        }
        Ok(())
    }

    fn handle_expose(&mut self, evt: ExposeEvent) {
        self.pending_exposure.insert(evt.window);
    }

    fn handle_key_press(&mut self, evt: KeyPressEvent) -> Result<(), ReplyOrIdError> {
        let sym = self.keyboard.key_sym(evt.detail.into());

        let mut action = WKeyCommand::Idle;
        for bind in &self.keyboard.keybinds {
            if bind.keysym == sym && evt.state == bind.mods_as_key_but_mask() {
                action = bind.action;
                break;
            }
        }

        match action {
            WKeyCommand::FocusClient(dir) => self.focus_adjacent(dir)?,
            WKeyCommand::MoveClient(dir) => self.move_adjacent(dir)?,
            WKeyCommand::FocusMonitor(dir) => self.focus_adjacent_monitor(dir)?,
            WKeyCommand::Spawn(cmd) => self.spawn_program(cmd),
            WKeyCommand::Destroy => self.destroy_window()?,
            WKeyCommand::AdjustMainWidth(dir) => self.adjust_main_width(dir)?,
            WKeyCommand::Layout(layout) => self.update_layout(layout),
            WKeyCommand::SelectWorkspace(idx) => self.select_tag(idx, true)?,
            WKeyCommand::MoveClientToWorkspace(ws_idx) => self.move_client_to_tag(ws_idx)?,
            WKeyCommand::MoveClientToMonitor(dir) => self.move_client_to_monitor(dir)?,
            WKeyCommand::UnFloat => self.unfloat_focused_client()?,
            WKeyCommand::Fullscreen => self.fullscreen_focused_client()?,
            WKeyCommand::Exit => self.try_exit(),
            _ => {}
        }
        Ok(())
    }

    fn fullscreen_focused_client(&mut self) -> Result<(), ReplyOrIdError> {
        let m = &self.monitors[self.selmon];
        if let Some(c) = m.selected_client() {
            let fullscreen = !c.is_fullscreen;
            self.fullscreen(self.selmon, fullscreen)?;
        }
        Ok(())
    }

    fn fullscreen(&mut self, mon_idx: usize, fullscreen: bool) -> Result<(), ReplyOrIdError> {
        let rect = self.monitors[mon_idx].rect;
        let idx = self.monitors[mon_idx].client.unwrap();
        if let Some(c) = self.monitors[mon_idx].selected_client_mut() {
            if fullscreen && !c.is_fullscreen {
                self.conn.change_property32(
                    PropMode::REPLACE,
                    c.window,
                    self.atoms._NET_WM_STATE,
                    self.atoms.ATOM,
                    &[self.atoms._NET_WM_STATE_FULLSCREEN],
                )?;
                c.is_fullscreen = true;
                c.old_state = c.is_floating;
                c.old_bw = c.bw;
                c.bw = 0;
                c.is_floating = true;
                let bh = util::bar_height();
                self.resize_client(
                    idx,
                    mon_idx,
                    rect.x,
                    rect.y - bh as i16,
                    rect.w,
                    rect.h + bh,
                )?;
            } else if !fullscreen && c.is_fullscreen {
                self.conn.change_property32(
                    PropMode::REPLACE,
                    c.window,
                    self.atoms._NET_WM_STATE,
                    self.atoms.ATOM,
                    &[0],
                )?;
                c.is_fullscreen = false;
                c.is_floating = c.old_state;
                c.bw = c.old_bw;
                let r = c.old_rect;
                self.resize_client(idx, mon_idx, r.x, r.y, r.w, r.h)?;
                self.recompute_layout(self.selmon)?;
            }
        }

        Ok(())
    }

    fn unfloat_focused_client(&mut self) -> Result<(), ReplyOrIdError> {
        let m = &mut self.monitors[self.selmon];
        if let Some(c) = m.selected_client_mut() {
            if !c.is_floating {
                return Ok(());
            }

            c.is_floating = false;
            let pos = Pos::new(c.rect.x + (c.rect.w as i16 / 2), c.rect.y);

            if let Some(dir) = m.find_adjacent_monitor(pos) {
                self.move_client_to_monitor(dir).unwrap();
            }
            self.recompute_layout(self.selmon)?;
            self.warp_pointer_to_focused_client()?;
        }
        Ok(())
    }

    fn adjust_main_width(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        let mut m = &mut self.monitors[self.selmon];
        match dir {
            WDirection::Prev if m.width_factor - WIDTH_ADJUSTMENT_FACTOR >= 0.05 => {
                m.width_factor -= WIDTH_ADJUSTMENT_FACTOR;
            }
            WDirection::Next if m.width_factor <= 0.95 => {
                m.width_factor += WIDTH_ADJUSTMENT_FACTOR;
            }
            _ => {}
        }
        self.recompute_layout(self.selmon)?;
        Ok(())
    }

    fn move_client_to_monitor(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        if self.monitors[self.selmon].client.is_none() {
            return Ok(());
        }

        let idx = match dir {
            WDirection::Prev if self.selmon == 0 => self.monitors.len() - 1,
            WDirection::Prev => self.selmon - 1,
            WDirection::Next if self.selmon == self.monitors.len() - 1 => 0,
            WDirection::Next => self.selmon + 1,
        };

        if idx == self.selmon {
            return Ok(());
        }

        self.unfocus(self.selmon)?;

        let m = &mut self.monitors[self.selmon];
        let mut c = m.remove_client(m.client.unwrap());

        let dest_mon = &mut self.monitors[idx];
        c.monitor = idx;
        c.tag = dest_mon.tag;

        dest_mon.push_client(c);

        self.recompute_layout(idx)?;
        self.recompute_layout(self.selmon)?;

        self.focus()?;
        self.warp_pointer_to_focused_client()?;
        Ok(())
    }

    fn move_client_to_tag(&mut self, new_tag: usize) -> Result<(), ReplyOrIdError> {
        if self.monitors[self.selmon].client.is_none() || self.monitors[self.selmon].tag == new_tag
        {
            return Ok(());
        }

        self.unfocus(self.selmon)?;
        self.monitors[self.selmon].client_to_tag(&self.conn, new_tag)?;
        self.focus()?;
        self.recompute_layout(self.selmon)?;
        Ok(())
    }

    fn handle_map_request(&mut self, evt: MapRequestEvent) -> Result<(), ReplyOrIdError> {
        match self.conn.get_window_attributes(evt.window) {
            Ok(reply) => match reply.reply() {
                Ok(wa) if wa.override_redirect => return Ok(()),
                _ => {}
            },
            _ => {}
        };

        if self.win_to_client(evt.window).is_some() {
            return Ok(());
        }

        let geom = match self.conn.get_geometry(evt.window) {
            Ok(geom_reply) => match geom_reply.reply() {
                Ok(geom) => geom,
                Err(e) => {
                    println!(
                        "ERROR: unwrap geometry reply for win {}: {e:#?}",
                        evt.window
                    );
                    return Ok(());
                }
            },
            Err(e) => {
                println!("ERROR: geometry request for win {}: {e:#?}", evt.window);
                return Ok(());
            }
        };

        self.manage_window(evt.window, &geom)
    }

    fn handle_motion_notify(&mut self, evt: MotionNotifyEvent) -> Result<(), ReplyOrIdError> {
        let m = &self.monitors[self.selmon];
        if m.bar.has_pointer(evt.root_x, evt.root_y) {
            return Ok(());
        }

        let mon_has_pointer = m.has_pos(Pos::from(&evt));

        // skip monitor focus change if a window is being manipulated
        if !mon_has_pointer && self.drag_window.is_none() && self.resize_window.is_none() {
            self.focus_at_pointer(&evt)?;
        }

        if let Some(drag_info) = self.drag_window {
            self.mouse_move(drag_info, evt)?;
        }

        if let Some(last_time) = self.resize_window {
            self.mouse_resize(last_time, evt)?;
        }

        Ok(())
    }

    fn mouse_resize(
        &mut self,
        last_resize: u32,
        ev: MotionNotifyEvent,
    ) -> Result<(), ReplyOrIdError> {
        let m = &self.monitors[self.selmon];
        if let Some(idx) = m.client {
            let c = m.clients[idx];
            if c.is_fullscreen || ev.time - last_resize <= (1000 / 60) {
                return Ok(());
            }

            if c.is_floating {
                let nw = 1.max(ev.root_x - c.rect.x - (2 * BORDER_WIDTH as i16) + 1) as u16;
                let nh = 1.max(ev.root_y - c.rect.y - (2 * BORDER_WIDTH as i16) + 1) as u16;

                // copy before move
                let x = c.rect.x;
                let y = c.rect.y;

                self.resize(idx, self.selmon, x, y, nw, nh, true)?;
            }
        }
        Ok(())
    }

    fn resize(
        &mut self,
        c_idx: usize,
        mon_idx: usize,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        interact: bool,
    ) -> Result<(), ReplyOrIdError> {
        if let Ok(succeeded) = self.apply_size_hints(c_idx, mon_idx, x, y, w, h, interact) {
            if succeeded {
                self.resize_client(c_idx, mon_idx, x, y, w, h)?;
            }
        }

        Ok(())
    }

    fn resize_client(
        &mut self,
        c_idx: usize,
        mon_idx: usize,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
    ) -> Result<(), ReplyOrIdError> {
        {
            let m = &mut self.monitors[mon_idx];
            let c = &mut m.clients[c_idx];
            c.rect = Rect::new(x, y, w, h);

            self.conn.configure_window(
                c.window,
                &ConfigureWindowAux::new()
                    .x(x as i32)
                    .y(y as i32)
                    .width(w as u32)
                    .height(h as u32)
                    .border_width(c.bw as u32),
            )?;
        }

        let WClientState {
            window, rect, bw, ..
        } = self.monitors[mon_idx].clients[c_idx];
        self.configure_client(window, rect, bw)?;
        Ok(())
    }

    fn mouse_move(
        &mut self,
        (oc_pos, op_pos, last_move): (Pos, Pos, u32),
        ev: MotionNotifyEvent,
    ) -> Result<(), ReplyOrIdError> {
        if let Some(c) = &mut self.monitors[self.selmon].selected_client_mut() {
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
            self.conn
                .configure_window(c.window, &ConfigureWindowAux::new().x(nx).y(ny))?;
            self.conn.flush()?;
        }
        Ok(())
    }

    fn handle_property_notify(&mut self, evt: PropertyNotifyEvent) -> Result<(), ReplyOrIdError> {
        if evt.atom == self.atoms._NET_WM_NAME {
            let title = self.get_window_title(evt.window)?;
            self.monitors[self.selmon]
                .bar
                .update_title(self.conn, title);
        }
        Ok(())
    }

    fn handle_unmap_notify(&mut self, evt: UnmapNotifyEvent) -> Result<(), ReplyOrIdError> {
        if self.win_to_client(evt.window).is_some() {
            self.unmanage(evt.window, false)?;
        }
        Ok(())
    }

    fn manage_window(
        &mut self,
        win: Window,
        geom: &GetGeometryReply,
    ) -> Result<(), ReplyOrIdError> {
        let is_floating = self.window_property_exists(
            win,
            self.atoms._NET_WM_WINDOW_TYPE_DIALOG,
            self.atoms._NET_WM_WINDOW_TYPE,
            self.atoms.ATOM,
        )?;

        let is_fullscreen = self.window_property_exists(
            win,
            self.atoms._NET_WM_STATE_FULLSCREEN,
            self.atoms._NET_WM_STATE,
            self.atoms.ATOM,
        )?;

        let mut trans = None;
        if let Ok(reply) = self.conn.get_property(
            false,
            win,
            self.atoms.WM_TRANSIENT_FOR,
            self.atoms.WINDOW,
            0,
            u32::MAX,
        ) {
            if let Ok(transient_for) = reply.reply() {
                if let Some(mut it) = transient_for.value32() {
                    trans = it.next();
                }
            }
        }
        let (mrect, mtag) = {
            let m = &self.monitors[self.selmon];
            (m.rect, m.tag)
        };

        let (mx, my, mw, mh) = (mrect.x, mrect.y, mrect.w, mrect.h);

        let rect = if is_fullscreen {
            mrect
        } else {
            let mut rect = Rect::from(geom);

            if rect.x + rect.w as i16 > mx + mw as i16 {
                rect.x = mx + mw as i16 - rect.w as i16 - (theme::window::BORDER_WIDTH as i16 * 2)
            }
            if rect.y + rect.h as i16 > my + mh as i16 {
                rect.y = my + mh as i16 + rect.h as i16 - (theme::window::BORDER_WIDTH as i16 * 2)
            }

            rect.x = rect.x.max(mx);
            rect.y = rect.y.max(my);
            rect
        };

        let mut c = WClientState::new(
            win,
            rect,
            rect, // we use the same rect here for now
            is_floating,
            is_fullscreen,
            mtag,
            self.selmon,
        );

        if let Ok(hints) = Self::get_normal_hints(&self.conn, win) {
            c.apply_size_hints(hints);
        }

        if !is_floating {
            c.old_state = trans.is_some() || c.is_fixed;
            c.is_floating = c.old_state;
        }

        if let Some(t) = trans {
            if let Some((mon_idx, _)) = self.win_to_client(t) {
                c.monitor = mon_idx;
            }
        }

        if c.is_floating {
            self.conn
                .configure_window(win, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))?;
        }

        let change_aux = ChangeWindowAttributesAux::new()
            .border_pixel(theme::window::BORDER_UNFOCUSED)
            .event_mask(
                EventMask::ENTER_WINDOW
                    | EventMask::FOCUS_CHANGE
                    | EventMask::PROPERTY_CHANGE
                    | EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::STRUCTURE_NOTIFY,
            );

        self.configure_client(win, rect, c.bw)?;
        self.conn.change_window_attributes(win, &change_aux)?;

        if c.monitor == self.selmon {
            self.unfocus(self.selmon)?;
        }

        self.monitors[c.monitor].push_client(c);
        self.update_client_list()?;

        self.set_client_state(win, WindowState::Normal)?;

        self.recompute_layout(c.monitor)?;
        self.conn.map_window(win)?;

        if is_fullscreen {
            self.fullscreen(c.monitor, true)?;
        }

        if c.monitor == self.selmon {
            self.conn
                .warp_pointer(NONE, win, 0, 0, 0, 0, rect.w as i16 / 2, rect.h as i16 / 2)?;
        }
        self.focus()?;

        self.conn.flush()?;
        self.conn.sync()?;

        Ok(())
    }

    fn win_to_client(&self, win: Window) -> Option<(usize, usize)> {
        for (mi, m) in self.monitors.iter().enumerate() {
            for (ci, c) in m.clients.iter().enumerate() {
                if c.window == win {
                    drop(m);
                    return Some((mi, ci));
                }
            }
        }
        None
    }

    fn move_adjacent(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        let m = &mut self.monitors[self.selmon];
        m.swap_clients(dir);
        self.ignore_enter = true;
        self.recompute_layout(self.selmon)?;
        self.warp_pointer_to_focused_client()?;
        Ok(())
    }

    fn recompute_layout(&mut self, mon_idx: usize) -> Result<(), ReplyOrIdError> {
        let mon = &mut self.monitors[mon_idx];
        let client_indices = mon.clients_in_tag(mon.tag);
        let client_indices: Vec<_> = client_indices
            .into_iter()
            .filter(|i| !mon.clients[*i].is_floating)
            .collect();

        let rects = layout_clients(&mon.layout, mon.width_factor, &mon, client_indices.len());

        if rects.is_none() {
            return Ok(());
        }

        for (i, rect) in client_indices.iter().zip(rects.unwrap()) {
            self.resize(*i, mon_idx, rect.x, rect.y, rect.w, rect.h, false)?;
        }
        self.conn.sync()?;
        Ok(())
    }

    fn run_auto_start_commands() -> Result<(), std::io::Error> {
        for cmd in AUTO_START_COMMANDS {
            if let Some((bin, args)) = util::cmd_bits(cmd) {
                Command::new(bin).args(args).spawn()?;
            }
        }
        Ok(())
    }

    fn scan_windows(&mut self) -> Result<(), ReplyOrIdError> {
        let tree_reply = self.conn.query_tree(self.screen.root)?.reply()?;

        for win in tree_reply.children {
            let attr = self.conn.get_window_attributes(win)?;
            let geom = self.conn.get_geometry(win)?;

            if let (Ok(attr), Ok(geom)) = (attr.reply(), geom.reply()) {
                if !attr.override_redirect && attr.map_state != MapState::UNMAPPED {
                    self.manage_window(win, &geom)?;
                }
            }
        }

        Ok(())
    }

    fn select_tag(&mut self, new_tag: usize, warp_pointer: bool) -> Result<(), ReplyOrIdError> {
        if self.monitors[self.selmon].tag == new_tag {
            return Ok(());
        }

        {
            self.unfocus(self.selmon)?;
            let m = &mut self.monitors[self.selmon];
            m.hide_clients(&self.conn, m.tag)?;

            m.set_tag(new_tag).unwrap();
            m.bar.update_tags(new_tag);
        }

        let title = if let Some(WClientState { window, .. }) =
            self.monitors[self.selmon].selected_client()
        {
            let win = *window;
            self.get_window_title(win)?
        } else {
            String::new()
        };
        self.monitors[self.selmon]
            .bar
            .update_title(self.conn, title);

        self.recompute_layout(self.selmon)?;
        self.focus()?;

        if warp_pointer {
            self.warp_pointer_to_focused_client()?;
        }

        Ok(())
    }

    fn send_event(&self, window: Window, proto: u32) -> Result<(), ReplyError> {
        let event = ClientMessageEvent::new(
            32,
            window,
            self.atoms.WM_PROTOCOLS,
            [proto, CURRENT_TIME, 0, 0, 0],
        );
        self.conn
            .send_event(false, window, EventMask::NO_EVENT, event)?;
        Ok(())
    }

    fn set_client_state(&mut self, win: Window, state: WindowState) -> Result<(), ReplyOrIdError> {
        self.conn.change_property(
            PropMode::REPLACE,
            win,
            self.atoms.WM_STATE,
            self.atoms.WM_STATE,
            8,
            2,
            &[state as u8, 0],
        )?;
        Ok(())
    }

    fn spawn_program(&self, cmd: &'static [&'static str]) {
        if let Some((bin, args)) = util::cmd_bits(cmd) {
            Command::new(bin).args(args).spawn().unwrap();
        }
    }

    fn try_exit(&mut self) {
        if self.should_exit.load(Ordering::Relaxed) {
            exit(0)
        }

        let should_exit = Arc::clone(&self.should_exit);

        thread::spawn(move || {
            should_exit.store(true, Ordering::Relaxed);
            thread::sleep(Duration::from_secs(2));
            should_exit.store(false, Ordering::Relaxed);
        });
    }

    fn unfocus(&mut self, mon_idx: usize) -> Result<(), ReplyError> {
        let m = &mut self.monitors[mon_idx];
        if let Some(c) = m.selected_client() {
            let unfocus_aux =
                ChangeWindowAttributesAux::new().border_pixel(theme::window::BORDER_UNFOCUSED);
            self.conn.change_window_attributes(c.window, &unfocus_aux)?;
            self.conn
                .delete_property(c.window, self.atoms._NET_ACTIVE_WINDOW)?;
        }

        Ok(())
    }

    fn unmanage(&mut self, win: Window, destroyed: bool) -> Result<(), ReplyOrIdError> {
        if let Some((mon_idx, _)) = self.win_to_client(win) {
            self.detach(win, mon_idx);
            if !destroyed {
                self.conn.grab_server()?;
                self.conn
                    .set_input_focus(InputFocus::POINTER_ROOT, win, CURRENT_TIME)?;
                self.set_client_state(win, WindowState::Withdrawn)?;
                self.conn.sync()?;
                self.conn.ungrab_server()?;
            }

            if mon_idx == self.selmon {
                self.focus()?;
            }

            self.recompute_layout(mon_idx)?;
            self.update_client_list()?;

            if self.monitors[self.selmon].client.is_some() {
                self.warp_pointer_to_focused_client()?;
            }

            self.conn.sync()?;
        }
        Ok(())
    }

    fn update_client_list(&self) -> Result<(), ReplyOrIdError> {
        self.conn
            .delete_property(self.screen.root, self.atoms._NET_CLIENT_LIST)?;
        self.for_all_clients(|c| {
            self.conn
                .change_property(
                    PropMode::APPEND,
                    self.screen.root,
                    self.atoms._NET_CLIENT_LIST,
                    self.atoms.WINDOW,
                    32,
                    1,
                    &c.window.to_ne_bytes(),
                )
                .unwrap();
            true
        });
        Ok(())
    }

    fn update_layout(&mut self, layout: WLayout) {
        let m = &mut self.monitors[self.selmon];
        if m.set_layout(layout) {
            m.bar.update_layout_symbol(self.conn, m.layout.to_string());
            self.recompute_layout(self.selmon).unwrap();
        }
    }

    fn warp_pointer_to_focused_client(&self) -> Result<(), ReplyOrIdError> {
        if let Some(c) = self.monitors[self.selmon].selected_client() {
            if let Ok(pointer_reply) = self.conn.query_pointer(c.window) {
                if let Ok(pointer) = pointer_reply.reply() {
                    if !pointer.same_screen {
                        return Ok(());
                    }
                    self.conn.warp_pointer(
                        NONE,
                        c.window,
                        0,
                        0,
                        0,
                        0,
                        c.rect.w as i16 / 2,
                        c.rect.h as i16 / 2,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn warp_pointer_to_focused_monitor(&self) -> Result<(), ReplyOrIdError> {
        let m = &self.monitors[self.selmon];
        self.conn.warp_pointer(
            NONE,
            self.screen.root,
            0,
            0,
            0,
            0,
            m.rect.x + (m.rect.w as i16 / 2),
            m.rect.y + (m.rect.h as i16 / 2),
        )?;
        Ok(())
    }

    fn window_property_exists(
        &mut self,
        window: Window,
        atom: u32,
        prop: u32,
        type_: u32,
    ) -> Result<bool, ReplyError> {
        if let Ok(reply) = self
            .conn
            .get_property(false, window, prop, type_, 0, u32::MAX)
        {
            if let Ok(reply) = reply.reply() {
                let found = match reply.format {
                    8 => reply.value8().unwrap().any(|a| a == atom as u8),
                    16 => reply.value16().unwrap().any(|a| a == atom as u16),
                    32 => reply.value32().unwrap().any(|a| a == atom),
                    _ => false,
                };
                return Ok(found);
            }
        }
        Ok(false)
    }
}

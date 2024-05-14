use crate::{
    client::{WClientState, WindowState},
    command::{WDirection, WKeyCommand, WMouseCommand},
    config::{
        auto_start::AUTO_START_COMMANDS,
        mouse::{DRAG_BUTTON, RESIZE_BUTTON},
        tags::WIDTH_ADJUSTMENT_FACTOR,
        theme::{self, window::BORDER_WIDTH},
    },
    keyboard::WKeyboard,
    monitor::WMonitor,
    mouse::WMouse,
    X_HANDLE,
};
use wwm_core::util::{
    primitives::{WPos, WRect},
    WLayout,
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
use wwm_core::text::TextRenderer;
use x11rb::{
    connection::Connection,
    protocol::{
        randr::ConnectionExt as _,
        xproto::{
            ButtonPressEvent, ButtonReleaseEvent, ChangeWindowAttributesAux, ClientMessageEvent,
            ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt, DestroyNotifyEvent,
            EnterNotifyEvent, EventMask, ExposeEvent, GetGeometryReply, KeyPressEvent,
            MapRequestEvent, MapState, MotionNotifyEvent, PropMode, PropertyNotifyEvent, StackMode,
            UnmapNotifyEvent, Window,
        },
        ErrorKind, Event,
    },
    rust_connection::{ReplyError, ReplyOrIdError},
    wrapper::ConnectionExt as _,
    xcb_ffi::XCBConnection,
    NONE,
};

#[repr(u8)]
enum NotifyMode {
    Normal,
    Inferior,
}

pub struct WinMan<'a> {
    #[allow(dead_code)]
    text_renderer: Rc<TextRenderer<'a, XCBConnection>>,
    monitors: Vec<WMonitor<'a>>,
    current_monitor: usize,
    pending_exposure: HashSet<Window>,
    drag_window: Option<(WPos, WPos, u32)>,
    resize_window: Option<u32>,
    keyboard: WKeyboard,
    mouse: WMouse,
    ignore_enter: bool,
    should_exit: Arc<AtomicBool>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum ShouldExit {
    Yes,
    No,
}

impl<'a> WinMan<'a> {
    pub fn init(keyboard: WKeyboard, mouse: WMouse) -> Result<Self, ReplyOrIdError> {
        let conn = &X_HANDLE.conn;
        let screen = X_HANDLE.screen();
        // TODO: error handling

        Self::become_wm(mouse.cursors.normal)?;
        Self::run_auto_start_commands().unwrap();

        let text_renderer =
            TextRenderer::new(conn, screen, theme::bar::FONT, theme::bar::FONT_SIZE).unwrap();
        let text_renderer = Rc::new(text_renderer);

        let mut monitors: Vec<WMonitor<'a>> = Self::get_monitors(&text_renderer)?.into();

        let primary_monitor_index = monitors.iter().position(|m| m.primary).unwrap_or(0);

        let primary_monitor = &mut monitors[primary_monitor_index];
        primary_monitor.bar.set_is_focused(true);
        primary_monitor.warp_pointer()?;

        let mut wwm = Self {
            text_renderer,
            monitors,
            current_monitor: primary_monitor_index,
            pending_exposure: Default::default(),
            drag_window: None,
            resize_window: None,
            keyboard,
            mouse,
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
            loop {
                X_HANDLE.conn.flush()?;
                if let Ok(Some(event)) = X_HANDLE.conn.poll_for_event() {
                    if self.handle_event(event)? == ShouldExit::Yes {
                        break 'eventloop;
                    }
                }
                for m in self.monitors.iter_mut() {
                    m.bar.draw(&X_HANDLE.conn);
                }
            }
        }
        Ok(())
    }

    fn become_wm(cursor: u32) -> Result<(), ReplyError> {
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

        let res = X_HANDLE
            .conn
            .change_window_attributes(X_HANDLE.screen().root, &change)
            .unwrap()
            .check();

        if let Err(ReplyError::X11Error(ref error)) = res {
            if error.error_kind == ErrorKind::Access {
                eprintln!("ERROR: Another WM is already running.");
                exit(1);
            }
        }

        X_HANDLE.conn.sync()?;

        res
    }

    fn destroy_window(&mut self) -> Result<(), ReplyOrIdError> {
        self.monitors[self.current_monitor].destroy_current_client()?;
        Ok(())
    }

    fn get_window_title(&mut self, window: Window) -> Result<String, ReplyOrIdError> {
        if let Ok(reply) = X_HANDLE.conn.get_property(
            false,
            window,
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
    fn focus_adjacent(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        let monitor = &mut self.monitors[self.current_monitor];
        monitor.unfocus_current_client()?;
        monitor.select_adjacent(dir);
        monitor.focus_current_client(true)?;
        Ok(())
    }

    fn focus_adjacent_monitor(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        self.monitors[self.current_monitor].unfocus()?;

        let selmon = match dir {
            WDirection::Prev if self.current_monitor == 0 => self.monitors.len() - 1,
            WDirection::Prev => self.current_monitor - 1,
            WDirection::Next if self.current_monitor == self.monitors.len() - 1 => 0,
            WDirection::Next => self.current_monitor + 1,
        };

        // change selected monitor
        self.current_monitor = selmon;
        self.monitors[self.current_monitor].focus()?;
        Ok(())
    }

    fn focus_at_pointer(&mut self, evt: &MotionNotifyEvent) -> Result<(), ReplyOrIdError> {
        let pos = WPos::from(evt);

        let current_monitor = &mut self.monitors[self.current_monitor];
        if !current_monitor.has_pos(&pos) {
            current_monitor.unfocus()?;
        }

        for (i, m) in self.monitors.iter_mut().enumerate() {
            if m.has_pos(&pos) && i != self.current_monitor {
                self.current_monitor = i;
                m.focus()?;
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
        text_renderer: &Rc<TextRenderer<'a, XCBConnection>>,
    ) -> Result<Vec<WMonitor<'a>>, ReplyError> {
        let monitors = X_HANDLE
            .conn
            .randr_get_monitors(X_HANDLE.screen().root, true)?
            .reply()?;
        let monitors: Vec<WMonitor> = monitors
            .monitors
            .iter()
            .map(|m| WMonitor::new(m, Rc::clone(text_renderer)))
            .collect();
        Ok(monitors)
    }

    fn handle_button_press(&mut self, evt: ButtonPressEvent) -> Result<(), ReplyOrIdError> {
        let m = &mut self.monitors[self.current_monitor];
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
        let m = &mut self.monitors[self.current_monitor];
        if let Some(ci) = m.client {
            let c = &mut m.clients[ci];
            // is outside
            if evt.root_x > c.rect.x.max(c.rect.x + c.rect.w as i16) {
                return Ok(());
            }

            let mut should_recompute_layout = false;
            match action {
                WMouseCommand::DragClient if self.drag_window.is_none() => {
                    self.drag_window = Some((
                        WPos::from(c.rect),
                        WPos::new(evt.root_x, evt.root_y),
                        evt.time,
                    ));
                    should_recompute_layout = true;
                }
                WMouseCommand::ResizeClient if self.resize_window.is_none() => {
                    X_HANDLE.conn.warp_pointer(
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
            X_HANDLE.conn.configure_window(
                c.window,
                &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            )?;

            c.is_floating = true;
            m.recompute_layout()?;
        }
        Ok(())
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
        if let Some((monitor_index, client_index)) = self.win_to_client(evt.window) {
            let m = &mut self.monitors[monitor_index];
            m.handle_configure_request(client_index, evt, monitor_index == self.current_monitor)?;
        } else if evt.window == X_HANDLE.screen().root {
            let configure_aux = ConfigureWindowAux::from_configure_request(&evt)
                .sibling(None)
                .stack_mode(None);
            X_HANDLE.conn.configure_window(evt.window, &configure_aux)?;
        } else {
            let configure_aux = ConfigureWindowAux::from_configure_request(&evt);
            X_HANDLE.conn.configure_window(evt.window, &configure_aux)?;
        }
        X_HANDLE.conn.sync()?;
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
            && entered_win != X_HANDLE.screen().root
        {
            return Ok(());
        }

        if let Some((mon_idx, client_idx)) = self.win_to_client(entered_win) {
            self.monitors[self.current_monitor].unfocus_current_client()?;

            self.current_monitor = mon_idx;
            self.monitors[self.current_monitor].set_current_client(client_idx)?;
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
        if evt.type_ == X_HANDLE.atoms._NET_WM_STATE {
            let data = evt.data.as_data32();
            if data[1] == X_HANDLE.atoms._NET_WM_STATE_FULLSCREEN
                || data[2] == X_HANDLE.atoms._NET_WM_STATE_FULLSCREEN
            {
                if let Some((mon_idx, client_idx)) = self.win_to_client(evt.window) {
                    let monitor = &mut self.monitors[mon_idx];
                    let monitor_rect = &monitor.rect;
                    let c = &mut monitor.clients[client_idx];
                    let fullscreen = data[0] == X_HANDLE.atoms._NET_WM_STATE_ADD
                        || (data[0] == X_HANDLE.atoms._NET_WM_STATE_TOGGLE && !c.is_fullscreen);
                    if fullscreen {
                        c.fullscreen(monitor_rect)?;
                    } else {
                        c.exit_fullscreen(monitor_rect)?;
                    }
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
            WKeyCommand::Layout(layout) => self.update_layout(layout)?,
            WKeyCommand::SelectTag(idx) => self.select_tag(idx, true)?,
            WKeyCommand::MoveClientToTag(ws_idx) => self.move_client_to_tag(ws_idx)?,
            WKeyCommand::MoveClientToMonitor(dir) => self.move_client_to_monitor(dir)?,
            WKeyCommand::UnFloat => self.unfloat_focused_client()?,
            WKeyCommand::Fullscreen => self.fullscreen_focused_client()?,
            WKeyCommand::Exit => self.try_exit(),
            _ => {}
        }
        Ok(())
    }

    fn fullscreen_focused_client(&mut self) -> Result<(), ReplyOrIdError> {
        self.monitors[self.current_monitor].fullscreen_focused_client()
    }

    fn unfloat_focused_client(&mut self) -> Result<(), ReplyOrIdError> {
        if let Ok(Some(direction)) = self.monitors[self.current_monitor].unfloat_focused_client() {
            self.move_client_to_monitor(direction)?;
        }
        Ok(())
    }

    fn adjust_main_width(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        let m = &mut self.monitors[self.current_monitor];
        match dir {
            WDirection::Prev if m.width_factor - WIDTH_ADJUSTMENT_FACTOR >= 0.05 => {
                m.width_factor -= WIDTH_ADJUSTMENT_FACTOR;
            }
            WDirection::Next if m.width_factor <= 0.95 => {
                m.width_factor += WIDTH_ADJUSTMENT_FACTOR;
            }
            _ => {}
        }
        m.recompute_layout()?;
        Ok(())
    }

    fn move_client_to_monitor(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        let monitor_count = self.monitors.len();

        let current_monitor = &mut self.monitors[self.current_monitor];

        let current_client_index = if let Some(client_index) = current_monitor.client {
            client_index
        } else {
            return Ok(());
        };

        let destination_monitor_index = match dir {
            WDirection::Prev if self.current_monitor == 0 => monitor_count - 1,
            WDirection::Prev => self.current_monitor - 1,
            WDirection::Next if self.current_monitor == monitor_count - 1 => 0,
            WDirection::Next => self.current_monitor + 1,
        };

        if destination_monitor_index == self.current_monitor {
            return Ok(());
        }

        current_monitor.unfocus_current_client()?;

        if let Ok(c) = current_monitor.remove_client(current_client_index) {
            self.monitors[destination_monitor_index]
                .push_and_focus_client(c, destination_monitor_index)?;
        }

        Ok(())
    }

    fn move_client_to_tag(&mut self, new_tag: usize) -> Result<(), ReplyOrIdError> {
        self.monitors[self.current_monitor].move_client_to_tag(new_tag)
    }

    fn handle_map_request(&mut self, evt: MapRequestEvent) -> Result<(), ReplyOrIdError> {
        match X_HANDLE.conn.get_window_attributes(evt.window) {
            Ok(reply) => match reply.reply() {
                Ok(wa) if wa.override_redirect => return Ok(()),
                _ => {}
            },
            _ => {}
        };

        if self.win_to_client(evt.window).is_some() {
            return Ok(());
        }

        let geom = match X_HANDLE.conn.get_geometry(evt.window) {
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
        let m = &mut self.monitors[self.current_monitor];
        if m.bar.has_pointer(evt.root_x, evt.root_y) {
            return Ok(());
        }

        if let Some(last_time) = self.resize_window {
            m.mouse_resize_client(last_time, evt)?;
        } else if let Some(drag_info) = self.drag_window {
            m.mouse_move(drag_info, evt)?;
        } else if !m.has_pos(&WPos::from(&evt))
            && self.drag_window.is_none()
            && self.resize_window.is_none()
        {
            self.focus_at_pointer(&evt)?;
        }

        Ok(())
    }

    fn handle_property_notify(&mut self, evt: PropertyNotifyEvent) -> Result<(), ReplyOrIdError> {
        if evt.atom == X_HANDLE.atoms._NET_WM_NAME {
            let title = self.get_window_title(evt.window)?;
            self.monitors[self.current_monitor].bar.update_title(title);
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
            X_HANDLE.atoms._NET_WM_WINDOW_TYPE_DIALOG,
            X_HANDLE.atoms._NET_WM_WINDOW_TYPE,
            X_HANDLE.atoms.ATOM,
        )?;

        let is_fullscreen = self.window_property_exists(
            win,
            X_HANDLE.atoms._NET_WM_STATE_FULLSCREEN,
            X_HANDLE.atoms._NET_WM_STATE,
            X_HANDLE.atoms.ATOM,
        )?;

        let mut trans = None;
        if let Ok(reply) = X_HANDLE.conn.get_property(
            false,
            win,
            X_HANDLE.atoms.WM_TRANSIENT_FOR,
            X_HANDLE.atoms.WINDOW,
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
            let m = &self.monitors[self.current_monitor];
            (m.rect, m.tag)
        };

        let (mx, my, mw, mh) = (mrect.x, mrect.y, mrect.w, mrect.h);

        let rect = if is_fullscreen {
            mrect
        } else {
            let mut rect = WRect::from(geom);

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
            rect, //NOTE: we use the same rect here for now
            is_floating,
            is_fullscreen,
            mtag,
            self.current_monitor,
        );

        c.apply_normal_hints()?;

        if is_floating {
            c.float()?;
        } else {
            c.old_state = trans.is_some() || c.is_fixed;
            c.is_floating = c.old_state;
        }

        if let Some(t) = trans {
            if let Some((mon_idx, _)) = self.win_to_client(t) {
                c.monitor = mon_idx;
            }
        }

        c.set_initial_window_attributes()?;

        let current_monitor = &mut self.monitors[self.current_monitor];

        current_monitor.unfocus_current_client()?;

        c.set_state(WindowState::Normal)?;

        if is_fullscreen {
            c.fullscreen(&current_monitor.rect)?;
        }

        current_monitor.push_and_focus_client(c, self.current_monitor)?;

        self.update_client_list()?;

        X_HANDLE.conn.map_window(win)?;

        X_HANDLE.conn.flush()?;

        Ok(())
    }

    fn win_to_client(&self, win: Window) -> Option<(usize, usize)> {
        for (mi, m) in self.monitors.iter().enumerate() {
            for (ci, c) in m.clients.iter().enumerate() {
                if c.window == win {
                    return Some((mi, ci));
                }
            }
        }
        None
    }

    fn move_adjacent(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        let monitor = &mut self.monitors[self.current_monitor];
        monitor.swap_clients(dir)?;
        self.ignore_enter = true;
        monitor.recompute_layout()?;
        monitor.warp_pointer()?;
        Ok(())
    }

    fn run_auto_start_commands() -> Result<(), std::io::Error> {
        for cmd in AUTO_START_COMMANDS {
            if let Some((bin, args)) = wwm_core::util::cmd::format(cmd) {
                Command::new(bin).args(args).spawn()?;
            }
        }
        Ok(())
    }

    fn scan_windows(&mut self) -> Result<(), ReplyOrIdError> {
        let tree_reply = X_HANDLE.conn.query_tree(X_HANDLE.screen().root)?.reply()?;

        for win in tree_reply.children {
            let attr = X_HANDLE.conn.get_window_attributes(win)?;
            let geom = X_HANDLE.conn.get_geometry(win)?;

            if let (Ok(attr), Ok(geom)) = (attr.reply(), geom.reply()) {
                if !attr.override_redirect && attr.map_state != MapState::UNMAPPED {
                    self.manage_window(win, &geom)?;
                }
            }
        }

        Ok(())
    }

    fn select_tag(&mut self, new_tag: usize, warp_pointer: bool) -> Result<(), ReplyOrIdError> {
        if self.monitors[self.current_monitor].tag == new_tag {
            return Ok(());
        }

        {
            self.unfocus(self.current_monitor)?;
            let m = &mut self.monitors[self.current_monitor];
            m.hide_clients(m.tag)?;

            m.set_tag(new_tag).unwrap();
            m.bar.update_tags(new_tag);
        }

        let title = if let Some(ci) = self.monitors[self.current_monitor].client {
            self.get_window_title(self.monitors[self.current_monitor].clients[ci].window)?
        } else {
            String::new()
        };
        let selmon = &mut self.monitors[self.current_monitor];
        selmon.bar.update_title(title);

        selmon.recompute_layout()?;
        selmon.focus_current_client(warp_pointer)?;

        Ok(())
    }

    fn spawn_program(&self, cmd: &'static [&'static str]) {
        if let Some((bin, args)) = wwm_core::util::cmd::format(cmd) {
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
        if let Some(ci) = m.client {
            let unfocus_aux =
                ChangeWindowAttributesAux::new().border_pixel(theme::window::BORDER_UNFOCUSED);
            X_HANDLE
                .conn
                .change_window_attributes(m.clients[ci].window, &unfocus_aux)?;
            X_HANDLE
                .conn
                .delete_property(m.clients[ci].window, X_HANDLE.atoms._NET_ACTIVE_WINDOW)?;
        }

        Ok(())
    }

    fn unmanage(&mut self, win: Window, destroyed: bool) -> Result<(), ReplyOrIdError> {
        if let Some((monitor_index, client_index)) = self.win_to_client(win) {
            let monitor = &mut self.monitors[monitor_index];
            let mut client = monitor.remove_client(client_index)?;

            if !destroyed {
                client.set_withdrawn()?;
            }

            if monitor_index == self.current_monitor {
                monitor.focus_current_client(true)?;
            }

            monitor.recompute_layout()?;

            if monitor.client.is_some() {
                monitor.warp_pointer()?;
            }

            self.update_client_list()?;

            X_HANDLE.conn.sync()?;
        }
        Ok(())
    }

    fn update_client_list(&self) -> Result<(), ReplyOrIdError> {
        let screen = X_HANDLE.screen();
        X_HANDLE
            .conn
            .delete_property(screen.root, X_HANDLE.atoms._NET_CLIENT_LIST)?;
        self.for_all_clients(|c| {
            X_HANDLE
                .conn
                .change_property(
                    PropMode::APPEND,
                    screen.root,
                    X_HANDLE.atoms._NET_CLIENT_LIST,
                    X_HANDLE.atoms.WINDOW,
                    32,
                    1,
                    &c.window.to_ne_bytes(),
                )
                .unwrap();
            true
        });
        Ok(())
    }

    fn update_layout(&mut self, layout: WLayout) -> Result<(), ReplyOrIdError> {
        let m = &mut self.monitors[self.current_monitor];
        if m.set_layout(layout) {
            m.bar.update_layout_symbol(m.layout);
            m.recompute_layout()?;
        }
        Ok(())
    }

    fn warp_pointer_to_focused_monitor(&self) -> Result<(), ReplyOrIdError> {
        let m = &self.monitors[self.current_monitor];
        X_HANDLE.conn.warp_pointer(
            NONE,
            X_HANDLE.screen().root,
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
        if let Ok(reply) = X_HANDLE
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

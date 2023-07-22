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
    util::{self, ClientCell, Pos, Rect, Size, WDirection, WVec},
    workspace::WWorkspace,
    AtomCollection,
};
use std::{
    cell::{RefCell, RefMut},
    collections::HashSet,
    process::{exit, Command},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    thread,
    time::Duration,
};
use wwm_bar::{
    font::{loader::LoadedFont, FontDrawer},
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
            MotionNotifyEvent, PropMode, PropertyNotifyEvent, Screen, SetMode, StackMode,
            UnmapNotifyEvent, Window,
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
    Iconic,
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
    monitors: WVec<WMonitor<'a, C>>,
    focused_monitor: Rc<RefCell<WMonitor<'a, C>>>,
    focused_workspace: Rc<RefCell<WWorkspace>>,
    focused_client: Option<Rc<RefCell<WClientState>>>,
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
    ) -> Self {
        // TODO: error handling
        let screen = &conn.setup().roots[screen_num];

        conn.flush().unwrap();

        Self::become_wm(conn, screen, mouse.cursors.normal).unwrap();
        Self::run_auto_start_commands().unwrap();

        let vis_info = Rc::new(RenderVisualInfo::new(conn, screen).unwrap());
        let font = LoadedFont::new(
            conn,
            vis_info.render.pict_format,
            theme::bar::FONT,
            theme::bar::FONT_SIZE,
        )
        .unwrap();
        let font_drawer = Rc::new(FontDrawer::new(font));

        let mut monitors: WVec<WMonitor<'a, C>> =
            Self::get_monitors(conn, screen, &font_drawer, &vis_info)
                .unwrap()
                .into();

        monitors.find_and_select(|m| m.borrow().primary);
        let focused_monitor = monitors.selected().unwrap();
        focused_monitor.borrow_mut().bar.set_is_focused(true);

        let focused_workspace = {
            let mon = focused_monitor.borrow();
            mon.workspaces.selected().unwrap()
        };

        let mut wwm = Self {
            conn,
            screen,
            font_drawer,
            monitors,
            focused_monitor,
            focused_workspace,
            focused_client: None, // we havent scanned windows yet so it's always None here
            pending_exposure: Default::default(),
            drag_window: None,
            resize_window: None,
            keyboard,
            mouse,
            atoms,
            ignore_enter: false,
            should_exit: Arc::new(AtomicBool::new(false)),
        };
        wwm.warp_pointer_to_focused_monitor().unwrap();

        // take care of potentially unmanaged windows
        wwm.scan_windows().unwrap();
        wwm
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        'eventloop: loop {
            // self.refresh();
            self.conn.flush()?;

            while let Ok(event) = self.conn.wait_for_event() {
                if self.handle_event(event)? == ShouldExit::Yes {
                    break 'eventloop;
                }

                for m in self.monitors.inner().iter() {
                    m.borrow_mut().bar.draw(self.conn);
                }
                self.conn.flush()?;
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

        res
    }

    fn destroy_window(&mut self) -> Result<(), ReplyOrIdError> {
        let window = {
            if let Some(client) = &self.focused_client {
                client.borrow().window
            } else {
                return Ok(());
            }
        };

        let delete_exists = self.window_property_exists(
            window,
            self.atoms.WM_DELETE_WINDOW,
            self.atoms.WM_PROTOCOLS,
            self.atoms.ATOM,
        )?;

        self.detach(window);

        if delete_exists {
            self.send_event(window, self.atoms.WM_DELETE_WINDOW)?;
        } else {
            self.conn.grab_server()?;
            self.conn.set_close_down_mode(CloseDown::DESTROY_ALL)?;
            self.conn.kill_client(window)?;
            self.conn.sync()?;
            self.conn.ungrab_server()?;
        }

        self.ignore_enter = true;
        Ok(())
    }

    // FIXME: take monitor and workspace affected
    fn detach(&mut self, window: Window) {
        let conn = self.conn;

        let mut ws_idx = None;
        let mut ws = self.focused_workspace.borrow_mut();
        ws.clients.retain(|client| {
            let c = client.borrow();
            if c.window != window {
                return true;
            }

            conn.grab_server().unwrap();
            conn.change_save_set(SetMode::DELETE, c.window).unwrap();
            conn.ungrab_server().unwrap();

            ws_idx = Some(c.workspace);

            false
        });

        if ws.clients.is_empty() {
            if let Some(i) = ws_idx {
                self.focused_monitor
                    .borrow_mut()
                    .bar
                    .set_has_clients(i, false);
            }
        }

        self.focused_client = None;
    }

    fn entered_adjacent_monitor(&mut self, win: Window) {
        self.monitors.find_and_select(|m| {
            let mut m = m.borrow_mut();
            let idx = m.workspaces.position(|ws| ws.borrow().has_client(win));
            if idx.is_none() {
                return false;
            }

            m.workspaces.select(idx.unwrap()).unwrap();

            let ws = m.workspaces.selected().unwrap();
            ws.borrow_mut().clients.find_and_select(|c| {
                let c = c.borrow();
                c.window == win
            });
            true
        });
        self.focused_monitor = self.monitors.selected().unwrap();
        self.focused_workspace = self.focused_monitor.borrow().focused_workspace();
    }

    fn get_window_title(&self, window: Window) -> Result<String, ReplyOrIdError> {
        let reply = self
            .conn
            .get_property(
                false,
                window,
                self.atoms._NET_WM_NAME,
                self.atoms.UTF8_STRING,
                0,
                8,
            )?
            .reply()?;
        if let Some(text) = reply.value8() {
            let text: Vec<u8> = text.collect();
            return Ok(String::from_utf8(text).unwrap());
        }
        Ok(String::new())
    }

    fn focus(&mut self) -> Result<(), ReplyOrIdError> {
        let ws = self.focused_workspace.borrow();
        self.focused_client = ws.focused_client();

        let win = {
            if let Some(client) = &self.focused_client {
                let c = client.borrow();
                let name = self.get_window_title(c.window)?;
                self.focused_monitor
                    .borrow_mut()
                    .bar
                    .update_title(self.conn, name);
                c.window
            } else {
                return Ok(());
            }
        };

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
        self.unfocus()?;
        {
            self.focused_workspace.borrow_mut().focus_neighbor(dir);
        }
        self.focus()?;
        self.warp_pointer_to_focused_client()?;
        Ok(())
    }

    fn focus_adjacent_monitor(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        self.unfocus()?;

        match dir {
            WDirection::Prev => self.monitors.prev_index(true, true),
            WDirection::Next => self.monitors.next_index(true, true),
        };

        let mon = self.monitors.selected().unwrap();

        self.focused_workspace = mon.borrow().focused_workspace();
        self.focused_client = self.focused_workspace.borrow().focused_client();

        // swap bar focus
        self.focused_monitor.borrow_mut().bar.set_is_focused(false);
        mon.borrow_mut().bar.set_is_focused(true);

        self.focused_monitor = mon;

        self.warp_pointer_to_focused_monitor().unwrap();

        self.focus()?;

        self.warp_pointer_to_focused_client().unwrap();
        Ok(())
    }

    fn focus_at_pointer(&mut self, evt: &MotionNotifyEvent) -> Result<(), ReplyOrIdError> {
        self.monitors
            .find_and_select(|m| m.borrow().has_pos(Pos::from(evt)));
        self.unfocus()?;
        self.focused_monitor = self.monitors.selected().unwrap();
        self.focused_workspace = self.focused_monitor.borrow().focused_workspace();
        self.focus()?;
        Ok(())
    }

    fn for_all_clients<F: Fn(&ClientCell) -> bool>(&self, cb: F) -> bool {
        let mut success = false;
        for mon in self.monitors.inner().iter() {
            for ws in mon.borrow().workspaces.inner().iter() {
                for c in ws.borrow().clients.inner().iter() {
                    if cb(c) {
                        success = true;
                    }
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
        println!("got button press: {evt:#?}");
        if self
            .focused_monitor
            .borrow()
            .bar
            .has_pointer(evt.root_x, evt.root_y)
        {
            let mut mon = self.focused_monitor.borrow_mut();
            if let Some(idx) = mon.bar.select_tag_at_pos(evt.event_x, evt.event_y) {
                drop(mon);
                self.select_workspace(idx, false)?;
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

        println!("got mouse action: {action:?}");

        self.manipulate_client_dims(evt, action)?;

        Ok(())
    }

    fn manipulate_client_dims(
        &mut self,
        evt: ButtonPressEvent,
        action: WMouseCommand,
    ) -> Result<(), ReplyOrIdError> {
        if let Some(c) = &self.focused_client {
            let mut c = c.borrow_mut();
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
            drop(c);
            self.recompute_layout(&self.focused_monitor).unwrap();
        }
        Ok(())
    }

    fn apply_size_hints(
        &self,
        c: &RefMut<'_, WClientState>,
        mut x: i16,
        mut y: i16,
        mut w: u16,
        mut h: u16,
        interact: bool,
    ) -> bool {
        let mon = self.focused_monitor.borrow();

        w = w.min(1);
        h = h.min(1);

        let (sw, sh) = (
            self.screen.width_in_pixels as i16,
            self.screen.height_in_pixels as i16,
        );

        if interact {
            if x > sw {
                x = sw - c.width() as i16;
            }
            if y > sh {
                y = sh - c.height() as i16;
            }
            if (x + w as i16 + 2 * c.bw as i16) < 0 {
                x = 0;
            }
            if (y + h as i16 + 2 * c.bw as i16) < 0 {
                y = 0;
            }
        } else {
            if x >= mon.rect.x + mon.rect.w as i16 {
                x = mon.rect.x + mon.rect.w as i16 - c.width() as i16;
            }
            if y >= mon.rect.y + mon.rect.h as i16 {
                y = mon.rect.y + mon.rect.h as i16 - c.height() as i16;
            }
            if (x + w as i16 + 2 * c.bw as i16) <= mon.rect.x {
                x = mon.rect.x;
            }
            if (y + h as i16 + 2 * c.bw as i16) <= mon.rect.y {
                y = mon.rect.y;
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
                // FIXME: error handling
                self.update_size_hints().unwrap();
            }

            // ICCCM 4.1.2.3
            let base_is_min = c.base_size.w == c.min_size.w && c.base_size.h == c.min_size.h;
            if base_is_min {
                w -= c.base_size.w;
                h -= c.base_size.h;
            }

            if c.mina > 0f32 && c.maxa > 0f32 {
                if c.maxa < w as f32 / h as f32 {
                    w = (h as f32 * c.maxa + 0.5) as u16;
                }
                if c.mina < h as f32 / w as f32 {
                    h = (w as f32 * c.mina + 0.5) as u16;
                }
            }
            if base_is_min {
                w -= c.base_size.w;
                h -= c.base_size.h;
            }

            if c.inc_size.w > 0 {
                w -= w % c.inc_size.w;
            }
            if c.inc_size.h > 0 {
                h -= h % c.inc_size.h;
            }

            w = c.min_size.w.max(w + c.base_size.w);
            h = c.min_size.h.max(h + c.base_size.h);

            if c.max_size.w > 0 {
                w = w.min(c.max_size.w)
            }
            if c.max_size.h > 0 {
                h = h.min(c.max_size.h);
            }
        }

        x != c.rect.x || y != c.rect.y || w != c.rect.w || h != c.rect.h
    }

    fn update_size_hints(&self) -> Result<(), ReplyOrIdError> {
        if let Some(c) = &self.focused_client {
            let mut c = c.borrow_mut();
            let wm_size_hints = WmSizeHints::get_normal_hints(self.conn, c.window)?.reply()?;

            if let Some(bs) = wm_size_hints.base_size {
                c.base_size = bs.into();
            } else if let Some(ms) = wm_size_hints.min_size {
                c.base_size = ms.into();
            } else {
                c.base_size = Size::default();
            }

            if let Some(is) = wm_size_hints.size_increment {
                c.inc_size = is.into();
            } else {
                c.inc_size = Size::default();
            }

            if let Some(ms) = wm_size_hints.max_size {
                c.max_size = ms.into();
            } else {
                c.max_size = Size::default();
            }

            if let Some(ms) = wm_size_hints.min_size {
                c.min_size = ms.into();
            } else {
                c.min_size = Size::default();
            }

            if let Some((min_a, max_a)) = wm_size_hints.aspect {
                c.mina = min_a.numerator as f32 / min_a.denominator as f32;
                c.maxa = max_a.numerator as f32 / max_a.denominator as f32;
            } else {
                c.mina = 0f32;
                c.maxa = 0f32;
            }

            c.is_fixed = c.max_size.w > 0
                && c.max_size.h > 0
                && c.max_size.w == c.min_size.w
                && c.max_size.h == c.min_size.h;
            c.hints_valid = true;
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

    fn handle_configure_request(&mut self, evt: ConfigureRequestEvent) -> Result<(), ReplyError> {
        if evt.window == self.screen.root {
            let aux = ConfigureWindowAux::from_configure_request(&evt)
                .sibling(None)
                .stack_mode(None);
            self.conn.configure_window(evt.window, &aux)?;
        }
        Ok(())
    }

    fn handle_destroy(&mut self, e: DestroyNotifyEvent) -> Result<(), ReplyOrIdError> {
        self.unmanage(e.window, true)?;
        Ok(())
    }

    fn handle_enter(&mut self, evt: EnterNotifyEvent) -> Result<(), ReplyOrIdError> {
        if self.resize_window.is_some() || self.drag_window.is_some() {
            return Ok(());
        }

        if self.ignore_enter {
            self.ignore_enter = false;
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

        let was_managed = self.for_all_clients(|c| {
            let c = c.borrow();
            c.window == entered_win
        });

        if !was_managed {
            return Ok(());
        }

        if let Some(client) = &self.focused_client {
            let c = client.borrow();
            if c.window == entered_win {
                return Ok(());
            }
        }

        self.unfocus()?;

        {
            let in_workspace = self.focused_workspace.borrow().has_client(entered_win);
            if !in_workspace {
                self.entered_adjacent_monitor(entered_win);
            }

            let mut ws = self.focused_workspace.borrow_mut();
            ws.clients.find_and_select(|c| {
                let c = c.borrow();
                c.window == entered_win
            });
        }

        self.focus()?;
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

    fn handle_client_message(&self, evt: ClientMessageEvent) -> Result<(), ReplyOrIdError> {
        if evt.type_ == self.atoms._NET_WM_STATE {
            let data = evt.data.as_data32();
            if data[1] == self.atoms._NET_WM_STATE_FULLSCREEN
                || data[2] == self.atoms._NET_WM_STATE_FULLSCREEN
            {
                if let Some(c) = self.win_to_client(evt.window) {
                    let c = c.borrow_mut();
                    let m = self.monitors.get(c.monitor).unwrap();
                    let fullscreen = data[0] == self.atoms._NET_WM_STATE_ADD
                        || (data[0] == self.atoms._NET_WM_STATE_TOGGLE && !c.is_fullscreen);
                    self.fullscreen(c, &m, fullscreen)?;
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
        println!("got action: {action:?}");

        match action {
            WKeyCommand::FocusClient(dir) => self.focus_adjacent(dir)?,
            WKeyCommand::MoveClient(dir) => self.move_adjacent(dir)?,
            WKeyCommand::FocusMonitor(dir) => self.focus_adjacent_monitor(dir)?,
            WKeyCommand::Spawn(cmd) => self.spawn_program(cmd),
            WKeyCommand::Destroy => self.destroy_window()?,
            WKeyCommand::AdjustMainWidth(dir) => self.adjust_main_width(dir)?,
            WKeyCommand::Layout(layout) => self.update_workspace_layout(layout),
            WKeyCommand::SelectWorkspace(idx) => self.select_workspace(idx, true).unwrap(),
            WKeyCommand::MoveClientToWorkspace(ws_idx) => self.move_client_to_workspace(ws_idx)?,
            WKeyCommand::MoveClientToMonitor(dir) => self.move_client_to_monitor(dir)?,
            WKeyCommand::UnFloat => self.unfloat_focused_client()?,
            WKeyCommand::Fullscreen => self.fullscreen_focused_client()?,
            WKeyCommand::Exit => self.try_exit(),
            _ => {}
        }
        Ok(())
    }

    fn fullscreen_focused_client(&mut self) -> Result<(), ReplyOrIdError> {
        if let Some(c) = &self.focused_client {
            let c = c.borrow_mut();
            let m = self.monitors.get(c.monitor).unwrap();
            let fullscreen = !c.is_fullscreen;
            self.fullscreen(c, &m, fullscreen)?;
        }
        Ok(())
    }

    fn fullscreen(
        &self,
        mut c: RefMut<'_, WClientState>,
        m: &Rc<RefCell<WMonitor<C>>>,
        fullscreen: bool,
    ) -> Result<(), ReplyOrIdError> {
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
            let rect = m.borrow().rect;
            let bh = util::bar_height();
            self.resize_client(c, rect.x, rect.y - bh as i16, rect.w, rect.h + bh)?;
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
            self.resize_client(c, r.x, r.y, r.w, r.h)?;
            self.recompute_layout(&m)?;
        }
        Ok(())
    }

    fn unfloat_focused_client(&mut self) -> Result<(), ReplyOrIdError> {
        if let Some(c) = &self.focused_client {
            let mut c = c.borrow_mut();
            if !c.is_floating {
                return Ok(());
            }

            c.is_floating = false;
            let pos = Pos::new(c.rect.x + (c.rect.w as i16 / 2), c.rect.y);
            drop(c);

            let mon = self.focused_monitor.borrow();
            if let Some(dir) = mon.find_adjacent_monitor(pos) {
                drop(mon);
                self.move_client_to_monitor(dir).unwrap();
            }
            self.recompute_layout(&self.focused_monitor)?;
            self.warp_pointer_to_focused_client()?;
        }
        Ok(())
    }

    fn adjust_main_width(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        {
            let mut ws = self.focused_workspace.borrow_mut();
            match dir {
                WDirection::Prev if ws.width_factor - WIDTH_ADJUSTMENT_FACTOR >= 0.05 => {
                    ws.width_factor -= WIDTH_ADJUSTMENT_FACTOR;
                }
                WDirection::Next if ws.width_factor <= 0.95 => {
                    ws.width_factor += WIDTH_ADJUSTMENT_FACTOR;
                }
                _ => {}
            }
        }
        self.recompute_layout(&self.focused_monitor)?;
        Ok(())
    }

    fn move_client_to_monitor(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        if self.focused_client.is_none() {
            return Ok(());
        }
        let idx = match dir {
            WDirection::Prev => self.monitors.prev_index(true, false).unwrap(),
            WDirection::Next => self.monitors.next_index(true, false).unwrap(),
        };

        if idx == self.monitors.index() {
            return Ok(());
        }

        self.unfocus()?;

        if let Some(mut removed) = self.focused_workspace.borrow_mut().remove_focused() {
            let mut m = self.monitors.get_mut(idx).unwrap();
            let ws_idx = m.workspaces.index();
            removed.workspace = ws_idx;
            removed.monitor = idx;

            m.bar.set_has_clients(ws_idx, true);
            m.add_client_to_workspace(ws_idx, removed);
        }

        let rc = self.monitors.get(idx).unwrap();
        self.recompute_layout(&rc)?;
        self.recompute_layout(&self.focused_monitor)?;

        self.focus()?;
        self.warp_pointer_to_focused_client()?;
        Ok(())
    }

    fn move_client_to_workspace(&mut self, ws_idx: usize) -> Result<(), ReplyOrIdError> {
        if self.focused_client.is_none() {
            return Ok(());
        }

        let focused_ws_idx = self.focused_monitor.borrow().workspaces.index();
        if focused_ws_idx == ws_idx {
            return Ok(());
        }

        self.unfocus()?;
        if let Some(mut removed) = self.focused_workspace.borrow_mut().remove_focused() {
            removed.workspace = ws_idx;
            self.focused_monitor
                .borrow_mut()
                .add_client_to_workspace(ws_idx, removed);
        }
        self.recompute_layout(&self.focused_monitor)?;
        self.focus()?;
        Ok(())
    }

    fn handle_map_request(&mut self, evt: MapRequestEvent) -> Result<(), ReplyOrIdError> {
        let wa = self.conn.get_window_attributes(evt.window)?;
        match wa.reply() {
            Ok(wa) if wa.override_redirect => return Ok(()),
            Err(e) => return Err(e)?,
            Ok(_) => {}
        }

        let was_managed = self.for_all_clients(|c| {
            let c = c.borrow();
            c.window == evt.window
        });

        if was_managed {
            return Ok(());
        }

        self.manage_window(evt.window, &self.conn.get_geometry(evt.window)?.reply()?)
    }

    fn handle_motion_notify(&mut self, evt: MotionNotifyEvent) -> Result<(), ReplyOrIdError> {
        let mon = self.focused_monitor.borrow();
        if mon.bar.has_pointer(evt.root_x, evt.root_y) {
            return Ok(());
        }

        let mon_has_pointer = mon.has_pos(Pos::from(&evt));
        drop(mon);

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
        if let Some(c) = &self.focused_client {
            let c = c.borrow_mut();

            if c.is_fullscreen || ev.time - last_resize <= (1000 / 60) {
                return Ok(());
            }

            if c.is_floating {
                let nw = 1.max(ev.root_x - c.rect.x - (2 * BORDER_WIDTH as i16) + 1) as u16;
                let nh = 1.max(ev.root_y - c.rect.y - (2 * BORDER_WIDTH as i16) + 1) as u16;

                // copy before move
                let x = c.rect.x;
                let y = c.rect.y;

                self.resize(c, x, y, nw, nh, true)?;
            }
        }
        Ok(())
    }

    fn resize(
        &self,
        c: RefMut<'_, WClientState>,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        interact: bool,
    ) -> Result<(), ReplyOrIdError> {
        if self.apply_size_hints(&c, x, y, w, h, interact) {
            self.resize_client(c, x, y, w, h)?;
        }

        Ok(())
    }

    fn resize_client(
        &self,
        mut c: RefMut<'_, WClientState>,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
    ) -> Result<(), ReplyOrIdError> {
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

        let mut ce = ConfigureNotifyEvent::default();
        ce.response_type = 22; // ConfigureNotify
        ce.event = c.window;
        ce.window = c.window;
        ce.x = c.rect.x;
        ce.y = c.rect.y;
        ce.width = c.rect.w;
        ce.height = c.rect.h;
        ce.border_width = c.bw;
        ce.override_redirect = false;
        self.conn
            .send_event(false, c.window, EventMask::STRUCTURE_NOTIFY, ce)?;

        self.conn.sync()?;
        Ok(())
    }

    fn mouse_move(
        &mut self,
        (oc_pos, op_pos, last_move): (Pos, Pos, u32),
        ev: MotionNotifyEvent,
    ) -> Result<(), ReplyOrIdError> {
        if let Some(c) = &self.focused_client {
            let mut c = c.borrow_mut();

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
            self.focused_monitor
                .borrow_mut()
                .bar
                .update_title(self.conn, title);
        }
        Ok(())
    }

    fn handle_unmap_notify(&mut self, evt: UnmapNotifyEvent) -> Result<(), ReplyOrIdError> {
        if self.for_all_clients(|c| c.borrow().window == evt.window) {
            self.unmanage(evt.window, false)?;
        }
        Ok(())
    }

    fn manage_window(
        &mut self,
        win: Window,
        geom: &GetGeometryReply,
    ) -> Result<(), ReplyOrIdError> {
        let mut is_floating = self.window_property_exists(
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

        let transient_for = self
            .conn
            .get_property(
                false,
                win,
                self.atoms.WM_TRANSIENT_FOR,
                self.atoms.WINDOW,
                0,
                u32::MAX,
            )?
            .reply()?;

        let mut ws_idx = self.focused_monitor.borrow().workspaces.index();
        let mut mon_idx = self.monitors.index();

        if let Some(trans) = transient_for.value32() {
            if let Some(t) = trans.collect::<Vec<u32>>().get(0) {
                if !is_floating {
                    is_floating = true;
                }

                if let Some(c) = self.win_to_client(*t) {
                    let c = c.borrow();
                    ws_idx = c.workspace;
                    mon_idx = c.monitor;
                }
            }
        }

        let mut conf_aux =
            ConfigureWindowAux::new().border_width(theme::window::BORDER_WIDTH as u32);

        let (mx, my, mw, mh) = {
            let m = self.focused_monitor.borrow();
            (m.rect.x, m.rect.y, m.rect.w, m.rect.h)
        };

        let mut x = geom.x;
        let mut y = geom.y;

        if geom.x + geom.width as i16 > mx + mw as i16 {
            x = mx + mw as i16 - geom.width as i16 - (theme::window::BORDER_WIDTH as i16 * 2)
        }
        if geom.y + geom.height as i16 > my + mh as i16 {
            y = my + mh as i16 + geom.height as i16 - (theme::window::BORDER_WIDTH as i16 * 2)
        }

        x = x.max(mx);
        y = y.max(my);

        conf_aux = conf_aux
            .stack_mode(StackMode::ABOVE)
            .x(x as i32)
            .y(y as i32);

        let change_aux = ChangeWindowAttributesAux::new()
            .border_pixel(theme::window::BORDER_UNFOCUSED)
            .event_mask(
                EventMask::ENTER_WINDOW
                    | EventMask::FOCUS_CHANGE
                    | EventMask::PROPERTY_CHANGE
                    | EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::STRUCTURE_NOTIFY,
            );

        self.conn.configure_window(win, &conf_aux)?;
        self.conn.change_window_attributes(win, &change_aux)?;

        // this should never fail
        let m = self.monitors.get(mon_idx).unwrap();
        let mb = m.borrow();
        let ws = mb.workspaces.get(ws_idx).unwrap();

        let rect = if is_fullscreen {
            mb.rect
        } else {
            Rect::new(x, y, geom.width, geom.height)
        };

        drop(mb);

        ws.borrow_mut().push_client(WClientState::new(
            win,
            rect,
            rect, // we use the same rect here for now
            is_floating,
            is_fullscreen,
            ws_idx,
            mon_idx,
        ));

        self.set_client_state(win, WindowState::Normal)?;

        self.recompute_layout(&m)?;
        self.conn.map_window(win)?;
        self.update_client_list()?;

        self.unfocus()?;
        self.focus()?;

        if is_fullscreen {
            self.fullscreen_focused_client()?;
        }

        {
            let mut m = self.focused_monitor.borrow_mut();
            let ws_idx = m.workspaces.index();
            m.bar.set_has_clients(ws_idx, true);
        }

        self.update_size_hints()?;
        self.warp_pointer_to_focused_client()?;

        Ok(())
    }

    fn win_to_client(&self, win: Window) -> Option<Rc<RefCell<WClientState>>> {
        for m in self.monitors.inner().iter() {
            for ws in m.borrow().workspaces.inner().iter() {
                let c = ws.borrow().find_client_by_win(win);
                if c.is_some() {
                    return c;
                }
            }
        }
        None
    }

    fn move_adjacent(&mut self, dir: WDirection) -> Result<(), ReplyOrIdError> {
        {
            let mut ws = self.focused_workspace.borrow_mut();
            ws.swap_with_neighbor(dir);
        }

        let focused_client = self.focused_workspace.borrow().focused_client();
        if focused_client.is_some() {
            self.ignore_enter = true;
            self.recompute_layout(&self.focused_monitor)?;
        }
        self.focused_client = focused_client;
        self.warp_pointer_to_focused_client()?;
        Ok(())
    }

    fn recompute_layout(&self, mon: &Rc<RefCell<WMonitor<C>>>) -> Result<(), ReplyOrIdError> {
        let mon = mon.borrow();
        let ws = mon.focused_workspace();
        let ws = ws.borrow();
        let non_floating_clients = ws
            .clients
            .inner()
            .iter()
            .filter(|c| !c.borrow().is_floating)
            .collect();

        let rects = layout_clients(&ws.layout, ws.width_factor, &mon, &non_floating_clients);

        if rects.is_none() {
            return Ok(());
        }

        for (client, rect) in non_floating_clients.iter().zip(rects.unwrap()) {
            self.resize(client.borrow_mut(), rect.x, rect.y, rect.w, rect.h, false)?;
        }
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

    fn select_workspace(&mut self, idx: usize, warp: bool) -> Result<(), ReplyOrIdError> {
        {
            let m = self.focused_monitor.borrow();
            // early return since we dont want to do anything here
            if m.is_focused_workspace(idx) {
                return Ok(());
            }
        }

        self.focused_workspace.borrow().hide_clients(self.conn)?;

        {
            let mut m = self.focused_monitor.borrow_mut();
            m.focus_workspace_from_index(idx).unwrap();
            self.focused_workspace = m.focused_workspace();
            let ws = self.focused_workspace.borrow();
            self.focused_client = ws.focused_client();
            m.bar.update_tags(idx);
            m.bar.update_layout_symbol(self.conn, ws.layout.to_string());
            let title = if let Some(c) = &self.focused_client {
                self.get_window_title(c.borrow().window)?
            } else {
                String::new()
            };
            m.bar.update_title(self.conn, title);
        }

        self.recompute_layout(&self.focused_monitor)?;

        self.focus()?;
        if warp {
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

    fn unfocus(&mut self) -> Result<(), ReplyError> {
        if let Some(client) = &self.focused_client {
            let c = client.borrow();
            let unfocus_aux =
                ChangeWindowAttributesAux::new().border_pixel(theme::window::BORDER_UNFOCUSED);
            self.conn.change_window_attributes(c.window, &unfocus_aux)?;
            self.conn
                .delete_property(c.window, self.atoms._NET_ACTIVE_WINDOW)?;

            self.focused_monitor
                .borrow_mut()
                .bar
                .update_title(self.conn, "");
        }

        self.focused_client = None;
        Ok(())
    }

    fn unmanage(&mut self, win: Window, destroyed: bool) -> Result<(), ReplyOrIdError> {
        self.detach(win);
        if !destroyed {
            self.conn.grab_server()?;
            self.set_client_state(win, WindowState::Withdrawn)?;
            self.conn.sync()?;
            self.conn.ungrab_server()?;
        }

        // FIXME: this has to be reworked if a client gets unmanaged on a
        //        non-focused workspace or monitor
        self.focus()?;
        self.update_client_list()?;
        self.recompute_layout(&self.focused_monitor)?;
        self.warp_pointer_to_focused_client()?;
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
                    &c.borrow().window.to_ne_bytes(),
                )
                .unwrap();
            true
        });
        Ok(())
    }

    fn update_workspace_layout(&mut self, layout: WLayout) {
        if self.focused_workspace.borrow_mut().set_layout(layout) {
            self.focused_monitor.borrow_mut().bar.update_layout_symbol(
                self.conn,
                self.focused_workspace.borrow().layout.to_string(),
            );
            self.recompute_layout(&self.focused_monitor).unwrap();
        }
    }

    fn warp_pointer_to_focused_client(&self) -> Result<(), ReplyOrIdError> {
        if let Some(client) = &self.focused_client {
            let c = client.borrow();
            let pointer = self.conn.query_pointer(c.window)?.reply()?;
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
        Ok(())
    }

    fn warp_pointer_to_focused_monitor(&self) -> Result<(), ReplyOrIdError> {
        let m = self.focused_monitor.borrow();
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
        let reply = self
            .conn
            .get_property(
                false,
                window,
                prop,
                type_,
                0,
                std::mem::size_of::<u32>() as u32,
            )?
            .reply()?;
        let mut found = false;
        if let Some(mut value) = reply.value32() {
            found = value.any(|a| a == atom);
        } else if let Some(mut value) = reply.value16() {
            found = value.any(|a| a == atom as u16);
        } else if let Some(mut value) = reply.value8() {
            found = value.any(|a| a == atom as u8);
        }
        Ok(found)
    }
}

use crate::{
    client::{ClientRect, WClientState},
    config::{
        auto_start::AUTO_START_COMMANDS, mouse::DRAG_BUTTON, theme,
        workspaces::WIDTH_ADJUSTMENT_FACTOR,
    },
    keyboard::{keybind::WCommand, WKeyboard},
    layouts::{layout_clients, WLayout},
    monitor::WMonitor,
    util::{self, ClientCell, WVec},
    workspace::{StackDirection, WWorkspace},
    AtomCollection,
};
use std::{
    cell::RefCell,
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
    cursor::Handle as CursorHandle,
    protocol::{
        randr::ConnectionExt as _,
        xkb::StateNotifyEvent,
        xproto::{
            ButtonPressEvent, ButtonReleaseEvent, ChangeWindowAttributesAux, ClientMessageEvent,
            ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt, DestroyNotifyEvent,
            EnterNotifyEvent, EventMask, ExposeEvent, GetGeometryReply, InputFocus, KeyPressEvent,
            MapRequestEvent, MapState, MotionNotifyEvent, PropMode, PropertyNotifyEvent, Screen,
            SetMode, StackMode, UnmapNotifyEvent, Window,
        },
        ErrorKind, Event,
    },
    resource_manager::new_from_default,
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
    NotifyNonlinear,
    NotifyNonlinearVirtual,
    NotifyPointer,
    NotifyPointerRoot,
    NotifyDetailNone,
}

pub struct WinMan<'a, C: Connection> {
    conn: &'a C,
    screen: &'a Screen,
    font_drawer: Rc<FontDrawer>,
    monitors: WVec<WMonitor<'a, C>>,
    focused_monitor: Rc<RefCell<WMonitor<'a, C>>>,
    focused_workspace: Rc<RefCell<WWorkspace>>,
    focused_client: Option<Rc<RefCell<WClientState>>>,
    pending_exposure: HashSet<Window>,
    drag_window: Option<(Window, (i16, i16))>,
    keyboard: WKeyboard,
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
        atoms: AtomCollection,
    ) -> Self {
        // TODO: error handling
        let screen = &conn.setup().roots[screen_num];

        Self::become_wm(conn, screen_num, screen).unwrap();
        Self::run_auto_start_commands().unwrap();

        let vis_info = Rc::new(RenderVisualInfo::new(conn, &screen).unwrap());
        let font = LoadedFont::new(conn, vis_info.render.pict_format).unwrap();
        let font_drawer = Rc::new(FontDrawer::new(font));

        let mut monitors: WVec<WMonitor<'a, C>> =
            Self::get_monitors(conn, screen, &font_drawer, &vis_info)
                .unwrap()
                .into();

        monitors.find_and_select(|m| m.borrow().primary);
        let focused_monitor = monitors.selected().unwrap();

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
            keyboard,
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
            }
        }
        Ok(())
    }

    fn become_wm(conn: &'a C, screen_num: usize, screen: &Screen) -> Result<(), ReplyError> {
        let resource_db = new_from_default(conn)?;
        let cursor_handle = CursorHandle::new(conn, screen_num, &resource_db)?;
        let cursor_handle = cursor_handle.reply().unwrap();
        conn.flush()?;

        let change = ChangeWindowAttributesAux::default()
            .event_mask(
                EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::POINTER_MOTION
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::PROPERTY_CHANGE,
            )
            .cursor(cursor_handle.load_cursor(conn, "left_ptr").unwrap());

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
            self.atoms.ATOM_ATOM,
        )?;

        self.detach(window);

        if delete_exists {
            self.send_event(window, self.atoms.WM_DELETE_WINDOW)?;
        } else {
            self.conn.destroy_window(window)?;
        }

        self.conn.flush()?;

        self.ignore_enter = true;
        Ok(())
    }

    fn detach(&mut self, window: Window) {
        let conn = self.conn;

        let mut ws = self.focused_workspace.borrow_mut();
        ws.clients.retain(|client| {
            let c = client.borrow();
            if c.window != window {
                return true;
            }

            conn.grab_server().unwrap();
            conn.change_save_set(SetMode::DELETE, c.window).unwrap();
            conn.ungrab_server().unwrap();
            false
        });
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
            println!("name bytes: {text:#?}");
            return Ok(String::from_utf8(text).unwrap());
        }
        return Ok(String::new());
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

        self.conn.flush()?;
        Ok(())
    }

    fn focus_adjacent(&mut self, dir: StackDirection) -> Result<(), ReplyOrIdError> {
        self.unfocus()?;
        {
            self.focused_workspace.borrow_mut().focus_neighbor(dir);
        }
        self.focus()?;
        self.warp_pointer_to_focused_client()?;
        Ok(())
    }

    fn focus_adjacent_monitor(&mut self, dir: StackDirection) -> Result<(), ReplyOrIdError> {
        self.unfocus()?;

        match dir {
            StackDirection::Prev => self.monitors.prev_index(true, true),
            StackDirection::Next => self.monitors.next_index(true, true),
        };

        let mon = self.monitors.selected().unwrap();

        self.focused_workspace = mon.borrow().focused_workspace();
        self.focused_client = self.focused_workspace.borrow().focused_client();
        self.focused_monitor = mon;

        self.warp_pointer_to_focused_monitor().unwrap();

        self.focus()?;

        self.warp_pointer_to_focused_client().unwrap();
        Ok(())
    }

    fn focus_at_pointer(&mut self, evt: &MotionNotifyEvent) -> Result<(), ReplyOrIdError> {
        self.monitors
            .find_and_select(|m| m.borrow().has_pointer(evt));
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
        conn.flush()?;
        let monitors: Vec<WMonitor<C>> = monitors
            .monitors
            .iter()
            .map(|m| WMonitor::new(m, conn, Rc::clone(font_drawer), Rc::clone(vis_info)))
            .collect();
        Ok(monitors)
    }

    fn handle_button_press(&mut self, evt: ButtonPressEvent) {
        if evt.detail != DRAG_BUTTON || u16::from(evt.state) != 0 {
            return;
        }
        let ws = self.focused_workspace.borrow();
        if let Some(client) = ws.find_client_by_win(evt.event) {
            let c = client.borrow();
            if self.drag_window.is_none() && evt.event_x < 0.max(c.rect.width as i16) {
                let (x, y) = (-evt.event_x, -evt.event_y);
                self.drag_window = Some((c.window, (x, y)));
            }
        }
    }

    fn handle_button_release(&mut self, evt: ButtonReleaseEvent) -> Result<(), ReplyError> {
        if evt.detail == DRAG_BUTTON {
            self.drag_window = None;
        }

        Ok(())
    }

    fn handle_configure_request(&mut self, evt: ConfigureRequestEvent) -> Result<(), ReplyError> {
        let aux = ConfigureWindowAux::from_configure_request(&evt)
            .sibling(None)
            .stack_mode(None);
        self.conn.configure_window(evt.window, &aux)?;
        self.conn.flush()?;
        Ok(())
    }

    fn handle_destroy(&mut self, e: DestroyNotifyEvent) -> Result<(), ReplyOrIdError> {
        self.unmanage(e.window, true)?;
        Ok(())
    }

    fn handle_enter(&mut self, evt: EnterNotifyEvent) -> Result<(), ReplyOrIdError> {
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
            Event::ButtonPress(e) => self.handle_button_press(e),
            Event::ButtonRelease(e) => self.handle_button_release(e)?,
            Event::MotionNotify(e) => self.handle_motion_notify(e)?,
            Event::KeyPress(e) => self.handle_key_press(e)?,
            Event::XkbStateNotify(e) => self.handle_xkb_state_notify(e)?,
            Event::PropertyNotify(e) => self.handle_property_notify(e)?,
            Event::Error(e) => eprintln!("ERROR: {e:#?}"),
            _ => {}
        }

        Ok(ShouldExit::No)
    }

    fn handle_expose(&mut self, evt: ExposeEvent) {
        self.pending_exposure.insert(evt.window);
    }

    fn handle_key_press(&mut self, evt: KeyPressEvent) -> Result<(), ReplyOrIdError> {
        let sym = self.keyboard.key_sym(evt.detail.into());

        let mut action = WCommand::Idle;
        for bind in &self.keyboard.keybinds {
            if bind.keysym == sym && evt.state == bind.mods_as_key_but_mask() {
                action = bind.action;
                break;
            }
        }

        match action {
            WCommand::FocusClientPrev => self.focus_adjacent(StackDirection::Prev)?,
            WCommand::FocusClientNext => self.focus_adjacent(StackDirection::Next)?,
            WCommand::MoveClientPrev => self.move_adjacent(StackDirection::Prev)?,
            WCommand::MoveClientNext => self.move_adjacent(StackDirection::Next)?,
            WCommand::FocusMonitorNext => self.focus_adjacent_monitor(StackDirection::Next)?,
            WCommand::FocusMonitorPrev => self.focus_adjacent_monitor(StackDirection::Prev)?,
            WCommand::Spawn(cmd) => self.spawn_program(cmd),
            WCommand::Destroy => self.destroy_window()?,
            WCommand::IncreaseMainWidth => {
                {
                    let mut ws = self.focused_workspace.borrow_mut();
                    if ws.width_factor + WIDTH_ADJUSTMENT_FACTOR > 0.95 {
                        return Ok(());
                    }
                    ws.width_factor += WIDTH_ADJUSTMENT_FACTOR;
                }
                self.recompute_layout()?;
            }
            WCommand::DecreaseMainWidth => {
                {
                    let mut ws = self.focused_workspace.borrow_mut();
                    if ws.width_factor - WIDTH_ADJUSTMENT_FACTOR < 0.05 {
                        return Ok(());
                    }
                    ws.width_factor -= WIDTH_ADJUSTMENT_FACTOR;
                }
                self.recompute_layout()?;
            }
            WCommand::Layout(layout) => self.update_workspace_layout(layout),
            WCommand::SelectWorkspace(idx) => self.select_workspace(idx).unwrap(),
            WCommand::Exit => self.try_exit(),
            WCommand::Idle => {}
        }
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
        if !self.focused_monitor.borrow().has_pointer(&evt) {
            self.focus_at_pointer(&evt)?;
        }

        if let Some((win, (x, y))) = self.drag_window {
            let (x, y) = (x + evt.root_x, y + evt.root_y);
            let (x, y) = (x as i32, y as i32);
            self.conn
                .configure_window(win, &ConfigureWindowAux::new().x(x).y(y))?;
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

    fn handle_xkb_state_notify(&mut self, evt: StateNotifyEvent) -> Result<(), ReplyOrIdError> {
        if i32::try_from(evt.device_id).unwrap() == self.keyboard.device_id {
            self.keyboard.update_state_mask(evt);
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

        // WM_TRANSIENT_FOR Property

        // The WM_TRANSIENT_FOR property (of type WINDOW) contains the ID of another top-level window.
        // The implication is that this window is a pop-up on behalf of the named window, and window
        // managers may decide not to decorate transient windows or may treat them differently in other ways.
        // In particular, window managers should present newly mapped WM_TRANSIENT_FOR windows without
        // requiring any user interaction, even if mapping top-level windows normally does require
        // interaction. Dialogue boxes, for example, are an example of windows that should have
        // WM_TRANSIENT_FOR set.

        // It is important not to confuse WM_TRANSIENT_FOR with override-redirect. WM_TRANSIENT_FOR
        // should be used in those cases where the pointer is not grabbed while the window is mapped
        // (in other words, if other windows are allowed to be active while the transient is up).
        // If other windows must be prevented from processing input (for example, when
        // implementing pop-up menus), use override-redirect and grab the pointer while the window is mapped.

        // let trans = self
        //     .conn
        //     .get_property(
        //         false,
        //         win,
        //         self.atoms.WM_TRANSIENT_FOR,
        //         self.atoms.ATOM,
        //         0,
        //         std::mem::size_of::<u32>() as u32,
        //     )?
        //     .reply()?;
        // println!("trans: {trans:#?}");
        // if let Some(val) = trans.value32() {
        //     println!("trans: {:?}", val.into_iter().collect::<Vec<u32>>());
        // }

        let mut conf_aux =
            ConfigureWindowAux::new().border_width(theme::window::BORDER_WIDTH as u32);

        let (mx, my, mw, mh) = {
            let m = self.focused_monitor.borrow();
            (m.x, m.y, m.width, m.height)
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
                    | EventMask::STRUCTURE_NOTIFY,
            );

        self.conn.configure_window(win, &conf_aux)?;
        self.conn.change_window_attributes(win, &change_aux)?;

        self.focused_workspace
            .borrow_mut()
            .push_client(WClientState::new(
                win,
                ClientRect::new(x, y, geom.width, geom.height),
                is_floating,
            ));
        self.set_client_state(win, WindowState::Normal)?;

        self.recompute_layout()?;
        self.conn.map_window(win)?;
        self.update_client_list()?;

        self.unfocus()?;
        self.focus()?;
        self.warp_pointer_to_focused_client()?;

        Ok(())
    }

    fn move_adjacent(&mut self, dir: StackDirection) -> Result<(), ReplyOrIdError> {
        {
            let mut ws = self.focused_workspace.borrow_mut();
            ws.swap_with_neighbor(dir);
        }

        let focused_client = self.focused_workspace.borrow().focused_client();
        if focused_client.is_some() {
            self.ignore_enter = true;
            self.recompute_layout()?;
        }
        self.focused_client = focused_client;
        self.warp_pointer_to_focused_client()?;
        Ok(())
    }

    fn recompute_layout(&mut self) -> Result<(), ReplyOrIdError> {
        let ws = self.focused_workspace.borrow();
        let non_floating_clients = ws
            .clients
            .inner()
            .iter()
            .filter(|c| !c.borrow().is_floating)
            .collect();

        let rects = layout_clients(
            &ws.layout,
            ws.width_factor,
            &self.focused_monitor.borrow(),
            &non_floating_clients,
        );

        if rects.is_none() {
            return Ok(());
        }

        for (client, rect) in non_floating_clients.iter().zip(rects.unwrap()) {
            let client_aux = ConfigureWindowAux::from(rect)
                .sibling(None)
                .stack_mode(None);

            let mut c = client.borrow_mut();
            c.rect = rect;

            self.conn.configure_window(c.window, &client_aux)?;
        }
        self.conn.flush()?;
        Ok(())
    }

    #[allow(dead_code)]
    fn refresh(&mut self) {
        // TODO: when the bar is implemented, we want to update the bar information here
        todo!()
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
        self.conn.flush()?;

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

    fn select_workspace(&mut self, idx: usize) -> Result<(), ReplyOrIdError> {
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

        self.recompute_layout()?;

        self.warp_pointer_to_focused_client()?;

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
        self.conn.flush()?;
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
            self.conn.flush()?;

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
        self.focus()?;
        self.update_client_list()?;
        self.recompute_layout()?;
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
        self.conn.flush()?;
        Ok(())
    }

    fn update_workspace_layout(&mut self, layout: WLayout) {
        if self.focused_workspace.borrow_mut().set_layout(layout) {
            self.focused_monitor.borrow_mut().bar.update_layout_symbol(
                self.conn,
                self.focused_workspace.borrow().layout.to_string(),
            );
            self.recompute_layout().unwrap();
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
                c.rect.width as i16 / 2,
                c.rect.height as i16 / 2,
            )?;
            self.conn.flush()?;
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
            m.x + (m.width as i16 / 2),
            m.y + (m.height as i16 / 2),
        )?;
        self.conn.flush()?;
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

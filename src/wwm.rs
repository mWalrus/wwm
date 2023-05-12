use crate::{
    client::WClientState,
    config::{mouse::DRAG_BUTTON, theme},
    keyboard::{keybind::WCommand, WKeyboard},
    layouts::{layout_clients, WLayout},
    monitor::{StackDirection, WMonitor, WWorkspace},
    util::WVec,
    AtomCollection,
};
use std::{
    cell::RefCell,
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
    process::{exit, Command},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    thread,
    time::Duration,
};
use x11rb::{
    connection::Connection,
    cursor::Handle as CursorHandle,
    protocol::{
        randr::ConnectionExt as _,
        xkb::StateNotifyEvent,
        xproto::{
            ButtonPressEvent, ButtonReleaseEvent, ChangeWindowAttributesAux, ClientMessageEvent,
            ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt, CreateWindowAux,
            EnterNotifyEvent, EventMask, ExposeEvent, GetGeometryReply, InputFocus, KeyPressEvent,
            MapRequestEvent, MapState, MotionNotifyEvent, Screen, SetMode, StackMode,
            UnmapNotifyEvent, Window, WindowClass,
        },
        ErrorKind, Event,
    },
    resource_manager::new_from_default,
    rust_connection::{ReplyError, ReplyOrIdError},
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE,
};

pub struct WinMan<'a, C: Connection> {
    conn: &'a C,
    screen: &'a Screen,
    monitors: WVec<WMonitor>,
    focused_monitor: Rc<RefCell<WMonitor>>,
    focused_workspace: Rc<RefCell<WWorkspace>>,
    focused_client: Option<Rc<RefCell<WClientState>>>,
    ignore_sequences: BinaryHeap<Reverse<u16>>,
    pending_exposure: HashSet<Window>,
    drag_window: Option<(Window, (i16, i16))>,
    layout: WLayout,
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
        let monitors = Self::get_monitors(conn, screen).unwrap();

        let mut monitors: WVec<WMonitor> = monitors.into();
        monitors.find_and_select(|m| m.borrow().primary);
        let focused_monitor = monitors.selected().unwrap();

        let focused_workspace = {
            let mon = focused_monitor.borrow();
            let ws = mon.workspaces.selected().unwrap();
            ws
        };

        let mut wwm = Self {
            conn,
            screen,
            monitors,
            focused_monitor,
            focused_workspace,
            focused_client: None, // we havent scanned windows yet so it's always None here
            ignore_sequences: Default::default(),
            pending_exposure: Default::default(),
            drag_window: None,
            layout: WLayout::Tile,
            keyboard,
            atoms,
            ignore_enter: false,
            should_exit: Arc::new(AtomicBool::new(false)),
        };

        // take care of potentially unmanaged windows
        wwm.scan_windows().unwrap();
        wwm
    }

    fn get_monitors(conn: &'a C, screen: &Screen) -> Result<Vec<WMonitor>, ReplyError> {
        let monitors = conn.randr_get_monitors(screen.root, true)?.reply()?;
        let monitors: Vec<WMonitor> = monitors.monitors.iter().map(|m| m.into()).collect();
        Ok(monitors)
    }

    fn become_wm(conn: &'a C, screen_num: usize, screen: &Screen) -> Result<(), ReplyError> {
        // set up substructure redirects for the root window.
        // NOTE: this will fail if another window manager is already running
        let resource_db = new_from_default(conn)?;
        let cursor_handle = CursorHandle::new(conn, screen_num, &resource_db)?;
        let cursor_handle = cursor_handle.reply().unwrap();

        let change = ChangeWindowAttributesAux::default()
            .event_mask(
                EventMask::SUBSTRUCTURE_REDIRECT
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

    fn scan_windows(&mut self) -> Result<(), ReplyOrIdError> {
        let tree_reply = self.conn.query_tree(self.screen.root)?.reply()?;

        let mut cookies = Vec::with_capacity(tree_reply.children.len());
        for win in tree_reply.children {
            let attr = self.conn.get_window_attributes(win)?;
            let geom = self.conn.get_geometry(win)?;
            cookies.push((win, attr, geom));
        }

        for (win, attr, geom) in cookies {
            if let (Ok(attr), Ok(geom)) = (attr.reply(), geom.reply()) {
                if !attr.override_redirect && attr.map_state != MapState::UNMAPPED {
                    self.manage_window(win, &geom)?;
                }
            }
        }

        Ok(())
    }

    fn manage_window(
        &mut self,
        win: Window,
        geom: &GetGeometryReply,
    ) -> Result<(), ReplyOrIdError> {
        println!("managing new window");
        let frame_win = self.conn.generate_id()?;
        let win_aux = CreateWindowAux::new()
            .event_mask(
                EventMask::EXPOSURE
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::KEY_PRESS
                    | EventMask::KEY_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::ENTER_WINDOW
                    | EventMask::LEAVE_WINDOW
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::PROPERTY_CHANGE,
            )
            .border_pixel(theme::WINDOW_BORDER_UNFOCUSED)
            .background_pixel(self.screen.black_pixel);

        self.conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            frame_win,
            self.screen.root,
            geom.x,
            geom.y,
            geom.width,
            geom.height,
            1,
            WindowClass::INPUT_OUTPUT,
            0,
            &win_aux,
        )?;

        self.conn.grab_server()?;
        self.conn.change_save_set(SetMode::INSERT, win)?;

        let cookie = self.conn.reparent_window(win, frame_win, 0, 0)?;

        self.conn.map_window(win)?;
        self.conn.map_window(frame_win)?;
        self.conn.ungrab_server()?;

        self.unfocus_focused_client()?;

        self.focused_workspace
            .borrow_mut()
            .push_client(WClientState::new(win, frame_win, geom));

        // remember and ignore all reparent_window events
        self.ignore_sequences
            .push(Reverse(cookie.sequence_number() as u16));

        self.recompute_layout()?;

        self.focus_selected()?;
        self.warp_pointer_to_focused_client()?;

        Ok(())
    }

    fn recompute_layout(&mut self) -> Result<(), ReplyOrIdError> {
        let ws = self.focused_workspace.borrow();
        let rects = layout_clients(&self.layout, &self.focused_monitor.borrow(), &ws.clients);

        if rects.is_none() {
            return Ok(());
        }

        for (client, rect) in ws.clients.inner().iter().zip(rects.unwrap()) {
            let frame_aux = ConfigureWindowAux::from(rect)
                .sibling(None)
                .stack_mode(None);
            let client_aux = frame_aux.clone().x(0).y(0);

            let mut c = client.borrow_mut();
            c.rect = rect;

            self.conn.configure_window(c.window, &client_aux)?;
            self.conn.configure_window(c.frame, &frame_aux)?;
        }
        self.conn.flush()?;
        Ok(())
    }

    #[allow(dead_code)]
    fn refresh(&mut self) {
        // TODO: when the bar is implemented, we want to update the bar information here
        todo!()
    }

    fn handle_unmap_notify(&mut self, evt: UnmapNotifyEvent) -> Result<(), ReplyOrIdError> {
        self.reparent_and_destroy_frame(evt.window);
        self.recompute_layout()?;
        Ok(())
    }

    fn reparent_and_destroy_frame(&mut self, window: Window) {
        let root = self.screen.root;
        let conn = self.conn;

        let mut ws = self.focused_workspace.borrow_mut();
        ws.clients.retain(|client| {
            let c = client.borrow();
            if c.window != window {
                return true;
            }

            conn.change_save_set(SetMode::DELETE, c.window).unwrap();
            conn.reparent_window(c.window, root, c.rect.x, c.rect.y)
                .unwrap();
            conn.destroy_window(c.frame).unwrap();
            false
        });
    }

    fn handle_configure_request(&mut self, evt: ConfigureRequestEvent) -> Result<(), ReplyError> {
        let aux = ConfigureWindowAux::from_configure_request(&evt)
            .sibling(None)
            .stack_mode(None);
        self.conn.configure_window(evt.window, &aux)?;
        Ok(())
    }

    fn handle_map_request(&mut self, evt: MapRequestEvent) -> Result<(), ReplyOrIdError> {
        self.manage_window(evt.window, &self.conn.get_geometry(evt.window)?.reply()?)
    }

    fn handle_expose(&mut self, evt: ExposeEvent) {
        self.pending_exposure.insert(evt.window);
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
                c.frame == win || c.window == win
            });
            true
        });
        self.focused_monitor = self.monitors.selected().unwrap();
        self.focused_workspace = self.focused_monitor.borrow().focused_workspace();
    }

    fn handle_enter(&mut self, evt: EnterNotifyEvent) -> Result<(), ReplyError> {
        // FIXME: maybe there's a better way?
        if self.ignore_enter {
            self.ignore_enter = false;
            return Ok(());
        }

        let entered_win = evt.event;

        let (frame, window) = {
            if let Some(client) = &self.focused_client {
                let c = client.borrow();
                (c.frame, c.window)
            } else {
                return Ok(());
            }
        };

        if frame == entered_win || window == entered_win {
            return Ok(());
        }
        self.unfocus_focused_client()?;

        {
            let in_workspace = self.focused_workspace.borrow().has_client(entered_win);
            if !in_workspace {
                self.entered_adjacent_monitor(entered_win);
            }

            let mut ws = self.focused_workspace.borrow_mut();
            ws.clients.find_and_select(|c| {
                let c = c.borrow();
                c.frame == entered_win || c.window == entered_win
            });
        }

        self.focus_selected()?;
        Ok(())
    }

    fn focus_selected(&mut self) -> Result<(), ReplyError> {
        let ws = self.focused_workspace.borrow();
        self.focused_client = ws.focused_client();

        let (frame, win) = {
            if let Some(client) = &self.focused_client {
                let c = client.borrow();
                (c.frame, c.window)
            } else {
                return Ok(());
            }
        };

        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, win, CURRENT_TIME)?;
        self.conn.configure_window(
            frame,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;

        let focus_aux = ChangeWindowAttributesAux::new().border_pixel(theme::WINDOW_BORDER_FOCUSED);
        self.conn.change_window_attributes(frame, &focus_aux)?;

        self.ignore_enter = true;

        self.conn.flush()?;

        Ok(())
    }

    fn unfocus_focused_client(&mut self) -> Result<(), ReplyError> {
        if let Some(client) = &self.focused_client {
            let frame = client.borrow().frame;
            let unfocus_aux =
                ChangeWindowAttributesAux::new().border_pixel(theme::WINDOW_BORDER_UNFOCUSED);
            self.conn.change_window_attributes(frame, &unfocus_aux)?;
            self.conn.flush()?;
            self.focused_client = None;
        }
        Ok(())
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
                self.drag_window = Some((c.frame, (x, y)));
            }
        }
    }

    fn handle_button_release(&mut self, evt: ButtonReleaseEvent) -> Result<(), ReplyError> {
        if evt.detail == DRAG_BUTTON {
            self.drag_window = None;
        }

        Ok(())
    }

    fn handle_motion_notify(&mut self, evt: MotionNotifyEvent) -> Result<(), ReplyError> {
        if let Some((win, (x, y))) = self.drag_window {
            let (x, y) = (x + evt.root_x, y + evt.root_y);
            let (x, y) = (x as i32, y as i32);
            self.conn
                .configure_window(win, &ConfigureWindowAux::new().x(x).y(y))?;
        }
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
            .get_property(false, window, prop, type_, 0, u32::MAX)?
            .reply()?;
        let found = reply.value32().unwrap().find(|a| a == &atom).is_some();
        Ok(found)
    }

    fn send_delete_event(&mut self, window: Window) -> Result<(), ReplyError> {
        let event = ClientMessageEvent::new(
            32,
            window,
            self.atoms.WM_PROTOCOLS,
            [self.atoms.WM_DELETE_WINDOW, 0, 0, 0, 0],
        );
        self.conn
            .send_event(false, window, EventMask::NO_EVENT, event)?;
        self.conn.flush()?;
        Ok(())
    }

    fn destroy_window(&mut self) -> Result<bool, ReplyOrIdError> {
        let window = {
            if let Some(client) = &self.focused_client {
                client.borrow().window
            } else {
                return Ok(false);
            }
        };
        let delete_exists = self.window_property_exists(
            window,
            self.atoms.WM_DELETE_WINDOW,
            self.atoms.WM_PROTOCOLS,
            self.atoms.ATOM_ATOM,
        )?;

        self.reparent_and_destroy_frame(window);

        if delete_exists {
            self.send_delete_event(window)?;
        } else {
            self.conn.kill_client(window)?;
        }

        self.recompute_layout()?;

        self.focus_selected()?;
        self.warp_pointer_to_focused_client()?;

        return Ok(true);
    }

    fn spawn_program(&self, cmd: &'static [&'static str]) {
        let prog = cmd[0];

        if cmd.len() > 1 {
            let args = cmd.get(1..).unwrap();
            Command::new(prog).args(args).spawn().unwrap();
        } else {
            Command::new(prog).spawn().unwrap();
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
            WCommand::FocusClientPrev => self.focus_adjacent(StackDirection::Prev),
            WCommand::FocusClientNext => self.focus_adjacent(StackDirection::Next),
            WCommand::MoveClientPrev => self.move_adjacent(StackDirection::Prev)?,
            WCommand::MoveClientNext => self.move_adjacent(StackDirection::Next)?,
            WCommand::FocusMonitorNext => self.focus_adjacent_monitor(StackDirection::Next)?,
            WCommand::FocusMonitorPrev => self.focus_adjacent_monitor(StackDirection::Prev)?,
            WCommand::Spawn(cmd) => self.spawn_program(cmd),
            WCommand::Destroy => {
                if self.destroy_window()? {
                    self.ignore_enter = true;
                }
            }
            WCommand::Exit => self.try_exit(),
            WCommand::Idle => {}
        }
        Ok(())
    }

    fn warp_pointer_to_focused_client(&self) -> Result<(), ReplyOrIdError> {
        if let Some(client) = &self.focused_client {
            let c = client.borrow();
            println!("warping pointer to {} @ rect: {:#?}", c.window, c.rect);
            self.conn.warp_pointer(
                NONE,
                c.frame,
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

    fn focus_adjacent(&mut self, dir: StackDirection) {
        self.unfocus_focused_client().unwrap();
        {
            self.focused_workspace.borrow_mut().focus_neighbor(dir);
        }
        self.focus_selected().unwrap();
        self.warp_pointer_to_focused_client().unwrap();
    }

    fn handle_xkb_state_notify(&mut self, evt: StateNotifyEvent) -> Result<(), ReplyOrIdError> {
        // println!("EVENT: {evt:#?}");
        if i32::try_from(evt.device_id).unwrap() == self.keyboard.device_id {
            self.keyboard.update_state_mask(evt);
        }
        Ok(())
    }

    fn handle_event(&mut self, evt: Event) -> Result<ShouldExit, ReplyOrIdError> {
        let mut should_ignore = false;

        if let Some(seqno) = evt.wire_sequence_number() {
            while let Some(&Reverse(to_ignore)) = self.ignore_sequences.peek() {
                if to_ignore.wrapping_sub(seqno) <= u16::MAX / 2 {
                    should_ignore = to_ignore == seqno;
                    break;
                }
                self.ignore_sequences.pop();
            }
        }

        if should_ignore {
            return Ok(ShouldExit::No);
        }

        match evt {
            Event::UnmapNotify(e) => self.handle_unmap_notify(e)?,
            Event::ConfigureRequest(e) => self.handle_configure_request(e)?,
            Event::MapRequest(e) => self.handle_map_request(e)?,
            Event::Expose(e) => self.handle_expose(e),
            Event::EnterNotify(e) => self.handle_enter(e)?,
            Event::ButtonPress(e) => self.handle_button_press(e),
            Event::ButtonRelease(e) => self.handle_button_release(e)?,
            Event::MotionNotify(e) => self.handle_motion_notify(e)?,
            Event::KeyPress(e) => self.handle_key_press(e)?,
            Event::XkbStateNotify(e) => self.handle_xkb_state_notify(e)?,
            Event::Error(e) => eprintln!("ERROR: {e:#?}"),
            _ => {}
        }

        Ok(ShouldExit::No)
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        'eventloop: loop {
            // self.refresh();
            self.conn.flush()?;

            while let Some(event) = self.conn.wait_for_event().ok() {
                if self.handle_event(event)? == ShouldExit::Yes {
                    break 'eventloop;
                }
            }
        }
        Ok(())
    }

    fn focus_adjacent_monitor(&mut self, dir: StackDirection) -> Result<(), ReplyError> {
        self.unfocus_focused_client()?;

        match dir {
            StackDirection::Prev => self.monitors.prev_index(true, true),
            StackDirection::Next => self.monitors.next_index(true, true),
        };

        let tmp = self.monitors.inner();
        println!(
            "focusing monitor {} of {} possible",
            self.monitors.index() + 1,
            tmp.len()
        );

        let mon = self.monitors.selected().unwrap();

        self.focused_workspace = mon.borrow().focused_workspace();
        self.focused_client = self.focused_workspace.borrow().focused_client();
        self.focused_monitor = mon;

        self.warp_pointer_to_focused_monitor().unwrap();

        self.focus_selected()?;

        self.warp_pointer_to_focused_client().unwrap();
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
}

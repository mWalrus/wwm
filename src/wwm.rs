use crate::{
    client::ClientState,
    config::{commands::WCommand, mouse::DRAG_BUTTON, theme},
    keyboard::WKeyboard,
    layouts::{layout_clients, WLayout},
    monitor::{StackDirection, WMonitor, WWorkspace},
    AtomCollection,
};
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
    process::exit,
};
use x11rb::{
    connection::Connection,
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
    rust_connection::{ReplyError, ReplyOrIdError},
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME,
};

pub struct WinMan<'a, C: Connection> {
    conn: &'a C,
    screen: &'a Screen,
    monitors: Vec<WMonitor>,
    focused_monitor: usize,
    ignore_sequences: BinaryHeap<Reverse<u16>>,
    pending_exposure: HashSet<Window>,
    drag_window: Option<(Window, (i16, i16))>,
    layout: WLayout,
    keyboard: WKeyboard,
    atoms: AtomCollection,
    ignore_enter: bool,
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
        Self::become_wm(conn, screen).unwrap();
        let monitors = Self::get_monitors(conn, screen).unwrap();
        let focused_monitor = monitors.iter().position(|m| m.primary).unwrap_or(0);

        let mut wwm = Self {
            conn,
            screen,
            monitors,
            focused_monitor,
            ignore_sequences: Default::default(),
            pending_exposure: Default::default(),
            drag_window: None,
            layout: WLayout::Tile,
            keyboard,
            atoms,
            ignore_enter: false,
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

    fn become_wm(conn: &'a C, screen: &Screen) -> Result<(), ReplyError> {
        // set up substructure redirects for the root window.
        // NOTE: this will fail if another window manager is already running
        let change = ChangeWindowAttributesAux::default().event_mask(
            EventMask::SUBSTRUCTURE_REDIRECT
                | EventMask::SUBSTRUCTURE_NOTIFY
                | EventMask::BUTTON_PRESS
                | EventMask::STRUCTURE_NOTIFY
                | EventMask::PROPERTY_CHANGE,
        );
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

        self.focused_workspace_mut()
            .clients
            .push(ClientState::new(win, frame_win, geom));

        // remember and ignore all reparent_window events
        self.ignore_sequences
            .push(Reverse(cookie.sequence_number() as u16));

        self.recompute_layout()?;

        Ok(())
    }

    fn focused_workspace_mut(&mut self) -> &mut WWorkspace {
        let mon = &mut self.monitors[self.focused_monitor];
        let workspace = &mut mon.workspaces[mon.focused_workspace];
        &mut *workspace
    }

    fn focused_workspace(&self) -> &WWorkspace {
        let mon = &self.monitors[self.focused_monitor];
        &mon.workspaces[mon.focused_workspace]
    }

    fn recompute_layout(&mut self) -> Result<(), ReplyOrIdError> {
        let workspace = self.focused_workspace();
        let rects = layout_clients(
            &self.monitors[self.focused_monitor],
            &workspace.clients,
            &self.layout,
        );

        if rects.is_none() {
            return Ok(());
        }

        for (state, rect) in workspace.clients.iter().zip(rects.unwrap()) {
            let frame_aux = ConfigureWindowAux::from(rect)
                .sibling(None)
                .stack_mode(None);
            let client_aux = frame_aux.clone().x(0).y(0);
            self.conn.configure_window(state.window, &client_aux)?;
            self.conn.configure_window(state.frame, &frame_aux)?;
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

        let workspace = self.focused_workspace_mut();
        workspace.clients.retain(|state| {
            if state.window != window {
                return true;
            }

            conn.change_save_set(SetMode::DELETE, state.window).unwrap();
            conn.reparent_window(state.window, root, state.rect.x, state.rect.y)
                .unwrap();
            conn.destroy_window(state.frame).unwrap();
            false
        });
        workspace.correct_focus();
    }

    fn handle_configure_request(&mut self, evt: ConfigureRequestEvent) -> Result<(), ReplyError> {
        if let Some(state) = self.find_client_by_id_mut(evt.window) {
            let _ = state;
            unimplemented!();
        }

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

    fn handle_enter(&mut self, evt: EnterNotifyEvent) -> Result<(), ReplyError> {
        // FIXME: maybe there's a better way?
        if self.ignore_enter {
            self.ignore_enter = false;
            return Ok(());
        }

        let (frame, window) = {
            if let Some(state) = self.find_client_by_id(evt.event) {
                (state.frame, state.window)
            } else {
                return Ok(());
            }
        };
        let workspace = self.focused_workspace();
        if let Some(idx) = workspace.focused_client {
            let focused_frame = workspace.clients[idx].frame;
            if frame != focused_frame {
                self.focus(frame, window)?;
                self.unfocus(focused_frame)?;
            }
        } else {
            self.focus(frame, window)?;
        }
        Ok(())
    }

    fn focus(&mut self, frame: Window, window: Window) -> Result<(), ReplyError> {
        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, window, CURRENT_TIME)?;
        self.conn.configure_window(
            frame,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;

        let focus_aux = ChangeWindowAttributesAux::new().border_pixel(theme::WINDOW_BORDER_FOCUSED);
        self.conn.change_window_attributes(frame, &focus_aux)?;
        self.conn.flush()?;

        self.focused_workspace_mut().set_focus_from_frame(frame);

        Ok(())
    }

    fn unfocus(&mut self, frame: Window) -> Result<(), ReplyError> {
        let unfocus_aux =
            ChangeWindowAttributesAux::new().border_pixel(theme::WINDOW_BORDER_UNFOCUSED);
        self.conn.change_window_attributes(frame, &unfocus_aux)?;
        self.conn.flush()?;
        Ok(())
    }

    fn handle_button_press(&mut self, evt: ButtonPressEvent) {
        if evt.detail != DRAG_BUTTON || u16::from(evt.state) != 0 {
            return;
        }
        if let Some(state) = self.find_client_by_id(evt.event) {
            if self.drag_window.is_none() && evt.event_x < 0.max(state.rect.width as i16) {
                let (x, y) = (-evt.event_x, -evt.event_y);
                self.drag_window = Some((state.frame, (x, y)));
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
        self.recompute_layout().unwrap();
        Ok(())
    }

    fn destroy_window(&mut self) -> Result<bool, ReplyOrIdError> {
        let workspace = self.focused_workspace();
        if let Some(idx) = workspace.focused_client {
            println!("destroy window focused client index: {idx}");
            let window = workspace.clients[idx].window;

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

            return Ok(true);
        }
        Ok(false)
    }

    fn handle_key_press(&mut self, evt: KeyPressEvent) -> Result<(), ReplyOrIdError> {
        let sym = self.keyboard.key_sym(evt.detail.into());

        let mut action = WCommand::PassThrough;
        for bind in &self.keyboard.keybinds {
            if bind.keysym == sym && evt.state == bind.mods_as_key_but_mask() {
                action = bind.action;
                break;
            }
        }

        match action {
            WCommand::FocusUp => self.focus_adjacent(StackDirection::Prev),
            WCommand::FocusDown => self.focus_adjacent(StackDirection::Next),
            WCommand::MoveUp => self.move_adjacent(StackDirection::Prev)?,
            WCommand::MoveDown => self.move_adjacent(StackDirection::Next)?,
            WCommand::Spawn(cmd) => {
                println!("running spawn command: {cmd:?}");
            }
            WCommand::Destroy => {
                if self.destroy_window()? {
                    self.ignore_enter = true;
                    self.focus_adjacent(StackDirection::Next);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn move_adjacent(&mut self, dir: StackDirection) -> Result<(), ReplyOrIdError> {
        let workspace = self.focused_workspace_mut();
        if let Some(idx) = workspace.focused_client {
            let new_idx = workspace.idx_from_direction(idx, dir);
            workspace.swap(new_idx, idx);
            workspace.set_focus(new_idx);

            // NOTE: since the cursor stays in the same spot after moving clients
            // we will generate a `EnterNotify` event since we are now hovering a new window.
            // This flag helps the enter notify handler to decide whether we want to
            // process the event.
            self.ignore_enter = true;
            self.recompute_layout()?;
        }
        Ok(())
    }

    fn focus_adjacent(&mut self, dir: StackDirection) {
        let workspace = self.focused_workspace_mut();
        if let Some((frame, idx)) = workspace.focused_frame_and_idx() {
            let new_idx = workspace.idx_from_direction(idx, dir);

            self.unfocus(frame).unwrap();
            let ClientState { frame, window, .. } = self.focused_workspace().clients[new_idx];
            self.focus(frame, window).unwrap();
        }
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

    fn find_client_by_id_mut(&mut self, win: Window) -> Option<&mut ClientState> {
        self.focused_workspace_mut()
            .clients
            .iter_mut()
            .find(|state| state.window == win || state.frame == win)
    }

    fn find_client_by_id(&self, win: Window) -> Option<&ClientState> {
        self.focused_workspace()
            .clients
            .iter()
            .find(|state| state.window == win || state.frame == win)
    }
}

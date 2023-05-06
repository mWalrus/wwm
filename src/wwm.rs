use crate::{
    client::ClientState,
    config::{commands::WCommand, mouse::DRAG_BUTTON, theme},
    keyboard::WKeyboard,
    layouts::{layout_clients, WLayout},
    monitor::Monitor,
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
        xkb::StateNotifyEvent,
        xproto::{
            Allow, ButtonPressEvent, ButtonReleaseEvent, ChangeWindowAttributesAux,
            ClientMessageEvent, ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt,
            CreateWindowAux, EnterNotifyEvent, EventMask, ExposeEvent, GetGeometryReply,
            InputFocus, KeyPressEvent, MapRequestEvent, MapState, MotionNotifyEvent, Screen,
            SetMode, StackMode, UnmapNotifyEvent, Window, WindowClass,
        },
        ErrorKind, Event,
    },
    rust_connection::{ReplyError, ReplyOrIdError},
    wrapper::ConnectionExt as _,
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME,
};
const CLIENT_CAP: usize = 256;

pub struct WinMan<'a, C: Connection> {
    conn: &'a C,
    screen_num: usize,
    clients: Vec<ClientState>,
    ignore_sequences: BinaryHeap<Reverse<u16>>,
    pending_exposure: HashSet<Window>,
    drag_window: Option<(Window, (i16, i16))>,
    layout: WLayout,
    last_focused: Option<(Window, Window)>,
    keyboard: WKeyboard,
    atoms: AtomCollection,
    ignore_enter: bool,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum ShouldExit {
    Yes,
    No,
}

enum StackDirection {
    Prev,
    Next,
}

impl<'a, C: Connection> WinMan<'a, C> {
    pub fn init(
        conn: &'a C,
        screen_num: usize,
        keyboard: WKeyboard,
        atoms: AtomCollection,
    ) -> Self {
        // TODO: error handling
        Self::become_wm(conn, screen_num).unwrap();

        let mut wwm = Self {
            conn,
            screen_num,
            clients: Vec::with_capacity(CLIENT_CAP),
            ignore_sequences: Default::default(),
            pending_exposure: Default::default(),
            drag_window: None,
            layout: WLayout::Tile,
            last_focused: None,
            keyboard,
            atoms,
            ignore_enter: false,
        };

        // take care of potentially unmanaged windows
        wwm.scan_windows().unwrap();
        wwm
    }

    fn become_wm(conn: &'a C, screen_num: usize) -> Result<(), ReplyError> {
        // set up substructure redirects for the root window.
        // NOTE: this will fail if another window manager is already running
        let screen = &conn.setup().roots[screen_num];
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
        let screen = &self.conn.setup().roots[self.screen_num];
        let tree_reply = self.conn.query_tree(screen.root)?.reply()?;

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
        let screen = &self.conn.setup().roots[self.screen_num];
        assert!(!self.window_id_in_use(win));

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
            .background_pixel(screen.black_pixel);

        self.conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            frame_win,
            screen.root,
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

        self.clients.push(ClientState::new(win, frame_win, geom));

        // remember and ignore all reparent_window events
        self.ignore_sequences
            .push(Reverse(cookie.sequence_number() as u16));

        self.recompute_layout(screen)?;

        Ok(())
    }

    fn window_id_in_use(&self, win: Window) -> bool {
        self.clients
            .iter()
            .find(|c| c.window == win || c.frame == win)
            .is_some()
    }

    fn recompute_layout(&mut self, s: &Screen) -> Result<(), ReplyOrIdError> {
        let rects = layout_clients(
            &Monitor {
                x: 0,
                y: 0,
                width: s.width_in_pixels,
                height: s.height_in_pixels,
            },
            &self.clients,
            &self.layout,
        );

        for (state, rect) in self.clients.iter().zip(rects) {
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

    fn refresh(&mut self) {
        // TODO: when the bar is implemented, we want to update the bar information here
        todo!()
    }

    fn handle_unmap_notify(&mut self, evt: UnmapNotifyEvent) -> Result<(), ReplyOrIdError> {
        let screen = &self.conn.setup().roots[self.screen_num];
        let root = screen.root;
        let conn = self.conn;

        self.clients.retain(|state| {
            if state.window != evt.window {
                return true;
            }

            conn.change_save_set(SetMode::DELETE, state.window).unwrap();
            conn.reparent_window(state.window, root, state.rect.x, state.rect.y)
                .unwrap();
            conn.destroy_window(state.frame).unwrap();
            false
        });
        self.recompute_layout(screen)?;
        Ok(())
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
        if let Some((focused_frame, _)) = self.last_focused {
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
        println!("setting focus on frame {frame} containing window {window}");
        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, window, CURRENT_TIME)?;
        self.conn.configure_window(
            frame,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;

        let focus_aux = ChangeWindowAttributesAux::new().border_pixel(theme::WINDOW_BORDER_FOCUSED);
        self.conn.change_window_attributes(frame, &focus_aux)?;
        self.conn.flush()?;

        self.last_focused = Some((frame, window));

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
        let screen = &self.conn.setup().roots[self.screen_num];
        self.recompute_layout(screen).unwrap();
        Ok(())
    }

    fn handle_key_press(&mut self, evt: KeyPressEvent) -> Result<(), ReplyOrIdError> {
        println!("=========== got keypress");
        let sym = self.keyboard.key_sym(evt.detail.into());

        let mut action = WCommand::PassThrough;
        for bind in &self.keyboard.keybinds {
            if bind.keysym == sym && evt.state == bind.mods_as_key_but_mask() {
                action = bind.action
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
                if let Some((_, window)) = self.last_focused {
                    println!("Destroying window {window}");
                    if let Err(e) = self.send_delete_event(window) {
                        eprintln!("Failed to kill window {window} gracefully, force killing");
                        eprintln!("ERROR: {e}");
                        self.conn.kill_client(window)?;
                    }
                    self.focus_adjacent(StackDirection::Next);
                }
            }
            WCommand::PassThrough => {
                // LINK: https://www.x.org/releases/X11R7.6/doc/xproto/x11protocol.html#requests:AllowEvents
                // LINK: https://www.x.org/releases/X11R7.6/doc/xproto/x11protocol.html#requests:GrabKeyboard
                if let Some((_, window)) = self.last_focused {
                    println!("Sending key event to window {window}");
                    self.conn
                        .allow_events(Allow::REPLAY_KEYBOARD, CURRENT_TIME)?;
                    self.conn.sync()?;
                    self.conn
                        .send_event(true, window, EventMask::KEY_PRESS, evt)?;
                    println!("Flushing event pool");
                    self.conn.flush()?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn move_adjacent(&mut self, dir: StackDirection) -> Result<(), ReplyOrIdError> {
        if let Some(idx) = self.currently_focused_client_index() {
            let new_idx = self.client_idx_from_direction(idx, dir);
            let screen = &self.conn.setup().roots[self.screen_num];
            self.clients.swap(new_idx, idx);

            let ClientState { frame, .. } = self.clients[idx];
            self.unfocus(frame)?;

            let ClientState { window, frame, .. } = self.clients[new_idx];
            self.focus(frame, window)?;

            // NOTE: since the cursor stays in the same spot after moving clients
            // we will generate a `EnterNotify` event since we are now hovering a new window.
            // This flag helps the enter notify handler to decide whether we want to
            // process the event.
            self.ignore_enter = true;
            self.recompute_layout(screen)?;
        }
        Ok(())
    }

    fn focus_adjacent(&mut self, dir: StackDirection) {
        if let Some(idx) = self.currently_focused_client_index() {
            let (frame, _) = self.last_focused.unwrap();
            self.unfocus(frame).unwrap();

            let new_idx = self.client_idx_from_direction(idx, dir);
            let ClientState { frame, window, .. } = self.clients[new_idx];
            self.focus(frame, window).unwrap();
        } else {
            eprintln!("ERROR: could not find index of focused client");
        }
    }

    fn client_idx_from_direction(&self, idx: usize, dir: StackDirection) -> usize {
        match dir {
            StackDirection::Prev => {
                if idx == 0 {
                    self.clients.len() - 1
                } else {
                    idx - 1
                }
            }
            StackDirection::Next => {
                if idx == self.clients.len() - 1 {
                    0
                } else {
                    idx + 1
                }
            }
        }
    }

    fn handle_xkb_state_notify(&mut self, evt: StateNotifyEvent) -> Result<(), ReplyOrIdError> {
        println!("EVENT: {evt:#?}");
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

    fn currently_focused_client_index(&self) -> Option<usize> {
        if let Some((frame, _)) = self.last_focused {
            return self.clients.iter().position(|state| state.frame == frame);
        }
        None
    }

    fn find_client_by_id_mut(&mut self, win: Window) -> Option<&mut ClientState> {
        self.clients
            .iter_mut()
            .find(|state| state.window == win || state.frame == win)
    }

    fn find_client_by_id(&self, win: Window) -> Option<&ClientState> {
        self.clients
            .iter()
            .find(|state| state.window == win || state.frame == win)
    }
}

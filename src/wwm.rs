use crate::{
    client::ClientState,
    config::{keymap::MOD_KEY, mouse::DRAG_BUTTON, theme},
    layouts::{layout_clients, WLayout},
    monitor::Monitor,
};
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
    process::exit,
};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{
            Atom, ButtonPressEvent, ButtonReleaseEvent, ChangeWindowAttributesAux,
            ClientMessageEvent, ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt,
            CreateWindowAux, EnterNotifyEvent, EventMask, ExposeEvent, GetGeometryReply, GrabMode,
            GrabStatus, InputFocus, KeyPressEvent, MapRequestEvent, MapState, MotionNotifyEvent,
            Screen, SetMode, StackMode, UnmapNotifyEvent, Window, WindowClass,
        },
        ErrorKind, Event,
    },
    rust_connection::{ReplyError, ReplyOrIdError},
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME,
};

const CLIENT_CAP: usize = 256;

pub struct WinMan<'a, C: Connection> {
    conn: &'a C,
    screen_num: usize,
    clients: Vec<ClientState>,
    ignore_sequences: BinaryHeap<Reverse<u16>>,
    pending_exposure: HashSet<Window>,
    wm_protocols: Atom,
    wm_delete_window: Atom,
    drag_window: Option<(Window, (i16, i16))>,
    layout: WLayout,
    last_focused: Option<Window>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum ShouldExit {
    Yes,
    No,
}

impl<'a, C: Connection> WinMan<'a, C> {
    pub fn init(conn: &'a C, screen_num: usize) -> Self {
        // TODO: error handling
        Self::become_wm(conn, screen_num).unwrap();

        let wm_protocols = conn.intern_atom(false, b"WM_PROTOCOLS").unwrap();
        let wm_delete_window = conn.intern_atom(false, b"WM_DELETE_WINDOW").unwrap();

        let mut wwm = Self {
            conn,
            screen_num,
            clients: Vec::with_capacity(CLIENT_CAP),
            ignore_sequences: Default::default(),
            pending_exposure: Default::default(),
            wm_protocols: wm_protocols.reply().unwrap().atom,
            wm_delete_window: wm_delete_window.reply().unwrap().atom,
            drag_window: None,
            layout: WLayout::Tile,
            last_focused: None,
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
                | EventMask::KEY_PRESS
                | EventMask::KEY_RELEASE,
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

        let grab_cookie = conn.grab_keyboard(
            true,
            screen.root,
            CURRENT_TIME,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        )?;

        if let Ok(r) = grab_cookie.reply() {
            if r.status == GrabStatus::SUCCESS {
                // TODO: actual logger
                println!("Successfully grabbed the keyboard");
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
                    | EventMask::POINTER_MOTION
                    | EventMask::ENTER_WINDOW,
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
        let (frame, window) = {
            if let Some(state) = self.find_client_by_id(evt.event) {
                (state.frame, state.window)
            } else {
                return Ok(());
            }
        };
        if let Some(last_frame) = self.last_focused {
            if frame != last_frame {
                self.focus(frame, window)?;
                self.unfocus(last_frame)?;
            }
        } else {
            self.focus(frame, window)?;
        }
        Ok(())
    }

    fn focus(&mut self, frame: Window, window: Window) -> Result<(), ReplyError> {
        self.conn
            .set_input_focus(InputFocus::PARENT, window, CURRENT_TIME)?;
        self.conn.configure_window(
            frame,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;

        let focus_aux = ChangeWindowAttributesAux::new().border_pixel(theme::WINDOW_BORDER_FOCUSED);
        self.conn.change_window_attributes(frame, &focus_aux)?;
        self.conn.flush()?;

        self.last_focused = Some(frame);

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

        if let Some(state) = self.find_client_by_id(evt.event) {
            if evt.event_x >= 0.max(state.rect.width as i16) {
                let event = ClientMessageEvent::new(
                    32,
                    state.window,
                    self.wm_protocols,
                    [self.wm_delete_window, 0, 0, 0, 0],
                );
                self.conn
                    .send_event(false, state.window, EventMask::NO_EVENT, event)?;
            }
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

    fn handle_key_press(&mut self, evt: KeyPressEvent) -> Result<(), ReplyOrIdError> {
        println!("{evt:#?}");

        // TODO: https://github.com/psychon/x11rb/issues/782#issuecomment-1367881755

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

        // TODO: key press/release events
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

use std::{cmp::Reverse, collections::BinaryHeap, process::exit};

use smallmap::Map;
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{
            Atom, ButtonPressEvent, ButtonReleaseEvent, ChangeWindowAttributesAux,
            ConfigureRequestEvent, ConnectionExt, CreateWindowAux, EnterNotifyEvent, EventMask,
            ExposeEvent, GetGeometryReply, MapRequestEvent, MapState, MotionNotifyEvent, Screen,
            SetMode, UnmapNotifyEvent, Window, WindowClass,
        },
        ErrorKind,
    },
    rust_connection::{ReplyError, ReplyOrIdError},
    COPY_DEPTH_FROM_PARENT,
};

use crate::config::theme;

pub struct WinMan<'a, C: Connection> {
    conn: &'a C,
    screen_num: usize,
    clients: Map<Window, Window>,
    ignore_sequences: BinaryHeap<Reverse<u16>>,
    wm_protocols: Atom,
    wm_delete_window: Atom,
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
            clients: Map::with_capacity(256),
            ignore_sequences: Default::default(),
            wm_protocols: wm_protocols.reply().unwrap().atom,
            wm_delete_window: wm_delete_window.reply().unwrap().atom,
        };

        // take care of potentially unmanaged windows
        wwm.scan_windows().unwrap();

        wwm
    }

    fn become_wm(conn: &'a C, screen_num: usize) -> Result<(), ReplyError> {
        // set up substructure redirects for the root window.
        // NOTE: this will fail if another window manager is already running
        let screen = &conn.setup().roots[screen_num];
        let change = ChangeWindowAttributesAux::default()
            .event_mask(EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY);
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

        self.clients.insert(frame_win, win);

        // remember and ignore all reparent_window events
        self.ignore_sequences
            .push(Reverse(cookie.sequence_number() as u16));

        Ok(())
    }

    fn window_id_in_use(&self, win: Window) -> bool {
        self.clients.contains_key(&win)
    }

    fn refresh(&mut self) {
        // TODO: when the bar is implemented, we want to update the bar information here
        todo!()
    }

    fn handle_unmap_notify(&mut self, evt: UnmapNotifyEvent) {
        todo!()
    }

    fn handle_configure_request(&mut self, evt: ConfigureRequestEvent) {
        todo!()
    }

    fn handle_map_request(&mut self, evt: MapRequestEvent) {
        todo!()
    }

    fn handle_expose(&mut self, evt: ExposeEvent) {
        todo!()
    }

    fn handle_enter(&mut self, evt: EnterNotifyEvent) {
        todo!()
    }

    fn handle_button_press(&mut self, evt: ButtonPressEvent) {
        todo!()
    }

    fn handle_button_release(&mut self, evt: ButtonReleaseEvent) {
        todo!()
    }

    fn handle_motion_notify(&mut self, evt: MotionNotifyEvent) {
        todo!()
    }

    pub fn run() {
        todo!()
    }
}

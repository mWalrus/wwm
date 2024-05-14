mod client;
mod command;
mod config;
mod keyboard;
mod layouts;
mod monitor;
mod mouse;
mod wwm;

use keyboard::WKeyboard;
use lazy_static::lazy_static;
use mouse::WMouse;
use wwm::WinMan;
use x11rb::atom_manager;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::Screen;
use x11rb::xcb_ffi::XCBConnection;

atom_manager! {
    pub AtomCollection: AtomCollectionsCookie {
        UTF8_STRING,
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        WM_STATE,
        WM_TAKE_FOCUS,
        WM_TRANSIENT_FOR,
        WM_HINTS,
        WM_CLASS,
        WM_SIZE_HINTS,
        WM_NORMAL_HINTS,
        ATOM,
        ATOM_ATOM,
        WINDOW,
        STRING,
        _NET_WM_NAME,
        _NET_SUPPORTED,
        _NET_CLIENT_LIST,
        _NET_CLIENT_INFO,
        _NET_ACTIVE_WINDOW,
        _NET_SUPPORTING_WM_CHECK,
        _NET_WM_STATE,
        _NET_WM_STATE_ADD,
        _NET_WM_STATE_TOGGLE,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DIALOG,
    }
}

pub struct X11Handle {
    conn: XCBConnection,
    xcb_conn: xcb::Connection,
    atoms: AtomCollection,
    screen_num: usize,
}

impl X11Handle {
    pub fn screen(&self) -> &Screen {
        &self.conn.setup().roots[self.screen_num]
    }
}

lazy_static! {
    pub static ref X_HANDLE: X11Handle = {
        let (xcb_conn, screen_num) = xcb::Connection::connect(None).unwrap();
        let screen_num = usize::try_from(screen_num).unwrap();

        let conn = {
            let raw_conn = xcb_conn.get_raw_conn().cast();
            unsafe { XCBConnection::from_raw_xcb_connection(raw_conn, false) }
        }
        .unwrap();
        let atoms = AtomCollection::new(&conn).unwrap();
        let atoms = atoms.reply().unwrap();

        X11Handle {
            conn,
            xcb_conn,
            atoms,
            screen_num,
        }
    };
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let keyboard = WKeyboard::new(&X_HANDLE.conn, &X_HANDLE.xcb_conn, X_HANDLE.screen())?;

    let mouse = WMouse::new(&X_HANDLE.conn, X_HANDLE.screen_num);

    let mut wwm = WinMan::init(keyboard, mouse)?;
    wwm.run()?;
    Ok(())
}

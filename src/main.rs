mod client;
mod command;
mod config;
mod keyboard;
mod layouts;
mod monitor;
mod mouse;
mod util;
mod workspace;
mod wwm;

use keyboard::WKeyboard;
use mouse::WMouse;
use wwm::WinMan;
use x11rb::atom_manager;
use x11rb::connection::Connection;
use x11rb::xcb_ffi::XCBConnection;

atom_manager! {
    pub AtomCollection: AtomCollectionsCookie {
        UTF8_STRING,
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        WM_STATE,
        WM_TAKE_FOCUS,
        WM_TRANSIENT_FOR,
        ATOM,
        ATOM_ATOM,
        WINDOW,
        _NET_WM_NAME,
        _NET_SUPPORTED,
        _NET_CLIENT_LIST,
        _NET_CLIENT_INFO,
        _NET_ACTIVE_WINDOW,
        _NET_SUPPORTING_WM_CHECK,
        _NET_WM_STATE,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DIALOG,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (xcb_conn, screen_num) = xcb::Connection::connect(None)?;
    let screen_num = usize::try_from(screen_num)?;

    let conn = {
        let raw_conn = xcb_conn.get_raw_conn().cast();
        unsafe { XCBConnection::from_raw_xcb_connection(raw_conn, false) }
    }?;

    let atoms = AtomCollection::new(&conn)?;
    let atoms = atoms.reply()?;

    let screen = &conn.setup().roots[screen_num];
    let keyboard = WKeyboard::new(&conn, &xcb_conn, screen)?;
    let mouse = WMouse::new(&conn, screen_num);

    let mut wwm = WinMan::init(&conn, screen_num, keyboard, mouse, atoms);
    wwm.run().unwrap();
    Ok(())
}

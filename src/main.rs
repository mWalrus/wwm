mod client;
mod config;
mod keyboard;
mod layouts;
mod monitor;
mod util;
mod workspace;
mod wwm;

use keyboard::WKeyboard;
use wwm::WinMan;
use x11rb::atom_manager;
use x11rb::connection::Connection;
use x11rb::xcb_ffi::XCBConnection;

atom_manager! {
    pub AtomCollection: AtomCollectionsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        ATOM_ATOM,
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

    let mut wwm = WinMan::init(&conn, screen_num, keyboard, atoms);
    wwm.run().unwrap();
    Ok(())
}

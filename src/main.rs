mod theme;
mod wwm;

use wwm::WinMan;
use x11rb::{connect, connection::Connection};

fn main() {
    let (conn, screen_num) = connect(None).unwrap();
    WinMan::init(&conn, screen_num);
}

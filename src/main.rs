mod config;
mod wwm;

use wwm::WinMan;
use x11rb::connect;

fn main() {
    let (conn, screen_num) = connect(None).unwrap();
    WinMan::init(&conn, screen_num);
}

pub mod bar;
pub mod cmd;
pub mod color;
pub mod primitives;

use x11rb::protocol::xproto::ConfigWindow;

#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
pub enum WLayout {
    #[default]
    MainStack,
    Column,
}

impl std::fmt::Display for WLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let symbol = match self {
            WLayout::MainStack => "[]=",
            WLayout::Column => "|||",
        };
        write!(f, "{symbol}")
    }
}

// FIXME: remove this wrapper after updating x11rb
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub struct WConfigWindow(pub ConfigWindow);

impl From<ConfigWindow> for WConfigWindow {
    fn from(value: ConfigWindow) -> Self {
        Self(value)
    }
}
impl std::ops::BitOr for WConfigWindow {
    type Output = WConfigWindow;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for WConfigWindow {
    type Output = bool;
    fn bitand(self, rhs: Self) -> Self::Output {
        u16::from(self.0) & u16::from(rhs.0) == u16::from(rhs.0)
    }
}

impl WConfigWindow {
    pub const X: Self = Self(ConfigWindow::X);
    pub const Y: Self = Self(ConfigWindow::Y);
    pub const WIDTH: Self = Self(ConfigWindow::WIDTH);
    pub const HEIGHT: Self = Self(ConfigWindow::HEIGHT);
    pub const BORDER_WIDTH: Self = Self(ConfigWindow::BORDER_WIDTH);
}

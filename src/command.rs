use crate::{layouts::WLayout, util::WDirection};

#[derive(Debug, Clone, Copy)]
pub enum WKeyCommand {
    Destroy,
    Exit,
    FocusClient(WDirection),
    MoveClient(WDirection),
    FocusMonitor(WDirection),
    Idle,
    AdjustMainWidth(WDirection),
    Layout(WLayout),
    SelectWorkspace(usize),
    Spawn(&'static [&'static str]),
    MoveClientToWorkspace(usize),
    MoveClientToMonitor(WDirection),
    UnFloat,
    Fullscreen,
}

#[derive(Debug, Clone, Copy)]
pub enum WMouseCommand {
    DragClient,
    ResizeClient,
    Idle,
}

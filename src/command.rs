use crate::layouts::WLayout;

#[derive(Debug, Clone, Copy)]
pub enum WDirection {
    Prev,
    Next,
}

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

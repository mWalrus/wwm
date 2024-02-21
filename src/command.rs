use wwm_core::util::WLayout;

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
    SelectTag(usize),
    Spawn(&'static [&'static str]),
    MoveClientToTag(usize),
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

use crate::{
    client::{ClientRect, ClientState},
    config::window::{GAP_SIZE, MAIN_CLIENT_WIDTH_PERCENTAGE},
    monitor::Monitor,
};

pub enum WLayout {
    Tile,
    Column,
    Floating,
}

pub fn layout_clients(
    mon: &Monitor,
    clients: &Vec<ClientState>,
    layout: &WLayout,
) -> Vec<ClientRect> {
    match layout {
        WLayout::Tile => tile(mon, clients),
        _ => todo!(),
    }
}

fn tile(mon: &Monitor, clients: &Vec<ClientState>) -> Vec<ClientRect> {
    if clients.len() == 1 {
        return vec![ClientRect::new(
            GAP_SIZE as i16,
            GAP_SIZE as i16,
            mon.width - (GAP_SIZE * 2),
            mon.height - (GAP_SIZE * 2),
        )];
    }

    let main_space = mon.width_from_percentage(MAIN_CLIENT_WIDTH_PERCENTAGE);
    let main_width = apply_gap(main_space);

    let mut rects = vec![ClientRect::new(
        GAP_SIZE as i16,
        GAP_SIZE as i16,
        main_width,
        mon.height - (GAP_SIZE * 2),
    )];

    let stack_width = apply_gap(mon.width - main_space);

    let non_main_window_count = clients.len() - 1;
    let stack_client_height = apply_gap(mon.client_height(non_main_window_count));

    let mut y_offset = GAP_SIZE;
    for _ in 0..non_main_window_count {
        rects.push(ClientRect::new(
            (main_space + (GAP_SIZE / 2)) as i16,
            y_offset as i16,
            stack_width,
            stack_client_height,
        ));
        y_offset += stack_client_height + GAP_SIZE;
    }

    rects
}

fn apply_gap(size: u16) -> u16 {
    size - (GAP_SIZE + (GAP_SIZE / 2))
}

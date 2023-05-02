use crate::{
    client::{ClientRect, ClientState},
    config::window::MAIN_CLIENT_WIDTH_PERCENTAGE,
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
        WLayout::Column => col(mon, clients),
        _ => todo!(),
    }
}

fn tile(mon: &Monitor, clients: &Vec<ClientState>) -> Vec<ClientRect> {
    let bw = clients[0].border_width;
    if clients.len() == 1 {
        return single_client(mon, bw);
    }

    let main_width = mon.width_from_percentage(MAIN_CLIENT_WIDTH_PERCENTAGE);

    let mut rects = vec![ClientRect::new(
        0,
        0,
        main_width - bw * 2,
        mon.height - bw * 2,
    )];

    let stack_width = mon.width - main_width;

    let non_main_window_count = clients.len() - 1;
    let stack_client_height = mon.height / non_main_window_count as u16;

    for i in 0..non_main_window_count {
        rects.push(ClientRect::new(
            (main_width - bw) as i16,
            (i as u16 * stack_client_height).saturating_sub(bw) as i16,
            stack_width - bw,
            stack_client_height - bw,
        ));
    }

    rects
}

fn col(mon: &Monitor, clients: &Vec<ClientState>) -> Vec<ClientRect> {
    let bw = clients[0].border_width;
    if clients.len() == 1 {
        return single_client(mon, bw);
    }
    let mut rects = vec![];
    let client_width = mon.width / clients.len() as u16;
    let mut x_offset = 0;
    for _ in 0..clients.len() {
        rects.push(ClientRect::new(
            x_offset,
            0,
            client_width,
            mon.height - bw * 2,
        ));
        x_offset += (client_width - bw) as i16;
    }
    rects
}

fn single_client(mon: &Monitor, bw: u16) -> Vec<ClientRect> {
    vec![ClientRect::new(
        0,
        0,
        mon.width - bw * 2,
        mon.height - bw * 2,
    )]
}

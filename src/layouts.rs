use crate::{
    client::{ClientRect, ClientState},
    config::window::MAIN_CLIENT_WIDTH_PERCENTAGE,
    monitor::WMonitor,
};

#[derive(Default, Debug)]
pub enum WLayout {
    #[default]
    Tile,
    Column,
    Floating,
}

pub fn layout_clients(
    mon: &WMonitor,
    clients: &Vec<ClientState>,
    layout: &WLayout,
) -> Option<Vec<ClientRect>> {
    if clients.is_empty() {
        return None;
    }

    let rects = match layout {
        WLayout::Tile => tile(mon, clients),
        WLayout::Column => col(mon, clients),
        _ => todo!(),
    };

    Some(rects)
}

fn tile(mon: &WMonitor, clients: &Vec<ClientState>) -> Vec<ClientRect> {
    let bw = clients[0].border_width;
    if clients.len() == 1 {
        return single_client(mon, bw);
    }

    let main_width = mon.width_from_percentage(MAIN_CLIENT_WIDTH_PERCENTAGE);

    let mut rects = vec![ClientRect::new(
        mon.x,
        mon.y,
        main_width - bw * 2,
        mon.height - bw * 2,
    )];

    let non_main_window_count = clients.len() - 1;
    let stack_client_height = mon.height / non_main_window_count as u16;

    for i in 0..non_main_window_count {
        rects.push(ClientRect::new(
            mon.x + main_width as i16,
            mon.y + (i as u16 * stack_client_height) as i16,
            mon.width - main_width - (bw * 2),
            stack_client_height - (bw * 2),
        ));
    }

    rects
}

fn col(mon: &WMonitor, clients: &Vec<ClientState>) -> Vec<ClientRect> {
    let bw = clients[0].border_width;
    if clients.len() == 1 {
        return single_client(mon, bw);
    }
    let mut rects = vec![];
    let client_width = mon.width / clients.len() as u16;
    for i in 0..clients.len() {
        rects.push(ClientRect::new(
            mon.x + (i as i16 * client_width as i16),
            mon.y,
            mon.width - client_width - (bw * 2),
            mon.height - (bw * 2),
        ));
    }
    rects
}

fn single_client(mon: &WMonitor, bw: u16) -> Vec<ClientRect> {
    vec![ClientRect::new(
        mon.x,
        mon.y,
        mon.width - bw * 2,
        mon.height - bw * 2,
    )]
}

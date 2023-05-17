use crate::{
    client::ClientRect, config::workspaces::CLIENT_BORDER_WIDTH, monitor::WMonitor,
    util::ClientCell,
};

#[derive(Default, Debug)]
pub enum WLayout {
    #[default]
    Tile,
    Column,
    Floating,
}

pub fn layout_clients(
    layout: &WLayout,
    width_factor: f32,
    monitor: &WMonitor,
    clients: &Vec<&ClientCell>,
) -> Option<Vec<ClientRect>> {
    if clients.is_empty() {
        return None;
    }

    let rects = match layout {
        WLayout::Tile => tile(monitor, width_factor, clients),
        WLayout::Column => col(monitor, clients),
        _ => todo!(),
    };

    Some(rects)
}

fn tile(mon: &WMonitor, width_factor: f32, clients: &Vec<&ClientCell>) -> Vec<ClientRect> {
    if clients.len() == 1 {
        return single_client(mon);
    }

    let main_width = mon.width_from_percentage(width_factor);

    let mut rects = vec![];

    rects.push(ClientRect::new(
        mon.x,
        mon.y,
        main_width - CLIENT_BORDER_WIDTH * 2,
        mon.height - CLIENT_BORDER_WIDTH * 2,
    ));

    let non_main_window_count = clients.len() - 1;
    let stack_client_height = mon.height / non_main_window_count as u16;

    for (i, _) in clients.iter().skip(1).enumerate() {
        rects.push(ClientRect::new(
            mon.x + main_width as i16,
            mon.y + (i as u16 * stack_client_height) as i16,
            mon.width - main_width - (CLIENT_BORDER_WIDTH * 2),
            stack_client_height - (CLIENT_BORDER_WIDTH * 2),
        ));
    }

    rects
}

fn col(mon: &WMonitor, clients: &Vec<&ClientCell>) -> Vec<ClientRect> {
    if clients.len() == 1 {
        return single_client(mon);
    }
    let mut rects = vec![];
    let client_width = mon.width / clients.len() as u16;
    for i in 0..clients.len() {
        rects.push(ClientRect::new(
            mon.x + (i as i16 * client_width as i16),
            mon.y,
            mon.width - client_width - (CLIENT_BORDER_WIDTH * 2),
            mon.height - (CLIENT_BORDER_WIDTH * 2),
        ));
    }
    rects
}

fn single_client(mon: &WMonitor) -> Vec<ClientRect> {
    vec![ClientRect::new(
        mon.x,
        mon.y,
        mon.width - CLIENT_BORDER_WIDTH * 2,
        mon.height - CLIENT_BORDER_WIDTH * 2,
    )]
}

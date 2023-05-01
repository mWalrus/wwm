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
        _ => todo!(),
    }
}

fn tile(mon: &Monitor, clients: &Vec<ClientState>) -> Vec<ClientRect> {
    println!("Client count: {}", clients.len());
    if clients.len() == 1 {
        return vec![ClientRect::new(0, 0, mon.width, mon.height)];
    }

    let main_width = mon.width_from_percentage(MAIN_CLIENT_WIDTH_PERCENTAGE);

    let mut rects = vec![ClientRect::new(0, 0, main_width, mon.height)];

    let stack_width = mon.width - main_width;

    let non_main_window_count = clients.len() - 1;
    let stack_client_height = mon.client_height(non_main_window_count);

    for i in 0..non_main_window_count {
        rects.push(ClientRect::new(
            main_width as i16,
            (i * stack_client_height as usize) as i16,
            stack_width,
            stack_client_height,
        ))
    }

    println!("Rects: {rects:#?}");
    rects
}

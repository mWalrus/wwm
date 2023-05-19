use std::{cmp::Ordering, fmt};

use x11rb::connection::Connection;

use crate::{
    client::ClientRect,
    config::theme::window::BORDER_WIDTH,
    monitor::WMonitor,
    util::{bar_height, ClientCell},
};

#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
pub enum WLayout {
    #[default]
    Tile,
    Column,
    Floating,
}

impl fmt::Display for WLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let symbol = match self {
            WLayout::Tile => "[]=",
            WLayout::Column => "|||",
            WLayout::Floating => "   ",
        };
        write!(f, "{symbol}")
    }
}

pub fn layout_clients<C: Connection>(
    layout: &WLayout,
    width_factor: f32,
    monitor: &WMonitor<C>,
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

fn tile<C: Connection>(
    mon: &WMonitor<C>,
    width_factor: f32,
    clients: &Vec<&ClientCell>,
) -> Vec<ClientRect> {
    if clients.len() == 1 {
        return single_client(mon);
    }

    let main_width = mon.width_from_percentage(width_factor);

    let mut rects = vec![];

    rects.push(ClientRect::new(
        mon.x,
        mon.y,
        main_width - BORDER_WIDTH * 2,
        mon.height - BORDER_WIDTH * 2,
    ));

    let non_main_window_count = clients.len() - 1;
    let non_main_height = mon.height / non_main_window_count as u16;

    for (i, _) in clients.iter().skip(1).enumerate() {
        let cy = mon.y + (i as u16 * non_main_height) as i16;
        let mut ch = non_main_height;

        if i == non_main_window_count - 1 {
            let ctot = cy + ch as i16 - bar_height() as i16;
            let mtot = mon.y + mon.height as i16 - bar_height() as i16;

            match ctot.cmp(&mtot) {
                Ordering::Less => ch += ctot.abs_diff(mtot),
                Ordering::Greater => ch -= ctot.abs_diff(mtot),
                _ => {}
            }
        }

        rects.push(ClientRect::new(
            mon.x + main_width as i16,
            cy,
            mon.width - main_width - (BORDER_WIDTH * 2),
            ch - (BORDER_WIDTH * 2),
        ));
    }

    rects
}

fn col<C: Connection>(mon: &WMonitor<C>, clients: &Vec<&ClientCell>) -> Vec<ClientRect> {
    if clients.len() == 1 {
        return single_client(mon);
    }
    let mut rects = vec![];
    let client_width = mon.width / clients.len() as u16;
    for i in 0..clients.len() {
        rects.push(ClientRect::new(
            mon.x + (i as i16 * client_width as i16),
            mon.y,
            client_width - (BORDER_WIDTH * 2),
            mon.height - (BORDER_WIDTH * 2),
        ));
    }
    rects
}

fn single_client<C: Connection>(mon: &WMonitor<C>) -> Vec<ClientRect> {
    vec![ClientRect::new(
        mon.x,
        mon.y,
        mon.width - BORDER_WIDTH * 2,
        mon.height - BORDER_WIDTH * 2,
    )]
}

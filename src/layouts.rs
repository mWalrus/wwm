use crate::{config::bar_height, config::theme::window::BORDER_WIDTH};
use std::cmp::Ordering;
use wwm_core::util::{primitives::WRect, WLayout};

pub fn layout_clients(
    layout: &WLayout,
    width_factor: f32,
    monitor_rect: &WRect,
    clients: usize,
) -> Option<Vec<WRect>> {
    if clients == 0 {
        return None;
    }

    let rects = match layout {
        WLayout::MainStack => tile(monitor_rect, width_factor, clients),
        WLayout::Column => col(monitor_rect, clients),
    };

    Some(rects)
}

fn tile(monitor_rect: &WRect, width_factor: f32, clients: usize) -> Vec<WRect> {
    if clients == 1 {
        return single_client(monitor_rect);
    }

    let main_width = (monitor_rect.w as f32 * width_factor) as u16;

    let mut rects = vec![];

    rects.push(WRect::new(
        monitor_rect.x,
        monitor_rect.y,
        main_width - BORDER_WIDTH * 2,
        monitor_rect.h - BORDER_WIDTH * 2,
    ));

    let non_main_window_count = clients - 1;
    let non_main_height = monitor_rect.h / non_main_window_count as u16;

    for (i, _) in (0..clients).skip(1).enumerate() {
        let cy = monitor_rect.y + (i as u16 * non_main_height) as i16;
        let mut ch = non_main_height;

        if i == non_main_window_count - 1 {
            let ctot = cy + ch as i16 - bar_height() as i16;
            let mtot = monitor_rect.y + monitor_rect.h as i16 - bar_height() as i16;

            match ctot.cmp(&mtot) {
                Ordering::Less => ch += ctot.abs_diff(mtot),
                Ordering::Greater => ch -= ctot.abs_diff(mtot),
                _ => {}
            }
        }

        rects.push(WRect::new(
            monitor_rect.x + main_width as i16,
            cy,
            monitor_rect.w - main_width - (BORDER_WIDTH * 2),
            ch - (BORDER_WIDTH * 2),
        ));
    }

    rects
}

fn col(monitor_rect: &WRect, clients: usize) -> Vec<WRect> {
    if clients == 1 {
        return single_client(monitor_rect);
    }
    let mut rects = vec![];
    let client_width = monitor_rect.w / clients as u16;
    for i in 0..clients {
        rects.push(WRect::new(
            monitor_rect.x + (i as i16 * client_width as i16),
            monitor_rect.y,
            client_width - (BORDER_WIDTH * 2),
            monitor_rect.h - (BORDER_WIDTH * 2),
        ));
    }
    rects
}

fn single_client(monitor_rect: &WRect) -> Vec<WRect> {
    vec![WRect::new(
        monitor_rect.x,
        monitor_rect.y,
        monitor_rect.w - BORDER_WIDTH * 2,
        monitor_rect.h - BORDER_WIDTH * 2,
    )]
}

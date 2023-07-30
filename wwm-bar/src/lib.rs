use std::{
    rc::Rc,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use status_module::{WBarModMask, WBarModule};
use wwm_core::{
    text::{Text, TextRenderer},
    util::{hex_to_rgba_color, WRect},
    visual::RenderVisualInfo,
};
use x11rb::{
    connection::Connection,
    protocol::{
        render::{Color, ConnectionExt as _, CreatePictureAux, Picture, PolyEdge, PolyMode},
        xproto::{
            BackingStore, ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, Gcontext,
            LineStyle, Rectangle, Window, WindowClass,
        },
    },
};

pub mod status_module;

#[derive(Debug)]
enum Redraw {
    Tag(usize),
    LayoutSymbol,
    Title,
    Modules,
}

pub struct WBar<'b, C: Connection> {
    window: Window,
    picture: Picture,
    rect: WRect,
    text_renderer: Rc<TextRenderer<'b, C>>,
    vis_info: Rc<RenderVisualInfo>,
    tags: Vec<WBarTag>,
    layout_symbol: Text,
    title: Text,
    layout_rect: WRect,
    title_rect: WRect,
    status_width: u16,
    section_padding: i16,
    colors: WBarColors,
    redraw_queue: Arc<Mutex<Vec<Redraw>>>,
    has_client_gc: Gcontext,
    has_client_gc_selected: Gcontext,
    clear_gc: Gcontext,
    is_focused: bool,
    modules: Vec<WBarModule>,
    padding: u16,
}

struct WBarColors {
    fg: Color,
    fg_selected: Color,
    bg: Color,
    bg_selected: Color,
}

#[derive(Debug)]
pub struct WBarTag {
    id: usize,
    text: Text,
    rect: WRect,
    selected: bool,
    has_clients: bool,
}

impl WBarTag {
    fn new(id: usize, text: Text, rect: WRect, selected: bool, has_clients: bool) -> Self {
        Self {
            id,
            text,
            rect,
            selected,
            has_clients,
        }
    }
}

impl<'b, C: Connection> WBar<'b, C> {
    pub fn new(
        conn: &C,
        text_renderer: Rc<TextRenderer<'b, C>>,
        vis_info: Rc<RenderVisualInfo>,
        rect: impl Into<WRect>,
        padding: u16,
        section_padding: i16,
        taglen: usize,
        layout_symbol: impl ToString,
        title: impl ToString,
        colors: [u32; 4],
        mod_mask: WBarModMask,
        status_interval: u64,
    ) -> Self {
        let rect = rect.into();

        let bar_win = conn.generate_id().unwrap();
        conn.create_window(
            vis_info.root.depth,
            bar_win,
            vis_info.screen_root,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .background_pixel(colors[1])
                .event_mask(EventMask::BUTTON_PRESS)
                .backing_store(BackingStore::WHEN_MAPPED)
                .override_redirect(1),
        )
        .unwrap();

        let fg = colors[0];
        let bg = colors[1];
        let fg_selected = colors[3];

        let colors = WBarColors {
            fg: hex_to_rgba_color(colors[0]),
            bg: hex_to_rgba_color(colors[1]),
            bg_selected: hex_to_rgba_color(colors[2]),
            fg_selected: hex_to_rgba_color(colors[3]),
        };

        let has_client_gc = conn.generate_id().unwrap();
        let has_client_gc_selected = conn.generate_id().unwrap();
        let clear_gc = conn.generate_id().unwrap();

        conn.create_gc(
            has_client_gc,
            bar_win,
            &CreateGCAux::new()
                .foreground(fg)
                .line_width(1)
                .line_style(LineStyle::SOLID),
        )
        .unwrap();

        conn.create_gc(
            has_client_gc_selected,
            bar_win,
            &CreateGCAux::new()
                .foreground(fg_selected)
                .line_width(1)
                .line_style(LineStyle::SOLID),
        )
        .unwrap();

        conn.create_gc(
            clear_gc,
            bar_win,
            &CreateGCAux::new()
                .foreground(bg)
                .line_width(1)
                .line_style(LineStyle::SOLID),
        )
        .unwrap();

        let mut tags = Vec::with_capacity(taglen);
        let mut x_offset = 0;
        for i in 0..taglen {
            let text = Text::new(
                conn,
                &text_renderer,
                &vis_info,
                i + 1,
                bar_win,
                padding,
                padding * 2,
            );

            let tag_rect = WRect::new(x_offset, rect.y, text.box_width as u16, rect.h);
            x_offset += text.box_width as i16;

            tags.push(WBarTag::new(i, text, tag_rect, i == 0, false));
        }

        let picture = conn.generate_id().unwrap();
        conn.render_create_picture(
            picture,
            bar_win,
            vis_info.root.pict_format,
            &CreatePictureAux::new()
                .polyedge(PolyEdge::SMOOTH)
                .polymode(PolyMode::IMPRECISE),
        )
        .unwrap();
        let layout_symbol = Text::new(
            conn,
            &text_renderer,
            &vis_info,
            layout_symbol,
            bar_win,
            padding,
            padding,
        );
        let title = Text::new(
            conn,
            &text_renderer,
            &vis_info,
            title,
            bar_win,
            padding,
            padding,
        );

        let layout_rect = WRect::new(
            x_offset + section_padding as i16,
            0,
            layout_symbol.box_width,
            rect.h,
        );
        let title_rect = WRect::new(
            x_offset + layout_rect.w as i16 + section_padding,
            0,
            rect.w - (x_offset + layout_rect.w as i16 + section_padding) as u16,
            rect.h,
        );
        conn.map_window(bar_win).unwrap();

        let mut bar = Self {
            window: bar_win,
            picture,
            rect,
            tags,
            vis_info,
            text_renderer,
            layout_symbol,
            title,
            layout_rect,
            title_rect,
            status_width: 0,
            section_padding,
            colors,
            redraw_queue: Arc::new(Mutex::new(vec![
                Redraw::Tag(0),
                Redraw::Tag(1),
                Redraw::Tag(2),
                Redraw::Tag(3),
                Redraw::Tag(4),
                Redraw::Tag(5),
                Redraw::Tag(6),
                Redraw::Tag(7),
                Redraw::Tag(8),
                Redraw::LayoutSymbol,
                Redraw::Title,
                Redraw::Modules,
            ])),
            has_client_gc,
            has_client_gc_selected,
            clear_gc,
            is_focused: false,
            modules: Self::init_modules(mod_mask),
            padding,
        };
        bar.run_status_loop(status_interval);
        bar
    }

    fn init_modules(mod_mask: WBarModMask) -> Vec<WBarModule> {
        let mut modules = vec![];

        if mod_mask & WBarModMask::VOL {
            modules.push(WBarModule::vol());
        }
        if mod_mask & WBarModMask::RAM {
            modules.push(WBarModule::ram());
        }
        if mod_mask & WBarModMask::CPU {
            modules.push(WBarModule::cpu());
        }
        if mod_mask & WBarModMask::DATE {
            modules.push(WBarModule::date());
        }
        if mod_mask & WBarModMask::VOL {
            modules.push(WBarModule::time());
        }

        modules
    }

    fn run_status_loop(&mut self, interval: u64) {
        let queue = Arc::clone(&self.redraw_queue);
        thread::spawn(move || loop {
            if let Ok(mut queue) = queue.lock() {
                queue.push(Redraw::Modules);
            }
            thread::sleep(Duration::from_millis(interval))
        });
    }

    pub fn has_pointer(&self, px: i16, py: i16) -> bool {
        self.rect.has_pointer(px, py)
    }

    pub fn select_tag_at_pos(&mut self, x: i16, y: i16) -> Option<usize> {
        if y > self.rect.y + self.rect.h as i16 {
            return None;
        }

        let mut tag_idx = None;
        for (i, t) in self.tags.iter_mut().enumerate() {
            if t.rect.has_pointer(x, y) {
                tag_idx = Some(i);
                break;
            }
        }
        tag_idx
    }

    pub fn update_layout_symbol(&mut self, conn: &C, layout_symbol: impl ToString) {
        self.layout_symbol = Text::new(
            conn,
            &self.text_renderer,
            &self.vis_info,
            layout_symbol,
            self.window,
            self.layout_symbol.vertical_padding,
            self.layout_symbol.horizontal_padding,
        );
        if let Ok(mut queue) = self.redraw_queue.lock() {
            queue.push(Redraw::Title);
            queue.push(Redraw::LayoutSymbol);
        }
    }

    pub fn update_title(&mut self, conn: &C, title: impl ToString) {
        self.title = Text::new(
            conn,
            &self.text_renderer,
            &self.vis_info,
            title,
            self.window,
            self.title.vertical_padding,
            self.title.horizontal_padding,
        );

        // FIXME: we need a cleaner solution for this
        let new_x = self.layout_rect.x + self.layout_rect.w as i16 + self.section_padding;
        self.title_rect = WRect::new(
            new_x,
            self.title_rect.y,
            self.title_rect.w - self.section_padding as u16,
            self.rect.h,
        );

        if let Ok(mut queue) = self.redraw_queue.lock() {
            queue.push(Redraw::Title);
        }
    }

    pub fn update_tags(&mut self, selected: usize) {
        if let Ok(mut queue) = self.redraw_queue.lock() {
            for (i, tag) in self.tags.iter_mut().enumerate() {
                if tag.id == selected {
                    tag.selected = true;
                    queue.push(Redraw::Tag(i));
                } else if tag.id != selected && tag.selected {
                    tag.selected = false;
                    queue.push(Redraw::Tag(i));
                }
            }
        }
    }

    pub fn set_is_focused(&mut self, is_focused: bool) {
        self.is_focused = is_focused;
        // queue redrawing of the newly focused bar
        // because we want to fill the focused tags client indicator
        // rectangle
        let idx = self.tags.iter().position(|t| t.selected).unwrap();
        if let Ok(mut queue) = self.redraw_queue.lock() {
            queue.push(Redraw::Tag(idx));
        }
    }

    pub fn set_has_clients(&mut self, tag_idx: usize, has_clients: bool) {
        if let Ok(mut queue) = self.redraw_queue.lock() {
            let tag = &mut self.tags[tag_idx];
            if tag.has_clients != has_clients {
                queue.push(Redraw::Tag(tag_idx))
            }
            tag.has_clients = has_clients;
        }
    }

    pub fn draw(&mut self, conn: &C) {
        if let Ok(mut queue) = self.redraw_queue.lock() {
            if queue.is_empty() {
                return;
            }

            for redraw_item in queue.drain(..) {
                match redraw_item {
                    Redraw::Tag(i) => {
                        let tag = &self.tags[i];
                        let (fg, bg) = if tag.selected {
                            (self.colors.fg_selected, self.colors.bg_selected)
                        } else {
                            (self.colors.fg, self.colors.bg)
                        };
                        self.text_renderer
                            .draw(tag.rect, &tag.text, self.picture, bg, fg)
                            .unwrap();

                        let client_rect: Rectangle =
                            WRect::new(tag.rect.x + 1, tag.rect.y + 1, 3, 3).into();
                        let client_rect_fill: Rectangle =
                            WRect::new(tag.rect.x + 1, tag.rect.y + 1, 4, 4).into();

                        if !tag.has_clients {
                            continue;
                        }

                        if tag.selected && self.is_focused {
                            conn.poly_fill_rectangle(
                                self.window,
                                self.has_client_gc_selected,
                                &[client_rect_fill],
                            )
                            .unwrap();
                        } else if tag.selected && !self.is_focused {
                            conn.poly_rectangle(
                                self.window,
                                self.has_client_gc_selected,
                                &[client_rect],
                            )
                            .unwrap();
                        } else if !tag.selected {
                            conn.poly_rectangle(self.window, self.has_client_gc, &[client_rect])
                                .unwrap();
                        }
                    }
                    Redraw::LayoutSymbol => {
                        self.text_renderer
                            .draw(
                                self.layout_rect,
                                &self.layout_symbol,
                                self.picture,
                                self.colors.bg,
                                self.colors.fg,
                            )
                            .unwrap();
                    }
                    Redraw::Title => {
                        self.text_renderer
                            .draw(
                                self.title_rect,
                                &self.title,
                                self.picture,
                                self.colors.bg,
                                self.colors.fg,
                            )
                            .unwrap();
                    }
                    Redraw::Modules => {
                        let mut strings = vec![];
                        for module in self.modules.iter() {
                            strings.push(module.0.update());
                        }

                        let text = Text::new(
                            conn,
                            &self.text_renderer,
                            &self.vis_info,
                            strings.join(" | "),
                            self.window,
                            self.padding,
                            self.padding,
                        );

                        let mut rect = WRect::new(
                            (self.rect.w - self.status_width) as i16,
                            0,
                            self.status_width,
                            self.rect.h,
                        );

                        if text.box_width < self.status_width {
                            // clear previous status section size
                            // otherwise, if the current text size is smaller,
                            // there will be remnants of the previous update's text
                            // in the bar.
                            conn.poly_fill_rectangle(self.window, self.clear_gc, &[rect.into()])
                                .unwrap();
                        }

                        rect.x = (self.rect.w - text.box_width) as i16;
                        rect.w = text.box_width;
                        self.status_width = text.box_width;

                        self.title_rect.w = self.title_rect.x.abs_diff(rect.x);
                        self.text_renderer
                            .draw(rect, &text, self.picture, self.colors.bg, self.colors.fg)
                            .unwrap();
                    }
                }
            }
            conn.flush().unwrap();
        }
    }
}

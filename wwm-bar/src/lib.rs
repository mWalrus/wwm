use std::{
    rc::Rc,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use status_module::{WBarModMask, WBarModule};
use wwm_core::{
    text::TextRenderer,
    util::{bar::WBarOptions, primitives::WRect, WLayout},
};
use x11rb::{
    connection::Connection,
    protocol::{
        render::{ConnectionExt as _, CreatePictureAux, Picture, PolyEdge, PolyMode},
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
    text_renderer: Rc<TextRenderer<'b, C>>,
    bar_options: WBarOptions,
    tags: Vec<WBarTag>,
    layout_symbol: WLayout,
    title: String,
    layout_rect: WRect,
    title_rect: WRect,
    status_width: u16,
    redraw_queue: Arc<Mutex<Vec<Redraw>>>,
    has_client_gc: Gcontext,
    has_client_gc_selected: Gcontext,
    clear_gc: Gcontext,
    is_focused: bool,
    modules: Vec<WBarModule>,
}

#[derive(Debug)]
pub struct WBarTag {
    id: usize,
    text: String,
    rect: WRect,
    selected: bool,
    has_clients: bool,
}

impl WBarTag {
    fn new(id: usize, text: impl ToString, rect: WRect, selected: bool, has_clients: bool) -> Self {
        Self {
            id,
            text: text.to_string(),
            rect,
            selected,
            has_clients,
        }
    }
}

impl<'b, C: Connection> WBar<'b, C> {
    // FIXME: properly handle/propagate errors
    pub fn new(
        conn: &C,
        text_renderer: Rc<TextRenderer<'b, C>>,
        bar_options: WBarOptions,
        mod_mask: WBarModMask,
        status_interval: u64,
    ) -> Self {
        let layout_symbol = WLayout::MainStack;
        let bar_win = conn.generate_id().unwrap();
        conn.create_window(
            text_renderer.visual_info.root.depth,
            bar_win,
            text_renderer.visual_info.screen_root,
            bar_options.rect.x,
            bar_options.rect.y,
            bar_options.rect.w,
            bar_options.rect.h,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .background_pixel(bar_options.colors.bg.0)
                .event_mask(EventMask::BUTTON_PRESS)
                .backing_store(BackingStore::WHEN_MAPPED)
                .override_redirect(1),
        )
        .unwrap();

        let has_client_gc = conn.generate_id().unwrap();
        let has_client_gc_selected = conn.generate_id().unwrap();
        let clear_gc = conn.generate_id().unwrap();

        conn.create_gc(
            has_client_gc,
            bar_win,
            &CreateGCAux::new()
                .foreground(bar_options.colors.fg.0)
                .line_width(1)
                .line_style(LineStyle::SOLID),
        )
        .unwrap();

        conn.create_gc(
            has_client_gc_selected,
            bar_win,
            &CreateGCAux::new()
                .foreground(bar_options.colors.selected_fg.0)
                .line_width(1)
                .line_style(LineStyle::SOLID),
        )
        .unwrap();

        conn.create_gc(
            clear_gc,
            bar_win,
            &CreateGCAux::new()
                .foreground(bar_options.colors.bg.0)
                .line_width(1)
                .line_style(LineStyle::SOLID),
        )
        .unwrap();

        let picture = conn.generate_id().unwrap();
        conn.render_create_picture(
            picture,
            bar_win,
            text_renderer.visual_info.root.pict_format,
            &CreatePictureAux::new()
                .polyedge(PolyEdge::SMOOTH)
                .polymode(PolyMode::IMPRECISE),
        )
        .unwrap();

        let mut x_offset = 0;

        let tags = Self::init_tags(bar_options, &mut x_offset);

        x_offset += bar_options.section_padding as i16;

        let layout_symbol_width =
            text_renderer.text_width(layout_symbol) + (bar_options.padding * 2);

        let layout_rect = WRect::new(x_offset, 0, layout_symbol_width, bar_options.rect.h);

        x_offset += layout_symbol_width as i16;

        let title_rect = WRect::new(
            x_offset,
            0,
            bar_options.rect.w - x_offset as u16,
            bar_options.rect.h,
        );

        conn.map_window(bar_win).unwrap();

        let mut bar = Self {
            window: bar_win,
            picture,
            tags,
            text_renderer,
            bar_options,
            layout_symbol,
            layout_rect,
            title: String::new(),
            title_rect,
            status_width: 0,
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
        };
        bar.run_status_loop(status_interval);
        bar
    }

    fn init_tags(bar_options: WBarOptions, x_offset: &mut i16) -> Vec<WBarTag> {
        let mut tags = Vec::with_capacity(bar_options.tag_count);
        for i in 0..bar_options.tag_count {
            let text = i + 1;
            let tag_rect = WRect::new(
                *x_offset,
                bar_options.rect.y,
                bar_options.tag_width, // create a square with the side == bar height
                bar_options.rect.h,
            );
            *x_offset += bar_options.tag_width as i16;

            tags.push(WBarTag::new(i, text, tag_rect, i == 0, false));
        }
        tags
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
        self.bar_options.rect.has_pointer(px, py)
    }

    pub fn select_tag_at_pos(&mut self, x: i16, y: i16) -> Option<usize> {
        if y > self.bar_options.rect.y + self.bar_options.rect.h as i16 {
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

    pub fn update_layout_symbol(&mut self, layout_symbol: WLayout) {
        self.layout_symbol = layout_symbol;

        // update the width of the layout symbol rect
        self.layout_rect.w = self.text_renderer.text_width(layout_symbol);

        if let Ok(mut queue) = self.redraw_queue.lock() {
            queue.push(Redraw::Title);
            queue.push(Redraw::LayoutSymbol);
        }
    }

    pub fn update_title(&mut self, title: impl ToString) {
        self.title = title.to_string();

        // FIXME: we need a cleaner solution for this
        let new_x =
            self.layout_rect.x + self.layout_rect.w as i16 + self.bar_options.section_padding;

        self.title_rect.x = new_x;

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
                            (
                                self.bar_options.colors.selected_fg.1,
                                self.bar_options.colors.selected_bg.1,
                            )
                        } else {
                            (self.bar_options.colors.fg.1, self.bar_options.colors.bg.1)
                        };
                        self.text_renderer
                            .draw(
                                tag.rect,
                                &tag.text,
                                self.bar_options.padding,
                                self.picture,
                                self.window,
                                bg,
                                fg,
                                true,
                            )
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
                                &self.layout_symbol.to_string(),
                                self.bar_options.padding,
                                self.picture,
                                self.window,
                                self.bar_options.colors.bg.1,
                                self.bar_options.colors.fg.1,
                                false,
                            )
                            .unwrap();
                    }
                    Redraw::Title => {
                        self.text_renderer
                            .draw(
                                self.title_rect,
                                &self.title,
                                self.bar_options.padding,
                                self.picture,
                                self.window,
                                self.bar_options.colors.bg.1,
                                self.bar_options.colors.fg.1,
                                false,
                            )
                            .unwrap();
                    }
                    Redraw::Modules => {
                        let mut strings = vec![];
                        for module in self.modules.iter() {
                            strings.push(module.0.update());
                        }

                        let text = strings.join(" | ");

                        let new_status_width = self.text_renderer.text_width(&text);

                        let mut rect = WRect::new(
                            (self.bar_options.rect.w - self.status_width) as i16,
                            0,
                            self.status_width,
                            self.bar_options.rect.h,
                        );

                        if new_status_width < self.status_width {
                            // clear previous status section size
                            // otherwise, if the current text size is smaller,
                            // there will be remnants of the previous update's text
                            // in the bar.
                            conn.poly_fill_rectangle(self.window, self.clear_gc, &[rect.into()])
                                .unwrap();
                        }

                        rect.x = (self.bar_options.rect.w
                            - new_status_width
                            - self.bar_options.section_padding as u16)
                            as i16;
                        rect.w = new_status_width;
                        self.status_width = new_status_width;

                        self.title_rect.w = self.title_rect.x.abs_diff(rect.x);
                        self.text_renderer
                            .draw(
                                rect,
                                &text,
                                self.bar_options.padding,
                                self.picture,
                                self.window,
                                self.bar_options.colors.bg.1,
                                self.bar_options.colors.fg.1,
                                false,
                            )
                            .unwrap();
                    }
                }
            }
            conn.flush().unwrap();
        }
    }
}

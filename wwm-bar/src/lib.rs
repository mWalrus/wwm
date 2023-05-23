use std::rc::Rc;

use font::{FontDrawer, RenderString};
use util::{hex_to_rgba_color, Rect};
use visual::RenderVisualInfo;
use x11rb::{
    connection::Connection,
    protocol::{
        render::{Color, ConnectionExt as _, CreatePictureAux, Picture, PolyEdge, PolyMode},
        xproto::{BackingStore, ConnectionExt, CreateWindowAux, EventMask, Window, WindowClass},
    },
};

pub mod font;
mod util;
pub mod visual;

const SECTION_PADDING: i16 = 10;

#[derive(Debug)]
enum Redraw {
    Tag(usize),
    LayoutSymbol,
    Title,
}

pub struct WBar {
    window: Window,
    picture: Picture,
    rect: Rect,
    font_drawer: Rc<FontDrawer>,
    vis_info: Rc<RenderVisualInfo>,
    tags: Vec<WWorkspaceTag>,
    layout_symbol: RenderString,
    title: RenderString,
    section_dims: [Rect; 3],
    colors: WBarColors,
    redraw_queue: Vec<Redraw>,
}

struct WBarColors {
    fg: Color,
    fg_selected: Color,
    bg: Color,
    bg_selected: Color,
}

pub struct WWorkspaceTag {
    id: usize,
    text: RenderString,
    rect: Rect,
    selected: bool,
}

impl WWorkspaceTag {
    fn new(id: usize, text: RenderString, rect: Rect, selected: bool) -> Self {
        Self {
            id,
            text,
            rect,
            selected,
        }
    }
}

impl WBar {
    pub fn new<C: Connection>(
        conn: &C,
        font_drawer: Rc<FontDrawer>,
        vis_info: Rc<RenderVisualInfo>,
        rect: impl Into<Rect>,
        padding: u16,
        taglen: usize,
        layout_symbol: impl ToString,
        title: impl ToString,
        colors: [u32; 4],
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

        let colors = WBarColors {
            fg: hex_to_rgba_color(colors[0]),
            bg: hex_to_rgba_color(colors[1]),
            bg_selected: hex_to_rgba_color(colors[2]),
            fg_selected: hex_to_rgba_color(colors[3]),
        };

        let mut tags = Vec::with_capacity(taglen);
        let mut x_offset = 0;
        for i in 0..taglen {
            let text = RenderString::new(
                conn,
                &font_drawer,
                &vis_info,
                i + 1,
                bar_win,
                padding,
                padding * 2,
            );

            let tag_rect = Rect::new(x_offset, rect.y, text.box_width as u16, rect.h);
            x_offset += text.box_width as i16;

            tags.push(WWorkspaceTag::new(i, text, tag_rect, i == 0));
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
        let layout_symbol = RenderString::new(
            conn,
            &font_drawer,
            &vis_info,
            layout_symbol,
            bar_win,
            padding,
            padding,
        );
        let title = RenderString::new(
            conn,
            &font_drawer,
            &vis_info,
            title,
            bar_win,
            padding,
            padding,
        );

        let section_dims = [
            Rect::new(0, 0, x_offset as u16, rect.h),
            Rect::new(
                x_offset + SECTION_PADDING,
                0,
                layout_symbol.box_width,
                rect.h,
            ),
            Rect::new(
                x_offset + layout_symbol.box_width as i16 + SECTION_PADDING,
                0,
                rect.w - (x_offset + layout_symbol.box_width as i16 + SECTION_PADDING) as u16,
                rect.h,
            ),
        ];

        conn.map_window(bar_win).unwrap();

        Self {
            window: bar_win,
            picture,
            rect,
            tags,
            vis_info,
            font_drawer,
            layout_symbol,
            title,
            section_dims,
            colors,
            redraw_queue: vec![
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
            ],
        }
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

    pub fn update_layout_symbol<C: Connection>(&mut self, conn: &C, layout_symbol: impl ToString) {
        self.layout_symbol = RenderString::new(
            conn,
            &self.font_drawer,
            &self.vis_info,
            layout_symbol,
            self.window,
            self.layout_symbol.vertical_padding,
            self.layout_symbol.horizontal_padding,
        );
        self.redraw_queue.push(Redraw::LayoutSymbol);
    }

    pub fn update_title<C: Connection>(&mut self, conn: &C, title: impl ToString) {
        self.title = RenderString::new(
            conn,
            &self.font_drawer,
            &self.vis_info,
            title,
            self.window,
            self.title.vertical_padding,
            self.title.horizontal_padding,
        );

        // FIXME: we need a cleaner solution for this
        let left_rect = &self.section_dims[1];
        let new_x = left_rect.x + left_rect.w as i16 + SECTION_PADDING;
        self.section_dims[2] =
            Rect::new(new_x, left_rect.y, self.rect.w - new_x as u16, self.rect.h);

        self.redraw_queue.push(Redraw::Title);
    }

    pub fn update_tags(&mut self, selected: usize) {
        for (i, tag) in self.tags.iter_mut().enumerate() {
            if tag.id == selected {
                tag.selected = true;
                self.redraw_queue.push(Redraw::Tag(i));
            } else if tag.id != selected && tag.selected {
                tag.selected = false;
                self.redraw_queue.push(Redraw::Tag(i));
            }
        }
    }

    pub fn draw<C: Connection>(&mut self, conn: &C) {
        if self.redraw_queue.is_empty() {
            return;
        }

        for redraw_item in self.redraw_queue.drain(..) {
            match redraw_item {
                Redraw::Tag(i) => {
                    let tag = &self.tags[i];
                    let (fg, bg) = if tag.selected {
                        (self.colors.fg_selected, self.colors.bg_selected)
                    } else {
                        (self.colors.fg, self.colors.bg)
                    };
                    self.font_drawer
                        .draw(conn, tag.rect, &tag.text, self.picture, bg, fg)
                        .unwrap();
                }
                Redraw::LayoutSymbol => {
                    self.font_drawer
                        .draw(
                            conn,
                            self.section_dims[1],
                            &self.layout_symbol,
                            self.picture,
                            self.colors.bg,
                            self.colors.fg,
                        )
                        .unwrap();
                }
                Redraw::Title => {
                    self.font_drawer
                        .draw(
                            conn,
                            self.section_dims[2],
                            &self.title,
                            self.picture,
                            self.colors.bg,
                            self.colors.fg,
                        )
                        .unwrap();
                }
            }
        }
        conn.flush().unwrap();
    }
}

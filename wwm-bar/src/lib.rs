use std::rc::Rc;

use font::{FontDrawer, RenderString};
use util::Rect;
use visual::RenderVisualInfo;
use x11rb::{
    connection::Connection,
    protocol::{
        render::{
            Color, ConnectionExt as _, CreatePictureAux, Picture, PolyEdge, PolyMode, Repeat,
        },
        xproto::{
            ChangeWindowAttributesAux, ConfigureWindowAux, ConnectionExt, CreateWindowAux,
            PropMode, Rectangle, StackMode, Window, WindowClass,
        },
    },
};

pub mod font;
mod util;
pub mod visual;

pub struct WBar {
    window: Window,
    picture: Picture,
    rect: Rect,
    font_drawer: Rc<FontDrawer>,
    tags: Vec<WWorkspaceTag>,
    layout_symbol: RenderString,
    title: RenderString,
    colors: WBarColors,
    tag_changes: Vec<usize>,
}

struct WBarColors {
    text: u32,
    bg: u32,
    bg_selected: u32,
}

pub struct WWorkspaceTag {
    id: usize,
    text: RenderString,
    window: Window,
    picture: Picture,
    selected: bool,
    rect: Rect,
}

impl WWorkspaceTag {
    fn new(
        id: usize,
        window: Window,
        picture: Picture,
        rect: Rect,
        text: RenderString,
        selected: bool,
    ) -> Self {
        Self {
            id,
            text,
            window,
            picture,
            rect,
            selected,
        }
    }
}

impl WBar {
    pub fn new<C: Connection>(
        conn: &C,
        font_drawer: Rc<FontDrawer>,
        vis_info: &RenderVisualInfo,
        rect: impl Into<Rect>,
        padding: u16,
        taglen: usize,
        layout_symbol: impl ToString,
        title: impl ToString,
        colors: [u32; 3],
    ) -> Self {
        let rect = rect.into();

        let colors = WBarColors {
            text: colors[0],
            bg: colors[1],
            bg_selected: colors[2],
        };

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
                .background_pixel(colors.bg)
                .override_redirect(1),
        )
        .unwrap();

        let mut tags = Vec::with_capacity(taglen);
        for i in 0..taglen {
            let tag_win = conn.generate_id().unwrap();
            let tag_pic = conn.generate_id().unwrap();
            let tag_pm = conn.generate_id().unwrap();
            let text = RenderString::new(&font_drawer, i + 1).pad(padding);

            let (w, h) = text.box_dimensions();
            let tag_rect = Rect::new(i as i16 * w as i16 + (i as i16 * 5), 0, w, h);
            // conn.create_window(
            //     vis_info.root.depth,
            //     tag_win,
            //     bar_win,
            //     tag_rect.x,
            //     tag_rect.y,
            //     tag_rect.w,
            //     tag_rect.h,
            //     0,
            //     WindowClass::INPUT_OUTPUT,
            //     0,
            //     &CreateWindowAux::new()
            //         // .background_pixel(0x626880) // FIXME: change
            //         .override_redirect(1),
            // )
            // .unwrap();

            conn.create_pixmap(vis_info.root.depth, tag_pm, bar_win, tag_rect.w, tag_rect.h)
                .unwrap();

            conn.render_create_picture(
                tag_pic,
                tag_pm,
                vis_info.root.pict_format,
                &CreatePictureAux::new().repeat(Repeat::NORMAL),
            )
            .unwrap();

            // conn.configure_window(
            //     tag_win,
            //     &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            // )
            // .unwrap();

            tags.push(WWorkspaceTag::new(
                i,
                tag_win,
                tag_pic,
                tag_rect,
                text,
                i == 0,
            ));
        }

        conn.map_window(bar_win).unwrap();
        for tag in &tags {
            conn.map_window(tag.window).unwrap();
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

        Self {
            window: bar_win,
            picture,
            rect,
            tags,
            layout_symbol: RenderString::new(&font_drawer, layout_symbol).pad(padding),
            title: RenderString::new(&font_drawer, title).pad(padding),
            font_drawer,
            colors,
            tag_changes: Vec::with_capacity(taglen),
        }
    }

    pub fn update<C: Connection>(
        &mut self,
        conn: &C,
        workspace_index: usize,
        layout_symbol: impl ToString,
        title: impl ToString,
    ) {
        self.title = RenderString::new(&self.font_drawer, title);
        self.layout_symbol = RenderString::new(&self.font_drawer, layout_symbol);

        // keep track of changes so we can re-render only these
        for (i, tag) in self.tags.iter_mut().enumerate() {
            if tag.id == workspace_index {
                tag.selected = true;
                self.tag_changes.push(i);
                let aux = ChangeWindowAttributesAux::new().border_pixel(self.colors.bg_selected);
                conn.change_window_attributes(tag.window, &aux).unwrap();
            } else if tag.id != workspace_index && tag.selected {
                tag.selected = false;
                self.tag_changes.push(i);
                let aux = ChangeWindowAttributesAux::new().border_pixel(self.colors.bg);
                conn.change_window_attributes(tag.window, &aux).unwrap();
            }
        }
    }

    pub fn draw<C: Connection>(&self, conn: &C) {
        for tag in self.tags.iter() {
            self.font_drawer.draw(conn, &tag, self.picture).unwrap();
        }
        // self.font_drawer
        //     .draw(
        //         conn,
        //         self.picture,
        //         self.picture,
        //         &self.layout_symbol,
        //         Color {
        //             red: 0xffff,
        //             green: 0xffff,
        //             blue: 0xffff,
        //             alpha: 0xffff,
        //         },
        //     )
        //     .unwrap();
        // TODO: draw text
    }
}

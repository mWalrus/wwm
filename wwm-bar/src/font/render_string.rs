use x11rb::{
    connection::Connection,
    protocol::{
        render::{ConnectionExt as _, CreatePictureAux, Picture, Repeat},
        xproto::{ConnectionExt, Pixmap},
    },
};

use crate::visual::RenderVisualInfo;

use super::{loader::FontEncodedChunk, FontDrawer};

#[derive(Debug, Clone)]
pub struct RenderString {
    pub chunks: Vec<FontEncodedChunk>,
    pub picture: Picture,
    pub pixmap: Pixmap,
    pub text_width: i16,
    pub text_height: u16,
    pub box_width: u16,
    pub box_height: u16,
    pub vertical_padding: u16,
    pub horizontal_padding: u16,
}

impl RenderString {
    pub fn new<C: Connection>(
        conn: &C,
        drawer: &FontDrawer,
        vis_info: &RenderVisualInfo,
        text: impl ToString,
        parent: u32,
        vertical_padding: u16,
        horizontal_padding: u16,
    ) -> Self {
        let text = text.to_string();
        let (text_width, text_height) = drawer.font.geometry(&text);
        let chunks = drawer.font.encode(&text, text_width - 1);

        let box_width = text_width as u16 + (horizontal_padding * 2);
        let box_height = text_height as u16 + (vertical_padding * 2);

        let picture = conn.generate_id().unwrap();
        let pixmap = conn.generate_id().unwrap();

        conn.create_pixmap(vis_info.root.depth, pixmap, parent, box_width, box_height)
            .unwrap();

        conn.render_create_picture(
            picture,
            pixmap,
            vis_info.root.pict_format,
            &CreatePictureAux::new().repeat(Repeat::NORMAL),
        )
        .unwrap();

        Self {
            chunks,
            text_width,
            text_height,
            picture,
            pixmap,
            box_width,
            box_height,
            vertical_padding,
            horizontal_padding,
        }
    }
}

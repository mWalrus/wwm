pub mod loader;
pub mod render_string;

use loader::X11Font;
pub use render_string::RenderString;
use thiserror::Error;
use x11rb::{
    connection::Connection,
    protocol::{
        render::{Color, ConnectionExt, Glyphset, PictOp, Picture},
        xproto::Rectangle,
    },
    xcb_ffi::ConnectionError,
};

use crate::util::Rect;

#[derive(Error, Debug)]
pub enum DrawerError {
    #[error("connection error: {0:#?}")]
    Connection(#[from] ConnectionError),
}

pub struct FontDrawer {
    font: X11Font,
}

impl FontDrawer {
    pub fn new(font: X11Font) -> Self {
        Self { font }
    }

    pub fn draw<C: Connection>(
        &self,
        conn: &C,
        rect: Rect,
        text: &RenderString,
        dst: Picture,
        bg: Color,
        fg: Color,
    ) -> Result<(), DrawerError> {
        let Rect { x, y, w, h } = rect;
        let fg_fill_area: Rectangle = Rect::new(0, y, w, h).into();
        let bg_fill_area: Rectangle = rect.into();

        conn.render_fill_rectangles(PictOp::SRC, text.picture, fg, &[fg_fill_area])?;
        conn.render_fill_rectangles(PictOp::SRC, dst, bg, &[bg_fill_area])?;

        let mut x_offset = x + text.horizontal_padding as i16;
        for chunk in &text.chunks {
            self.draw_glyphs(
                conn,
                (x_offset, y),
                chunk.glyph_set,
                text.picture,
                dst,
                &chunk.glyph_ids,
            )?;

            x_offset += chunk.width;
        }

        Ok(())
    }

    fn draw_glyphs<C: Connection>(
        &self,
        conn: &C,
        (x, y): (i16, i16),
        glyphs: Glyphset,
        src: Picture,
        dst: Picture,
        glyph_ids: &[u32],
    ) -> Result<(), DrawerError> {
        let mut buf = Vec::with_capacity(glyph_ids.len());
        let render = if glyph_ids.len() > 254 {
            &glyph_ids[..254]
        } else {
            glyph_ids
        };

        buf.extend_from_slice(&[render.len() as u8, 0, 0, 0]);

        buf.extend_from_slice(&(x).to_ne_bytes());
        buf.extend_from_slice(&(y).to_ne_bytes());

        for glyph in render {
            buf.extend_from_slice(&(glyph).to_ne_bytes());
        }

        conn.render_composite_glyphs16(PictOp::OVER, src, dst, 0, glyphs, 0, 0, &buf)?;
        Ok(())
    }
}

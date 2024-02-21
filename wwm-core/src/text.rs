use font_loader::system_fonts as fonts;
use fontdue::{Font as FontData, FontSettings, Metrics};
use smallmap::Map;
use thiserror::Error;
use x11rb::{
    connection::Connection,
    protocol::{
        render::{
            Color, ConnectionExt as _, CreatePictureAux, Glyphinfo, Glyphset, PictOp, Picture,
            Repeat,
        },
        xproto::{ConnectionExt, Rectangle, Screen, Window},
    },
    rust_connection::{ConnectionError, ReplyOrIdError},
};

use crate::{util::WRect, visual::VisualError};

use crate::visual::RenderVisualInfo;

#[derive(Error, Debug)]
pub enum FontError {
    #[error("Failed to load font data: {0}")]
    LoadFromBytes(&'static str),
    #[error("Could not find font: {0}")]
    NotFound(&'static str),
    #[error("Connection error: {0:#?}")]
    Connection(#[from] ConnectionError),
    #[error("Reply or ID error: {0:?}")]
    ReplyOrIdError(#[from] ReplyOrIdError),
    #[error("Visual info error {0:?}")]
    Visual(#[from] VisualError),
}

pub struct TextRenderer<'a, C: Connection> {
    conn: &'a C,
    pub gsid: Glyphset,
    char_map: Map<char, CharInfo>,
    pub font_height: i16,
    pub visual_info: RenderVisualInfo,
}

pub struct CharInfo {
    pub glyph_id: u32,
    pub horizontal_space: i16,
    pub height: u16,
}

#[derive(Debug, Clone)]
pub struct FontEncodedChunk {
    pub width: i16,
    pub font_height: i16,
    glyph_set: Glyphset,
    glyph_ids: Vec<u32>,
}

type RasterizationData = Vec<(char, Metrics, Vec<u8>)>;
type CharMapData = (Vec<u32>, Vec<Glyphinfo>, Vec<u8>, Map<char, CharInfo>);

impl<'a, C: Connection> TextRenderer<'a, C> {
    pub fn new(
        conn: &'a C,
        screen: &Screen,
        font_family: &'static str,
        font_size: f32,
    ) -> Result<Self, FontError> {
        let visual_info = RenderVisualInfo::new(conn, screen)?;
        let gsid = conn.generate_id()?;
        conn.render_create_glyph_set(gsid, visual_info.render.pict_format)?;

        let font = Self::evaluate(font_family, font_size)?;
        let (data, font_height) = Self::rasterize(&font, font_size);
        let (ids, glyphs, raw_data, char_map) =
            Self::generate_char_map(conn, gsid, data, font_height)?;

        conn.render_add_glyphs(gsid, &ids, &glyphs, &raw_data)
            .unwrap();

        Ok(TextRenderer {
            conn,
            gsid,
            char_map,
            font_height,
            visual_info,
        })
    }

    fn rasterize(font: &FontData, size: f32) -> (RasterizationData, i16) {
        let chars = font.chars();
        let mut data = Vec::with_capacity(chars.len());

        let mut max_height = 0;
        for (c, _) in font.chars() {
            let (metrics, bitmaps) = font.rasterize(*c, size);
            let height = metrics.height as i16 + metrics.ymin as i16;
            if height > max_height {
                max_height = height;
            }
            data.push((*c, metrics, bitmaps))
        }
        (data, max_height)
    }

    fn evaluate(family: &'static str, size: f32) -> Result<FontData, FontError> {
        let family = if family.is_empty() {
            "monospace"
        } else {
            family
        };
        let property = fonts::FontPropertyBuilder::new()
            .monospace()
            .family(family)
            .build();
        if let Some((font, _)) = fonts::get(&property) {
            let settings = FontSettings {
                scale: size,
                ..Default::default()
            };
            FontData::from_bytes(font, settings).map_err(FontError::LoadFromBytes)
        } else {
            Err(FontError::NotFound(family))
        }
    }

    fn generate_char_map(
        conn: &C,
        glyphset_id: u32,
        data: RasterizationData,
        font_height: i16,
    ) -> Result<CharMapData, FontError> {
        let mut ids = vec![];
        let mut glyphs = vec![];
        let mut raw_data = vec![];
        let mut char_map: Map<char, CharInfo> = Map::new();

        fn current_out_size(ids: usize, infos: usize, raw_data: usize) -> usize {
            core::mem::size_of::<u32>()
                + core::mem::size_of::<u32>() * ids
                + core::mem::size_of::<u32>() * infos
                + core::mem::size_of::<u32>() * raw_data
        }

        for (id, (c, metrics, bitmaps)) in data.into_iter().enumerate() {
            let id = id as u32;
            for byte in bitmaps {
                raw_data.extend_from_slice(&[byte, byte, byte, byte]);
            }

            let horizontal_space = metrics.advance_width as i16;
            let glyph_info = Glyphinfo {
                width: metrics.width as u16,
                height: metrics.height as u16,
                x: -metrics.xmin as i16,
                y: metrics.height as i16 - font_height + metrics.ymin as i16,
                x_off: horizontal_space,
                y_off: metrics.advance_height as i16,
            };

            ids.push(id);
            glyphs.push(glyph_info);
            char_map.insert(
                c,
                CharInfo {
                    glyph_id: id,
                    horizontal_space,
                    height: metrics.height as u16,
                },
            );

            let current_out_size = current_out_size(ids.len(), glyphs.len(), raw_data.len());
            if current_out_size >= 32768 {
                conn.render_add_glyphs(glyphset_id, &ids, &glyphs, &raw_data)?;
                ids.clear();
                glyphs.clear();
                raw_data.clear();
            }
        }
        Ok((ids, glyphs, raw_data, char_map))
    }

    pub fn text_width(&self, text: impl ToString) -> u16 {
        text.to_string().chars().fold(0u16, |acc, c| {
            if let Some(c) = self.char_map.get(&c) {
                return acc + c.horizontal_space as u16;
            }
            acc
        })
    }

    fn geometry(&self, text: impl ToString) -> (i16, u16) {
        let text = text.to_string();
        let mut width = 0;
        let mut height = 0;
        for c in text.chars() {
            if let Some(lc) = self.char_map.get(&c) {
                width += lc.horizontal_space;
                if height < lc.height {
                    height = lc.height;
                }
            }
        }
        (width, height)
    }

    pub fn encode(&self, text: &str, max_width: i16) -> Vec<FontEncodedChunk> {
        let mut total_width = 0;
        let mut total_glyphs = 0;
        let mut cur_width = 0;
        let mut cur_glyphs = vec![];
        let mut chunks = vec![];
        for char in text.chars() {
            total_glyphs += 1;
            if let Some(lchar) = self.char_map.get(&char) {
                if !cur_glyphs.is_empty() {
                    chunks.push(FontEncodedChunk {
                        width: core::mem::take(&mut cur_width),
                        font_height: self.font_height,
                        glyph_set: self.gsid,
                        glyph_ids: core::mem::take(&mut cur_glyphs),
                    });
                }

                if total_width + lchar.horizontal_space > max_width && !cur_glyphs.is_empty() {
                    chunks.push(FontEncodedChunk {
                        width: cur_width,
                        font_height: self.font_height,
                        glyph_set: self.gsid,
                        glyph_ids: cur_glyphs,
                    });
                    return chunks;
                }

                total_width += lchar.horizontal_space;
                chunks.push(FontEncodedChunk {
                    width: lchar.horizontal_space,
                    font_height: self.font_height,
                    glyph_set: self.gsid,
                    glyph_ids: vec![lchar.glyph_id],
                })
            }
            if total_glyphs == 254 {
                break;
            }
        }

        if !cur_glyphs.is_empty() {
            chunks.push(FontEncodedChunk {
                width: cur_width,
                font_height: self.font_height,
                glyph_set: self.gsid,
                glyph_ids: cur_glyphs,
            })
        }
        chunks
    }

    pub fn draw(
        &self,
        rect: WRect,
        text: &str,
        padding: u16,
        dst_picture: Picture,
        dst_window: Window,
        bg: Color,
        fg: Color,
        is_tag: bool,
    ) -> Result<(), FontError> {
        let text_width = self.text_width(&text);
        let chunks = self.encode(&text, text_width as i16 - 1);

        let text_picture = self.conn.generate_id()?;
        let text_pixmap = self.conn.generate_id()?;

        self.conn.create_pixmap(
            self.visual_info.root.depth,
            text_pixmap,
            dst_window,
            rect.w,
            rect.h,
        )?;

        self.conn.render_create_picture(
            text_picture,
            text_pixmap,
            self.visual_info.root.pict_format,
            &CreatePictureAux::new().repeat(Repeat::NORMAL),
        )?;

        let WRect { x, y, w, h } = rect;
        let fg_fill_area: Rectangle = WRect::new(0, y, w, h).into();
        let bg_fill_area: Rectangle = rect.into();

        self.conn
            .render_fill_rectangles(PictOp::SRC, text_picture, fg, &[fg_fill_area])?;
        self.conn
            .render_fill_rectangles(PictOp::SRC, dst_picture, bg, &[bg_fill_area])?;

        let mut x_offset = if is_tag {
            x + ((w as i16 / 2) - (text_width as i16 / 2))
        } else {
            x + padding as i16
        };

        for chunk in &chunks {
            self.draw_glyphs(
                x_offset,
                y,
                chunk.glyph_set,
                text_picture,
                dst_picture,
                &chunk.glyph_ids,
            )?;

            x_offset += chunk.width;
        }

        Ok(())
    }

    fn draw_glyphs(
        &self,
        x: i16,
        y: i16,
        glyphs: Glyphset,
        src: Picture,
        dst: Picture,
        glyph_ids: &[u32],
    ) -> Result<(), FontError> {
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

        self.conn
            .render_composite_glyphs16(PictOp::OVER, src, dst, 0, glyphs, 0, 0, &buf)?;
        Ok(())
    }
}

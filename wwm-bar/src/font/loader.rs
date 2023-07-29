use font_loader::system_fonts as fonts;
use fontdue::{Font as FontData, FontSettings, Metrics};
use smallmap::Map;
use thiserror::Error;
use x11rb::{
    connection::Connection,
    protocol::render::{ConnectionExt, Glyphinfo, Glyphset, Pictformat},
    rust_connection::{ConnectionError, ReplyOrIdError},
};

#[derive(Error, Debug)]
pub enum FontError {
    #[error("Failed to load font data: {0}")]
    LoadFromBytes(&'static str),
    #[error("Failed to create glyphset: {0:?}")]
    CreateGlyphset(#[from] ConnectionError),
    #[error("Failed to create glyphset ID: {0:?}")]
    GSID(#[from] ReplyOrIdError),
}

pub struct X11Font {
    pub gsid: Glyphset,
    pub char_map: Map<char, CharInfo>,
    pub font_height: i16,
    pub font: FontData,
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
    pub glyph_set: Glyphset,
    pub glyph_ids: Vec<u32>,
}

type RasterizationData = Vec<(char, Metrics, Vec<u8>)>;
type CharMapData = (Vec<u32>, Vec<Glyphinfo>, Vec<u8>, Map<char, CharInfo>);

impl X11Font {
    pub fn new<C: Connection>(
        conn: &C,
        pict_format: Pictformat,
        family: &'static str,
        font_size: f32,
    ) -> Result<Self, FontError> {
        let gsid = conn.generate_id()?;
        conn.render_create_glyph_set(gsid, pict_format)?;

        let font = Self::evaluate(family, font_size)?;
        let (data, font_height) = Self::rasterize(&font, font_size);
        let (ids, infos, raw_data, char_map) =
            Self::generate_char_map(conn, gsid, data, font_height);

        conn.render_add_glyphs(gsid, &ids, &infos, &raw_data)
            .unwrap();

        Ok(X11Font {
            gsid,
            char_map,
            font_height,
            font,
        })
    }

    fn current_out_size(ids: usize, infos: usize, raw_data: usize) -> usize {
        core::mem::size_of::<u32>()
            + core::mem::size_of::<u32>() * ids
            + core::mem::size_of::<u32>() * infos
            + core::mem::size_of::<u32>() * raw_data
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
            Err(FontError::LoadFromBytes(family))
        }
    }

    fn generate_char_map<C: Connection>(
        conn: &C,
        glyphset_id: u32,
        data: RasterizationData,
        font_height: i16,
    ) -> CharMapData {
        let mut ids = vec![];
        let mut infos = vec![];
        let mut raw_data = vec![];
        let mut char_map: Map<char, CharInfo> = Map::new();

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
            infos.push(glyph_info);
            char_map.insert(
                c,
                CharInfo {
                    glyph_id: id,
                    horizontal_space,
                    height: metrics.height as u16,
                },
            );

            let current_out_size = Self::current_out_size(ids.len(), infos.len(), raw_data.len());
            if current_out_size >= 32768 {
                conn.render_add_glyphs(glyphset_id, &ids, &infos, &raw_data)
                    .unwrap();
                ids.clear();
                infos.clear();
                raw_data.clear();
            }
        }
        (ids, infos, raw_data, char_map)
    }

    pub fn geometry(&self, text: &str) -> (i16, u16) {
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
        let (w, h) = (width, height);
        (w, h)
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
}

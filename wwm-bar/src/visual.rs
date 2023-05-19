use smallmap::Map;
use thiserror::Error;
use x11rb::{
    connection::Connection,
    protocol::{
        render::{query_pict_formats, Directformat, PictType, Pictformat, Pictforminfo},
        xproto::{Screen, Visualid, Visualtype},
    },
    rust_connection::{ConnectionError, ParseError, ReplyError},
};

#[derive(Error, Debug)]
pub enum VisualError {
    #[error("Failed to query for pict formats: {0}")]
    QueryError(#[from] ConnectionError),
    #[error("Failed to get a query reply: {0}")]
    ReplyError(#[from] ReplyError),
    #[error("No appropriate visual found")]
    NoAppropriateVisual,
    #[error("The server sent a malformed visual type: {0:?}")]
    Malformed(#[from] ParseError),
    #[error("The root visual is not true or direct color, but {0:?}")]
    NotTrueOrDirect(Visualtype),
}

#[derive(Debug)]
pub struct RenderVisualInfo {
    pub screen_root: u32,
    pub root: VisualInfo,
    pub render: VisualInfo,
}

#[derive(Debug)]
pub struct VisualInfo {
    pub id: Visualid,
    pub pict_format: Pictformat,
    pub direct_format: Directformat,
    pub depth: u8,
}

impl RenderVisualInfo {
    pub fn new<C: Connection>(conn: &C, screen: &Screen) -> Result<Self, VisualError> {
        let rvi = Self {
            screen_root: screen.root,
            root: VisualInfo::find_appropriate_visual(
                conn,
                screen.root_depth,
                Some(screen.root_visual),
            )?,
            render: VisualInfo::find_appropriate_visual(conn, 32, None)?,
        };
        Ok(rvi)
    }
}

impl VisualInfo {
    pub fn find_appropriate_visual<C: Connection>(
        conn: &C,
        depth: u8,
        id: Option<Visualid>,
    ) -> Result<VisualInfo, VisualError> {
        let formats = query_pict_formats(conn)?.reply()?;
        let candidates = formats
            .formats
            .into_iter()
            .filter_map(|pfi| {
                (pfi.type_ == PictType::DIRECT && pfi.depth == depth).then_some((pfi.id, pfi))
            })
            .collect::<Map<Pictformat, Pictforminfo>>();
        for screen in formats.screens {
            let candidate = screen
                .depths
                .into_iter()
                .find_map(|pd| {
                    (pd.depth == depth).then(|| {
                        pd.visuals.into_iter().find(|pv| {
                            if let Some(match_vid) = id {
                                pv.visual == match_vid && candidates.contains_key(&pv.format)
                            } else {
                                candidates.contains_key(&pv.format)
                            }
                        })
                    })
                })
                .flatten();
            if let Some(c) = candidate {
                return Ok(VisualInfo {
                    id: c.visual,
                    pict_format: c.format,
                    direct_format: candidates[&c.format].direct,
                    depth,
                });
            }
        }
        Err(VisualError::NoAppropriateVisual)
    }
}

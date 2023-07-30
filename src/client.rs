use x11rb::{properties::WmSizeHints, protocol::xproto::Window};

use crate::config::theme::window::BORDER_WIDTH;
use wwm_core::util::{WRect, WSize};

#[derive(Default, Debug, Clone, Copy)]
pub struct WClientState {
    pub window: Window,
    pub rect: WRect,
    pub old_rect: WRect,
    pub is_floating: bool,
    pub is_fullscreen: bool,
    pub is_fixed: bool,
    pub hints_valid: bool,
    pub bw: u16,
    pub base_size: Option<WSize>,
    pub min_size: Option<WSize>,
    pub max_size: Option<WSize>,
    pub inc_size: Option<WSize>,
    pub maxa: Option<f32>,
    pub mina: Option<f32>,
    pub tag: usize,
    pub monitor: usize,
    pub old_state: bool,
    pub old_bw: u16,
    pub prev: Option<usize>,
    pub next: Option<usize>,
}

impl WClientState {
    pub fn new(
        window: Window,
        rect: WRect,
        old_rect: WRect,
        is_floating: bool,
        is_fullscreen: bool,
        tag: usize,
        monitor: usize,
    ) -> Self {
        Self {
            window,
            rect,
            old_rect,
            is_floating,
            is_fullscreen,
            is_fixed: false,
            hints_valid: false,
            bw: BORDER_WIDTH,
            base_size: None,
            min_size: None,
            max_size: None,
            inc_size: None,
            maxa: None,
            mina: None,
            tag,
            monitor,
            old_state: false,
            old_bw: 0,
            prev: None,
            next: None,
        }
    }

    pub fn apply_size_hints(&mut self, hints: WmSizeHints) {
        if hints.base_size.is_some() {
            self.base_size = WSize::from(hints.base_size);
        } else if hints.min_size.is_some() {
            self.base_size = WSize::from(hints.min_size);
        } else {
            self.base_size = None;
        }

        if hints.size_increment.is_some() {
            self.inc_size = WSize::from(hints.size_increment);
        } else {
            self.inc_size = None;
        }

        if hints.max_size.is_some() {
            self.max_size = WSize::from(hints.max_size);
        } else {
            self.max_size = None;
        }

        if hints.min_size.is_some() {
            self.min_size = WSize::from(hints.min_size);
        } else {
            self.min_size = None;
        }

        if let Some((min_a, max_a)) = hints.aspect {
            self.mina = Some(min_a.numerator as f32 / min_a.denominator as f32);
            self.maxa = Some(max_a.numerator as f32 / max_a.denominator as f32);
        } else {
            self.mina = None;
            self.maxa = None;
        }

        self.is_fixed = false;
        if let Some(max_size) = self.max_size {
            if let Some(min_size) = self.min_size {
                self.is_fixed = max_size.w > 0
                    && max_size.h > 0
                    && max_size.w == min_size.w
                    && max_size.h == min_size.h;
            }
        }
        self.hints_valid = true;
    }

    pub fn adjust_aspect_ratio(&self, mut w: u16, mut h: u16) -> (u16, u16) {
        // ICCCM 4.1.2.3
        let mut base_is_min = false;
        let mut base_exists = false;
        let mut min_exists = false;
        if let Some(base_size) = self.base_size {
            base_exists = true;
            if let Some(min_size) = self.min_size {
                min_exists = true;
                base_is_min = base_size.w == min_size.w && base_size.h == min_size.h;
            }
        }

        if base_is_min && base_exists {
            let base = self.base_size.unwrap();
            w -= base.w;
            h -= base.h;
        }

        if let Some(maxa) = self.maxa {
            if maxa < w as f32 / h as f32 {
                w = (h as f32 * maxa + 0.5) as u16;
            }
        }

        if let Some(mina) = self.mina {
            if mina < h as f32 / w as f32 {
                h = (w as f32 * mina + 0.5) as u16;
            }
        }

        if base_is_min && base_exists {
            let base = self.base_size.unwrap();
            w -= base.w;
            h -= base.h;
        }

        if let Some(inc_size) = self.inc_size {
            w -= w % inc_size.w;
            h -= h % inc_size.h;
        }

        if min_exists && base_exists {
            let base = self.base_size.unwrap();
            let min = self.min_size.unwrap();
            w = min.w.max(w + base.w);
            h = min.h.max(h + base.h);
        }

        if let Some(max_size) = self.max_size {
            w = w.min(max_size.w);
            h = h.min(max_size.h);
        }
        (w, h)
    }
}

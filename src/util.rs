use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

use crate::{client::WClientState, config, monitor::WMonitor};

pub type ClientCell = Rc<RefCell<WClientState>>;

use thiserror::Error;
use x11rb::{
    connection::Connection,
    protocol::xproto::{ConfigureWindowAux, GetGeometryReply, MotionNotifyEvent},
};

#[derive(Default, Debug, Clone, Copy)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

impl From<&GetGeometryReply> for Rect {
    fn from(g: &GetGeometryReply) -> Self {
        Self {
            x: g.x,
            y: g.y,
            w: g.width,
            h: g.height,
        }
    }
}

impl From<Rect> for ConfigureWindowAux {
    fn from(cr: Rect) -> Self {
        ConfigureWindowAux::new()
            .x(cr.x as i32)
            .y(cr.y as i32)
            .width(cr.w as u32)
            .height(cr.h as u32)
    }
}

impl Rect {
    pub fn new(x: i16, y: i16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            w: width,
            h: height,
        }
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Size {
    pub w: u16,
    pub h: u16,
}

impl From<(i32, i32)> for Size {
    fn from((w, h): (i32, i32)) -> Self {
        Self {
            w: w as u16,
            h: h as u16,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Pos {
    pub x: i16,
    pub y: i16,
}

impl From<Rect> for Pos {
    fn from(value: Rect) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

impl From<&MotionNotifyEvent> for Pos {
    fn from(value: &MotionNotifyEvent) -> Self {
        Self {
            x: value.event_x,
            y: value.event_y,
        }
    }
}

impl Pos {
    pub fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum WDirection {
    Prev,
    Next,
}

#[derive(Error, Debug)]
pub enum StateError {
    #[error("{0} is out of bounds")]
    Bounds(usize),
}

#[derive(Default, Debug)]
pub struct WVec<T> {
    inner: Vec<Rc<RefCell<T>>>,
    index: usize,
}

impl<T> From<Vec<T>> for WVec<T> {
    fn from(v: Vec<T>) -> Self {
        Self {
            inner: v.into_iter().map(|t| Rc::new(RefCell::new(t))).collect(),
            index: 0,
        }
    }
}

impl<T> WVec<T> {
    pub fn new(inner: Vec<T>, start_index: usize) -> Self {
        Self {
            inner: inner
                .into_iter()
                .map(|t| Rc::new(RefCell::new(t)))
                .collect(),
            index: start_index,
        }
    }

    fn check_bounds(&mut self) {
        if self.index >= self.inner.len() {
            self.index = self.inner.len().saturating_sub(1);
        }
    }

    pub fn find<F: FnMut(&Rc<RefCell<T>>) -> bool>(&self, p: F) -> Option<Rc<RefCell<T>>> {
        if let Some(idx) = self.position(p) {
            return Some(self.inner[idx].clone());
        }
        None
    }

    pub fn find_and_select<F: FnMut(&Rc<RefCell<T>>) -> bool>(&mut self, p: F) {
        if let Some(new_idx) = self.inner.iter().position(p) {
            self.index = new_idx;
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn inner(&self) -> &Vec<Rc<RefCell<T>>> {
        &self.inner
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn next_index(&mut self, should_wrap: bool, should_set: bool) -> Option<usize> {
        if should_wrap {
            self.next_wrapped(should_set)
        } else {
            self.next_nonwrapped(should_set)
        }
    }

    fn next_nonwrapped(&mut self, should_set: bool) -> Option<usize> {
        if self.inner.is_empty() {
            return None;
        }

        let new = if self.index.saturating_add(1) < self.inner.len() {
            self.index.saturating_add(1)
        } else {
            self.index
        };

        if should_set {
            self.index = new;
        }

        Some(new)
    }

    fn next_wrapped(&mut self, should_set: bool) -> Option<usize> {
        if self.inner.is_empty() {
            return None;
        }

        let new = if self.index.saturating_add(1) >= self.inner.len() {
            0
        } else {
            self.index.saturating_add(1)
        };

        if should_set {
            self.index = new;
        }

        Some(new)
    }

    pub fn position<F: FnMut(&Rc<RefCell<T>>) -> bool>(&self, p: F) -> Option<usize> {
        self.inner.iter().position(p)
    }

    pub fn prev_index(&mut self, should_wrap: bool, should_set: bool) -> Option<usize> {
        if should_wrap {
            self.prev_wrapped(should_set)
        } else {
            self.prev_nonwrapped(should_set)
        }
    }

    fn prev_nonwrapped(&mut self, should_set: bool) -> Option<usize> {
        if self.inner.is_empty() {
            return None;
        }

        let new = if self.index.saturating_sub(1) == 0 {
            0
        } else {
            self.index.saturating_sub(1)
        };

        if should_set {
            self.index = new;
        }

        Some(new)
    }

    fn prev_wrapped(&mut self, should_set: bool) -> Option<usize> {
        if self.inner.is_empty() {
            return None;
        }

        let new = if self.index == 0 {
            self.inner.len() - 1
        } else {
            self.index.saturating_sub(1)
        };

        if should_set {
            self.index = new;
        }

        Some(new)
    }

    pub fn push_and_select(&mut self, item: T) {
        self.inner.push(Rc::new(RefCell::new(item)));
        self.index = self.inner.len() - 1;
    }

    pub fn remove_current(&mut self) -> Option<T> {
        if self.inner.is_empty() {
            return None;
        }
        let removed = self.inner.remove(self.index);
        self.check_bounds();

        if let Ok(removed) = Rc::try_unwrap(removed) {
            Some(removed.into_inner())
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, index: usize) -> Option<RefMut<T>> {
        if index >= self.inner.len() {
            return None;
        }

        Some(self.inner[index].borrow_mut())
    }

    pub fn get(&self, index: usize) -> Option<Rc<RefCell<T>>> {
        if index >= self.inner.len() {
            return None;
        }

        Some(Rc::clone(&self.inner[index]))
    }

    pub fn retain<F: FnMut(&Rc<RefCell<T>>) -> bool>(&mut self, p: F) {
        self.inner.retain(p);
        self.check_bounds();
    }

    pub fn select(&mut self, index: usize) -> Result<(), StateError> {
        if index >= self.inner.len() {
            return Err(StateError::Bounds(index));
        }

        self.index = index;

        Ok(())
    }

    pub fn selected(&self) -> Option<Rc<RefCell<T>>> {
        if self.inner.is_empty() {
            return None;
        }
        Some(self.inner[self.index].clone())
    }

    pub fn swap(&mut self, other: usize) -> Result<(), StateError> {
        if other >= self.inner.len() {
            return Err(StateError::Bounds(other));
        }
        self.inner.swap(self.index, other);

        // swap focus to the other index
        self.index = other;
        Ok(())
    }
}

pub fn cmd_bits(cmd: &'static [&'static str]) -> Option<(&'static str, &'static [&'static str])> {
    if cmd.is_empty() {
        return None;
    }

    if cmd.len() == 1 {
        return Some((cmd[0], &[]));
    }

    let (cmd, args) = cmd.split_at(1);
    Some((cmd[0], args))
}

pub const fn bar_height() -> u16 {
    config::theme::bar::FONT_SIZE as u16 + (config::theme::bar::PADDING * 2)
}

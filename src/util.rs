use std::{cell::RefCell, rc::Rc};

use crate::client::WClientState;

pub type ClientCell = Rc<RefCell<WClientState>>;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum StateError {
    #[error("{0} is out of bounds")]
    Bounds(usize),
}

#[derive(Default, Debug)]
pub struct WVec<T: Default> {
    inner: Vec<Rc<RefCell<T>>>,
    index: usize,
}

impl<T: Default> From<Vec<T>> for WVec<T> {
    fn from(v: Vec<T>) -> Self {
        Self {
            inner: v.into_iter().map(|t| Rc::new(RefCell::new(t))).collect(),
            index: 0,
        }
    }
}

impl<T: Default> WVec<T> {
    pub fn new(inner: Vec<T>, start_index: usize) -> Self {
        Self {
            inner: inner
                .into_iter()
                .map(|t| Rc::new(RefCell::new(t)))
                .collect(),
            index: start_index,
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn next_index(&mut self, should_wrap: bool, should_set: bool) -> Option<usize> {
        if should_wrap {
            self.next_wrapped(should_set)
        } else {
            self.next_nonwrapped(should_set)
        }
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

    pub fn prev_index(&mut self, should_wrap: bool, should_set: bool) -> Option<usize> {
        if should_wrap {
            self.prev_wrapped(should_set)
        } else {
            self.prev_nonwrapped(should_set)
        }
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

    pub fn inner(&self) -> &Vec<Rc<RefCell<T>>> {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut Vec<Rc<RefCell<T>>> {
        &mut self.inner
    }

    pub fn selected(&self) -> Option<Rc<RefCell<T>>> {
        if self.inner.is_empty() {
            return None;
        }
        Some(self.inner[self.index].clone())
    }

    pub fn find_and_select<F: FnMut(&Rc<RefCell<T>>) -> bool>(&mut self, p: F) {
        if let Some(new_idx) = self.inner.iter().position(p) {
            self.index = new_idx;
        }
    }

    pub fn retain<F: FnMut(&Rc<RefCell<T>>) -> bool>(&mut self, p: F) {
        self.inner.retain(p);
        self.check_bounds();
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

    pub fn find<F: FnMut(&Rc<RefCell<T>>) -> bool>(&self, p: F) -> Option<Rc<RefCell<T>>> {
        if let Some(idx) = self.position(p) {
            return Some(self.inner[idx].clone());
        }
        None
    }

    pub fn position<F: FnMut(&Rc<RefCell<T>>) -> bool>(&self, p: F) -> Option<usize> {
        self.inner.iter().position(p)
    }

    pub fn push_and_select(&mut self, item: T) {
        self.inner.push(Rc::new(RefCell::new(item)));
        self.index = self.inner.len() - 1;
    }

    pub fn remove_current(&mut self) {
        if self.inner.is_empty() {
            return;
        }
        self.inner.remove(self.index);
        self.check_bounds();
    }

    fn check_bounds(&mut self) {
        if self.index >= self.inner.len() {
            self.index = self.inner.len() - 1;
        }
    }

    pub fn select(&mut self, index: usize) -> Result<(), StateError> {
        if index >= self.inner.len() {
            return Err(StateError::Bounds(index));
        }

        self.index = index;

        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

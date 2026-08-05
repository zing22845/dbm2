//! A `ratatui::backend::Backend` wrapper that counts how many cells actually
//! changed on screen during each `draw`.
//!
//! The redundancy metric needs to know, per frame, how many of the redrawn
//! cells differed from the previous frame (`0` == fully redundant). ratatui's
//! `Terminal::draw` already computes this diff exactly once and streams the
//! changed cells to `Backend::draw`, so this wrapper forwards that stream to
//! the real backend and counts the cells as they pass through — single pass,
//! no extra diff, no allocation.

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Rect, Size};

/// Wraps any `Backend`, transparently forwarding all operations while counting
/// the changed cells per `draw`.
#[derive(Debug)]
pub struct CountingBackend<B> {
    inner: B,
    /// Cells changed in the most recent `draw`; `0` means the frame was fully
    /// redundant (identical to the previous frame).
    last_changed_cells: usize,
    /// Regions excluded from the changed-cell count (e.g. the footer's live
    /// fps/red readout), so self-updating debug chrome doesn't mask real
    /// redundancy. Fed by the run loop before each draw.
    exclude_rects: Vec<Rect>,
}

impl<B> CountingBackend<B> {
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            last_changed_cells: 0,
            exclude_rects: Vec::new(),
        }
    }

    /// The number of changed cells in the most recent `draw`.
    pub fn last_changed_cells(&self) -> usize {
        self.last_changed_cells
    }

    /// Replace the set of regions ignored when counting changed cells.
    pub fn set_exclude_rects(&mut self, rects: Vec<Rect>) {
        self.exclude_rects = rects;
    }

    /// Mutable access to the wrapped backend (e.g. to read the terminal size).
    pub fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }
}

impl<B: Backend> Backend for CountingBackend<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        // ratatui already built the buffer diff exactly once; `content` is the
        // iterator of *changed* cells (not the whole screen). Stream it
        // straight to the real backend so every cell is still DRAWN, and only
        // use `inspect` to count the ones outside `exclude_rects`.
        let exclude = self.exclude_rects.clone();
        let mut count = 0usize;
        let result = self.inner.draw(content.inspect(|&(x, y, _)| {
            if !exclude
                .iter()
                .any(|r| r.contains(Position::new(x, y)))
            {
                count += 1;
            }
        }));
        self.last_changed_cells = count;
        result
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

//! Transient gesture state shared by the pointer handlers.
//!
//! These values describe *the gesture in progress*, not the application: which
//! splitter is being dragged, where the last press landed (double-click
//! detection), the last wheel tick (trackpad debounce) and the event-loop spin
//! watchdog. They must survive across events, but they are not application
//! state — a TEA `update` never sees them — so they stay out of `AppState`.
//!
//! Only the gesture bookkeeping lives here. The *view-relevant* feedback of the
//! same gesture (the splitter hover highlight and the dragged scrollbar) is
//! painted by the renders, so those fields live on `AppState`
//! (`splitter_hover`, `scrollbar_drag`) where the view can read them; they are
//! written by the same input handlers and enjoy the same "transient, not routed
//! through `update`" status.

use std::time::Instant;

use ratatui::layout::Position;

use super::splitter::SplitterDrag;

/// Transient mouse-gesture state, owned by the run loop.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct MouseInteraction {
    /// The splitter currently being drag-resized, if any. Exactly one can be
    /// armed at a time, which is what stops a drag resizing two splits.
    pub(crate) splitter_drag: Option<SplitterDrag>,
    /// Position and time of the most recent left press, for double clicks.
    pub(crate) last_click: Option<(Position, Instant)>,
    /// `(time, direction, horizontal)` of the last wheel tick, for debouncing.
    pub(crate) last_wheel: Option<(Instant, i32, bool)>,
    /// Consecutive event-loop iterations without a repaint (spin watchdog).
    pub(crate) idle_iterations: u32,
}

/// What the run loop should do once a mouse event has been handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MouseOutcome {
    /// The event was a debounced wheel tick: skip the rest of this loop
    /// iteration (the watchdog counter has already been reset).
    Continue,
    /// The event was handled; repaint only if `repaint` is set.
    Handled { repaint: bool },
}

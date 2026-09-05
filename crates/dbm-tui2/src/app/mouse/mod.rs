//! Mouse input: turning a crossterm mouse event into app messages.
//!
//! Everything here is *input plumbing* — hit-testing a screen position against
//! the rendered layout and dispatching the resulting messages through the
//! central `update`. No state is mutated directly: every change goes through a
//! message, so the TEA data flow (`input -> msg -> update -> view`) stays intact
//! even for drags and scrollbar grabs.
//!
//! The children split the pipeline by stage:
//!
//! - [`dispatch`] is the entry point, routing one event to the child that owns
//!   its kind;
//! - [`press`] and [`drag`] hold those per-kind handlers;
//! - [`click`] routes a position to a message, [`wheel`] owns scrolling;
//! - [`splitter`] resolves which splitter a press armed;
//! - [`hover`] maintains the splitter highlight;
//! - [`state`] owns the transient gesture values, which deliberately stay out
//!   of `AppState`.

pub(crate) mod click;
pub(crate) mod dispatch;
pub(crate) mod drag;
pub(crate) mod hover;
pub(crate) mod press;
pub(crate) mod splitter;
pub(crate) mod state;
pub(crate) mod wheel;

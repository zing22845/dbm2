//! Mouse input: turning a crossterm mouse event into app messages.
//!
//! Everything here is *input plumbing* — hit-testing a screen position against
//! the rendered layout and dispatching the resulting messages through the
//! central `update`. No state is mutated directly: every change goes through a
//! message, so the TEA data flow (`input -> msg -> update -> view`) stays
//! intact even for drags and scrollbar grabs.
//!
//! The module owns the two things that would otherwise bloat the run loop:
//!
//! - the per-event-kind branches (press / drag / release / move / wheel), which
//!   used to be a ~2200-line `match` inside `run_event_loop`;
//! - the transient gesture state [`MouseInteraction`], which must survive
//!   across events but deliberately stays out of `AppState`.
//!
//! The children split the pipeline by stage: [`click`] routes a position to a
//! message, [`wheel`] owns scrolling, [`hover`] maintains the splitter
//! highlight, and `press` / `drag` hold the per-event-kind handlers this module
//! dispatches to.

pub(crate) mod click;
pub(crate) mod drag;
pub(crate) mod hover;
pub(crate) mod press;
pub(crate) mod splitter;
pub(crate) mod wheel;

use std::time::Instant;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::state::AppState;
use crate::app_shell::effect::EffectRunner;
use crate::app_shell::pane::Pane;

use self::drag::{handle_drag, handle_moved, handle_up};
use self::press::{handle_confirm_modal_click, handle_discover_close_confirm_click, handle_down};
use self::splitter::SplitterDrag;
use self::wheel::{
    WheelOutcome, handle_wheel_discover_results, handle_wheel_discover_targets,
    handle_wheel_explorer_instances, handle_wheel_explorer_objects, handle_wheel_iw_connections,
    handle_wheel_iw_overview, handle_wheel_sql,
};
use super::loop_mod::AppTerminal;

/// End every in-progress held-button drag at once: the active scrollbar, all
/// splitter drag flags, and the results column-resize drag.
///
/// Called from the three places a drag is known to be over:
/// - a real `MouseEventKind::Up`,
/// - a `FocusLost` (the button was released *outside* the window, so no `Up`
///   was ever delivered — otherwise the highlight would stay stuck),
/// - a fresh `Down` that proves the previous release was missed.
///
/// Centralizing the clear keeps it in one place instead of three duplicated
/// blocks (and mirrors the original dbm, which clears scrollbar/splitter/
/// column-resize together in its single `Up` arm).
pub(crate) fn clear_active_drags(state: &mut AppState) {
    state.scrollbar_drag = None;
    state.splitter_hover.set_dragging_flags([false; 7]);
    state.splitter_hover.results_col_resize_drag = None;
}

/// Transient mouse-gesture state, owned by the run loop.
///
/// These values describe *the gesture in progress*, not the application: which
/// splitter is being dragged, where the last press landed (double-click
/// detection), the last wheel tick (trackpad debounce) and the event-loop spin
/// watchdog. They must survive across events, but they are not application
/// state — a TEA `update` never sees them — so they stay out of `AppState`.
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

/// Handle one mouse event: hit-test its position against the rendered layout
/// and dispatch the resulting messages through `update`.
///
/// `interaction` carries the gesture state across events (see
/// [`MouseInteraction`]); it is written back on every exit path.
pub(crate) fn handle_mouse_event(
    mouse: MouseEvent,
    terminal: &mut AppTerminal,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    interaction: &mut MouseInteraction,
) -> anyhow::Result<MouseOutcome> {
    tracing::trace!(
        kind = ?mouse.kind,
        col = mouse.column,
        row = mouse.row,
        "mouse event received"
    );
    let point = Position::new(mouse.column, mouse.row);
    // Take the gesture state by value: every field is `Copy` and this function
    // has exactly one exit point below, so it is always written back.
    let mut splitter_drag = interaction.splitter_drag;
    let mut last_click = interaction.last_click;
    let mut last_wheel = interaction.last_wheel;
    let mut idle_iterations = interaction.idle_iterations;
    // Aggregated across the dispatched messages below: the round repaints only
    // if one of them changed rendered state.
    let mut dirty = false;

    // A fresh mouse-button press while a drag is still recorded means the
    // previous `Up` was missed (e.g. released outside the window without the
    // terminal losing focus). Drop the stale drag so its highlight cannot stay
    // stuck across the next redraw. A second `Down` can never be legitimate
    // while a drag is active — crossterm never delivers one without an
    // intervening `Up`.
    if matches!(mouse.kind, MouseEventKind::Down(_))
        && (state.scrollbar_drag.is_some()
            || splitter_drag.is_some()
            || state.splitter_hover.results_col_resize_drag.is_some()
            || state.splitter_hover.dragging_flags().iter().any(|&f| f))
    {
        splitter_drag = None;
        state.scrollbar_drag = None;
        state.splitter_hover.set_dragging_flags([false; 7]);
        state.splitter_hover.results_col_resize_drag = None;
        dirty = true;
    }

    // Set when a wheel tick is swallowed by the trackpad debounce: the run loop
    // must then skip the rest of its iteration.
    let mut skip_iteration = false;
    'mouse: {
        match mouse.kind {
            // A confirm modal is open: clicking its Yes/No button
            // confirms or cancels, matching the `y`/`n` keys.
            MouseEventKind::Down(MouseButton::Left)
                if state
                    .modal
                    .as_ref()
                    .is_some_and(crate::common::view::modal::is_confirm_modal) =>
            {
                handle_confirm_modal_click(
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    point,
                    &mut dirty,
                )?
            }
            // Discover's close-confirmation dialog: clicking Yes/No
            // confirms or cancels closing discover.
            MouseEventKind::Down(MouseButton::Left)
                if state.modal.is_none()
                    && matches!(state.focus, Pane::Discover(_))
                    && state.discover.close_confirm =>
            {
                handle_discover_close_confirm_click(
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    point,
                    &mut dirty,
                )?
            }
            MouseEventKind::Down(MouseButton::Left) if state.modal.is_none() => handle_down(
                &mouse,
                terminal,
                state,
                effect_runner,
                action_rx,
                point,
                &mut dirty,
                &mut splitter_drag,
                &mut last_click,
            )?,
            MouseEventKind::Drag(MouseButton::Left) => handle_drag(
                terminal,
                state,
                effect_runner,
                action_rx,
                point,
                &mut dirty,
                &mut splitter_drag,
            )?,
            MouseEventKind::Up(MouseButton::Left) => {
                handle_up(&mouse, terminal, state, &mut dirty, &mut splitter_drag)?
            }
            MouseEventKind::Moved => {
                handle_moved(&mouse, terminal, state, &mut dirty, &mut splitter_drag)?
            }

            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if matches!(
                    state.focus,
                    Pane::Discover(crate::app_shell::nav::DiscoverPane::Targets)
                ) && !state.discover.close_confirm =>
            {
                if handle_wheel_discover_targets(
                    &mouse,
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    point,
                    &mut dirty,
                    &mut last_wheel,
                    &mut idle_iterations,
                )? == WheelOutcome::Debounced
                {
                    skip_iteration = true;
                    break 'mouse;
                }
            }

            // Scroll wheel: route to the discover results pane
            // when focus is on Results and mouse is inside it.
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if matches!(
                    state.focus,
                    Pane::Discover(crate::app_shell::nav::DiscoverPane::Results)
                ) && !state.discover.close_confirm =>
            {
                if handle_wheel_discover_results(
                    &mouse,
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    &mut dirty,
                    &mut last_wheel,
                    &mut idle_iterations,
                )? == WheelOutcome::Debounced
                {
                    skip_iteration = true;
                    break 'mouse;
                }
            }

            // Scroll wheel: route to explorer objects pane when
            // focus is on Explorer Objects and mouse is inside it.
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
                if matches!(
                    state.focus,
                    Pane::Explorer(crate::app_shell::nav::ExplorerPane::Objects)
                ) =>
            {
                if handle_wheel_explorer_objects(
                    &mouse,
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    &mut dirty,
                    &mut last_wheel,
                    &mut idle_iterations,
                )? == WheelOutcome::Debounced
                {
                    skip_iteration = true;
                    break 'mouse;
                }
            }

            // Scroll wheel: route to explorer instances pane when
            // focus is on Explorer Instances and mouse is inside it.
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
                if matches!(
                    state.focus,
                    Pane::Explorer(crate::app_shell::nav::ExplorerPane::Instances)
                ) =>
            {
                if handle_wheel_explorer_instances(
                    &mouse,
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    &mut dirty,
                    &mut last_wheel,
                    &mut idle_iterations,
                )? == WheelOutcome::Debounced
                {
                    skip_iteration = true;
                    break 'mouse;
                }
            }

            // Scroll wheel: route to IW connections when focus is on
            // InstanceWorkspace Connections AND mouse is inside IW body.
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if matches!(
                    state.focus,
                    Pane::InstanceWorkspace(crate::app_shell::nav::IwPane::Connections)
                ) =>
            {
                if handle_wheel_iw_connections(
                    &mouse,
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    &mut dirty,
                    &mut last_wheel,
                    &mut idle_iterations,
                )? == WheelOutcome::Debounced
                {
                    skip_iteration = true;
                    break 'mouse;
                }
            }

            // Scroll wheel: route to IW overview when focus is on
            // InstanceWorkspace Overview AND mouse is inside IW body.
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if matches!(
                    state.focus,
                    Pane::InstanceWorkspace(crate::app_shell::nav::IwPane::Overview)
                ) =>
            {
                if handle_wheel_iw_overview(
                    &mouse,
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    &mut dirty,
                    &mut last_wheel,
                    &mut idle_iterations,
                )? == WheelOutcome::Debounced
                {
                    skip_iteration = true;
                    break 'mouse;
                }
            }

            // Scroll wheel: route to the history list when the
            // mouse is inside it, matching discover targets'
            // focus+position gating. Moving the cursor pushes the
            // viewport in discover-style mode (cursor at top of
            // viewport → can scroll up; cursor at bottom → can
            // scroll down).
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
                if matches!(state.focus, Pane::SQLWorkspace) =>
            {
                if handle_wheel_sql(
                    &mouse,
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    point,
                    &mut dirty,
                    &mut last_wheel,
                    &mut idle_iterations,
                )? == WheelOutcome::Debounced
                {
                    skip_iteration = true;
                    break 'mouse;
                }
            }

            _ => {
                tracing::trace!(
                    is_left_down = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)),
                    modal_open = state.modal.is_some(),
                    "mouse event ignored"
                );
            }
        }
    }

    // Hand the gesture state back: the run loop owns it so it survives into the
    // next event without ever entering `AppState`.
    interaction.splitter_drag = splitter_drag;
    interaction.last_click = last_click;
    interaction.last_wheel = last_wheel;
    interaction.idle_iterations = idle_iterations;

    Ok(if skip_iteration {
        MouseOutcome::Continue
    } else {
        MouseOutcome::Handled { repaint: dirty }
    })
}

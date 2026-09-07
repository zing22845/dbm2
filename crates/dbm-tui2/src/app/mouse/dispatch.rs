//! Per-event-kind dispatch: the single entry point the run loop calls.
//!
//! This module only routes. The gesture values are unpacked from
//! [`MouseInteraction`], handed to the child that owns the event kind, and
//! written back on the way out — so a handler never touches `AppState` beyond
//! the messages it dispatches.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Size};
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::state::AppState;
use crate::app_shell::effect::EffectRunner;
use crate::app_shell::pane::Pane;

use super::drag::{handle_drag, handle_moved, handle_up};
use super::press::{
    handle_confirm_modal_click, handle_discover_close_confirm_click, handle_down,
    handle_results_picker_modal_click,
};
use super::state::{MouseInteraction, MouseOutcome};
use super::wheel::{
    WheelOutcome, handle_wheel_discover_results, handle_wheel_discover_targets,
    handle_wheel_explorer_instances, handle_wheel_explorer_objects, handle_wheel_iw_connections,
    handle_wheel_iw_overview, handle_wheel_sql,
};

/// Handle one mouse event: hit-test its position against the rendered layout
/// and dispatch the resulting messages through `update`.
///
/// `interaction` carries the gesture state across events (see
/// [`MouseInteraction`]); it is written back on every exit path.
pub(crate) fn handle_mouse_event(
    mouse: MouseEvent,
    size: Size,
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
            || state.sql_editor_selecting
            || splitter_drag.is_some()
            || state.splitter_hover.results_col_resize_drag.is_some()
            || state.splitter_hover.dragging_flags().iter().any(|&f| f))
    {
        splitter_drag = None;
        state.scrollbar_drag = None;
        state.sql_editor_selecting = false;
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
                    size,
                    state,
                    effect_runner,
                    action_rx,
                    point,
                    &mut dirty,
                )?
            }
            // A rows-per-page / page-jump picker is open: clicking a row-limit
            // preset applies it and a click outside the anchored popup closes
            // it, matching the original dbm's picker mouse handling.
            MouseEventKind::Down(MouseButton::Left)
                if state.modal.as_ref().is_some_and(|m| {
                    matches!(
                        m,
                        crate::app::state::ModalKind::ResultsRowLimitPicker { .. }
                            | crate::app::state::ModalKind::ResultsPageInput { .. }
                    )
                }) =>
            {
                handle_results_picker_modal_click(
                    size,
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
                    size,
                    state,
                    effect_runner,
                    action_rx,
                    point,
                    &mut dirty,
                )?
            }
            MouseEventKind::Down(MouseButton::Left) if state.modal.is_none() => handle_down(
                &mouse,
                size,
                state,
                effect_runner,
                action_rx,
                point,
                &mut dirty,
                &mut splitter_drag,
                &mut last_click,
            )?,
            MouseEventKind::Drag(MouseButton::Left) => handle_drag(
                size,
                state,
                effect_runner,
                action_rx,
                point,
                &mut dirty,
                &mut splitter_drag,
            )?,
            MouseEventKind::Up(MouseButton::Left) => handle_up(
                &mouse,
                size,
                state,
                &mut dirty,
                &mut splitter_drag,
                effect_runner,
                action_rx,
            )?,
            MouseEventKind::Moved => {
                handle_moved(&mouse, size, state, &mut dirty, &mut splitter_drag)?
            }

            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if matches!(
                    state.focus,
                    Pane::Discover(crate::app_shell::nav::DiscoverPane::Targets)
                ) && !state.discover.close_confirm =>
            {
                if handle_wheel_discover_targets(
                    &mouse,
                    size,
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
                    size,
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
                    size,
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
                    size,
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
                    size,
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
                    size,
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
                    size,
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

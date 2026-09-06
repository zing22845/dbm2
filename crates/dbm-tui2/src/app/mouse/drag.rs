//! Press continuations: button held, released, and hover moves.
//!
//! These run after the press has armed something (a splitter drag or a scrollbar
//! grab) or when the pointer moves with no button held at all.

use crossterm::event::MouseEvent;
use ratatui::layout::{
    Size, {Position, Rect},
};
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::effect::EffectRunner;
use crate::common::layout::pane_scrollbar::ActiveScrollbar;
use crate::features::global_footer::layout as footer_layout;

use super::hover::update_splitter_hover;
use super::splitter::SplitterDrag;
use crate::app::geometry::{app_explorer_rect, sql_tab_area_for_hit, workspace_rect_for_hit};
use crate::app::round::process_message_round;

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

/// A held-button drag: follow the pointer for whichever scrollbar or
/// splitter the press armed.
pub(crate) fn handle_drag(
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
    splitter_drag: &mut Option<SplitterDrag>,
) -> anyhow::Result<()> {
    // Exactly one of the arms below can match:
    // `splitter_drag` is a single slot armed by the
    // press, so a drag resizes only the splitter it
    // started on — and only along that splitter's own
    // axis (a vertical splitter reads `x`, a horizontal
    // one reads `y`).
    if *splitter_drag == Some(SplitterDrag::Discover) {
        let body_top = 3u16;
        let body_h = size
            .height
            .saturating_sub(body_top)
            .saturating_sub(footer_layout::footer_height(&state.footer, size.width));
        // Use the same workspace/popup/body geometry the
        // render uses (live Explorer width + dynamic
        // engine height), so the drag track matches the
        // rendered split exactly.
        if let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state) {
            let discover_popup = crate::common::layout::modal::popup_rect(workspace, 75, 75);
            let body = crate::features::discover::layout::discover_body_area(
                discover_popup,
                &state.discover,
            );
            if body.height >= 3 {
                let height =
                    crate::features::discover::splitter::view::targets_height_for_y(body, point.y);
                let msg = AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::SetTargetsHeight { height },
                ));
                let result = process_message_round(effect_runner, action_rx, msg, state);
                *dirty |= result.dirty;
            }
        }
    }
    if *splitter_drag == Some(SplitterDrag::Explorer) {
        let body_top = 3u16;
        let body_h = size
            .height
            .saturating_sub(body_top)
            .saturating_sub(footer_layout::footer_height(&state.footer, size.width));
        let inner = app_explorer_rect(size, body_top, body_h, state).map(|explorer| {
            Rect::new(
                explorer.x.saturating_add(1),
                explorer.y.saturating_add(1),
                explorer.width.saturating_sub(2),
                explorer.height.saturating_sub(2),
            )
        });
        if body_h >= 3
            && let Some(inner) = inner
            && inner.height >= 3
        {
            let height =
                crate::features::explorer::splitter::view::instances_height_for_y(inner, point.y);
            let msg = AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
                crate::features::explorer::msg::ExplorerMessage::SetInstancesHeight { height },
            ));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }
    if *splitter_drag == Some(SplitterDrag::App) {
        let body_top = 3u16;
        let body_h = size
            .height
            .saturating_sub(body_top)
            .saturating_sub(footer_layout::footer_height(&state.footer, size.width));
        if body_h >= 3 {
            let body_area = Rect::new(0, body_top, size.width, body_h);
            let width =
                crate::features::app_splitter::view::explorer_width_for_x(body_area, point.x);
            let msg = AppMsg::SetExplorerWidth(width);
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }
    if let Some(SplitterDrag::Sql(tab_id, splitter)) = *splitter_drag {
        tracing::trace!(?splitter, ?point, "drag move begin");
        // The feature resolves the drag to a resize
        // message; the shell only supplies the area and
        // the coordinates.
        if let Some(tab_area) = sql_tab_area_for_hit(size, state)
            && let Some(msg) =
                crate::features::sql_workspace::sql_tab::splitter::view::sql_tab_splitter_resize_msg(
                    &state.sql.sql_tab,
                    tab_area,
                    tab_id,
                    splitter,
                    point.x,
                    point.y,
                )
        {
            let msg = AppMsg::Sql(crate::features::sql_workspace::msg::SqlMsg::Message(
                crate::features::sql_workspace::msg::SqlMessage::SqlTab(
                    crate::features::sql_workspace::sql_tab::msg::SqlTabMsg::Message(msg),
                ),
            ));
            tracing::debug!(?msg, "dispatch resize msg");
            let result = process_message_round(effect_runner, action_rx, msg, state);
            tracing::debug!("resize msg processed");
            *dirty |= result.dirty;
        }
    }

    // History list h_scrollbar drag: convert the current
    // absolute mouse x to a position inside the track
    // using the track geometry captured at Down time.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::HistoryH
    {
        let position = drag.offset_for_pointer(point.x, point.y);
        if let Some(active_tab) = state.sql.sql_tab.active_tab {
            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
            use crate::features::sql_workspace::sql_tab::history::msg::{
                HistoryMessage, HistoryMsg,
            };
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::History {
                    tab_id: active_tab,
                    msg: HistoryMsg::Message(HistoryMessage::SetHScroll { position }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        } else {
            state.scrollbar_drag = None;
        }
    }

    // History list v_scrollbar drag: scrollbar position
    // IS the viewport start. Send SetVScroll directly.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::HistoryV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        if let Some(active_tab) = state.sql.sql_tab.active_tab {
            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
            use crate::features::sql_workspace::sql_tab::history::msg::{
                HistoryMessage, HistoryMsg,
            };
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::History {
                    tab_id: active_tab,
                    msg: HistoryMsg::Message(HistoryMessage::SetVScroll { position: start }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        } else {
            state.scrollbar_drag = None;
        }
    }

    // Editor body v_scrollbar drag: same linear mapping as
    // history — scrollbar thumb position IS the viewport start.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::SqlV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        if let Some(active_tab) = state.sql.sql_tab.active_tab {
            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
            use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    tab_id: active_tab,
                    msg: EditorMsg::Message(EditorMessage::SetVScroll { position: start }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        } else {
            state.scrollbar_drag = None;
        }
    }

    // Results list h_scrollbar drag: convert mouse x
    // inside the track to a horizontal scroll position.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::ResultsH
    {
        let position = drag.offset_for_pointer(point.x, point.y);
        if let Some(active_tab) = state.sql.sql_tab.active_tab {
            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            use crate::features::sql_workspace::sql_tab::results::msg::{
                ResultsMessage, ResultsMsg,
            };
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Results {
                    tab_id: active_tab,
                    msg: ResultsMsg::Message(ResultsMessage::SetHScroll { position }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        } else {
            state.scrollbar_drag = None;
        }
    }

    // Results list v_scrollbar drag: scrollbar position
    // IS the viewport start. Send SetVScroll directly.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::ResultsV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        if let Some(active_tab) = state.sql.sql_tab.active_tab {
            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            use crate::features::sql_workspace::sql_tab::results::msg::{
                ResultsMessage, ResultsMsg,
            };
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Results {
                    tab_id: active_tab,
                    msg: ResultsMsg::Message(ResultsMessage::SetVScroll { position: start }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        } else {
            state.scrollbar_drag = None;
        }
    }

    // Results column-width resize drag: convert the mouse x to the target
    // width of the dragged column using the shared list
    // geometry.
    if let Some(col) = state.splitter_hover.results_col_resize_drag {
        use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
        use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
        use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage, ResultsMsg};
        if let Some(tab_area) = sql_tab_area_for_hit(size, state)
            && let Some(active_tab) = state.sql.sql_tab.active_tab()
            && let Some(list_inner) =
                crate::features::sql_workspace::sql_tab::input::results_list_rect(
                    active_tab, tab_area,
                )
        {
            let width =
                crate::features::sql_workspace::sql_tab::results::list::view::col_width_from_drag_x(
                    list_inner,
                    &active_tab.results.list,
                    col,
                    point.x,
                );
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Results {
                    tab_id: active_tab.session.id,
                    msg: ResultsMsg::Message(ResultsMessage::AdjustColWidthTo { col, width }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Discover targets v_scrollbar drag: same linear
    // mapping as history/results.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::DiscoverTargetsV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
        use crate::features::discover::targets::msg::{TargetsMessage, TargetsMsg};
        let msg = AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Targets(
            TargetsMsg::Message(TargetsMessage::SetVScroll { position: start }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // Discover results v_scrollbar drag: same pattern.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::DiscoverResultsV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
        use crate::features::discover::results::msg::{ResultsMessage, ResultsMsg};
        let msg = AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
            ResultsMsg::Message(ResultsMessage::SetVScroll { position: start }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // Explorer objects v_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::ObjectsV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        use crate::features::explorer::objects::msg::{ObjectsMessage, ObjectsMsg};
        let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
            ObjectsMsg::Message(ObjectsMessage::SetVScroll { position: start }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // Explorer instances v_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::TreeV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
            InstancesMsg::Message(InstancesMessage::SetVScroll { position: start }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // Explorer objects h_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::ObjectsH
    {
        let position = drag.offset_for_pointer(point.x, point.y);
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        use crate::features::explorer::objects::msg::{ObjectsMessage, ObjectsMsg};
        let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
            ObjectsMsg::Message(ObjectsMessage::SetHScroll { position }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // Explorer instances h_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::TreeH
    {
        let position = drag.offset_for_pointer(point.x, point.y);
        use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
            InstancesMsg::Message(InstancesMessage::SetHScroll { position }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // IW connections v_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::ConnectionsV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::instance_workspace::connections::msg::{
            ConnectionsMessage, ConnectionsMsg,
        };
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
            ConnectionsMsg::Message(ConnectionsMessage::SetVScroll { position: start }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // IW overview v_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::OverviewV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        use crate::features::instance_workspace::overview::msg::{OverviewMessage, OverviewMsg};
        let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Overview(OverviewMsg::Message(
            OverviewMessage::SetVScroll { position: start },
        ))));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }
    Ok(())
}

/// Button release: end every held-button drag, then re-evaluate which
/// splitter (if any) the cursor is now over.
pub(crate) fn handle_up(
    mouse: &MouseEvent,
    size: Size,
    state: &mut AppState,
    dirty: &mut bool,
    splitter_drag: &mut Option<SplitterDrag>,
) -> anyhow::Result<()> {
    // One drag slot covers every scrollbar, so releasing
    // the button ends whichever one was active — no need
    // to clear a flag per bar.
    // Releasing the button ends every held-button drag at
    // once (scrollbar, splitters, results column-resize).
    // The debug logs below are kept for diagnostics; the
    // state clear itself is delegated to `clear_active_drags`.
    // `take()` *ends* the drag: the target is cleared,
    // not merely logged, so a finished drag cannot leak
    // into the next one. The old per-splitter booleans
    // were only read here and never reset, so every
    // later drag also resized whatever had been dragged
    // before (a vertical move resizing a horizontal
    // split and vice versa).
    match splitter_drag.take() {
        Some(SplitterDrag::Explorer) => {
            tracing::debug!("explorer instances/objects splitter drag finished")
        }
        Some(SplitterDrag::Discover) => {
            tracing::debug!("discover targets/results splitter drag finished")
        }
        Some(SplitterDrag::App) => {
            tracing::debug!("app explorer/workspace splitter drag finished")
        }
        Some(SplitterDrag::Sql(_, splitter)) => {
            tracing::debug!(?splitter, "splitter drag finished")
        }
        None => {}
    }
    let col_resize = state.splitter_hover.results_col_resize_drag;
    if col_resize.is_some() {
        tracing::debug!(col = col_resize, "results column resize drag finished");
    }
    // Ending a drag *changes the rendered highlight* (the
    // scrollbar thumb / splitter drops its accent color),
    // so force a repaint even when the release point is not
    // over a splitter. Without this the cleared
    // `scrollbar_drag` / `dragging_flags` would not be
    // painted and the "active" highlight would stay stuck
    // on screen (the `Up` only set `dirty` via hover, which
    // is false over a bare scrollbar).
    let was_dragging = state.scrollbar_drag.is_some()
        || state.splitter_hover.results_col_resize_drag.is_some()
        || state.splitter_hover.dragging_flags().iter().any(|&f| f);
    clear_active_drags(state);
    *dirty |= was_dragging;
    // Re-evaluate hover after drag end — the cursor may
    // still be over a splitter.
    if update_splitter_hover(state, mouse.column, mouse.row, size) {
        *dirty = true;
    }
    Ok(())
}

/// Hover with no button held: end a drag whose `Up` was missed (released
/// outside the window), then re-evaluate the splitter hover highlight.
pub(crate) fn handle_moved(
    mouse: &MouseEvent,
    size: Size,
    state: &mut AppState,
    dirty: &mut bool,
    splitter_drag: &mut Option<SplitterDrag>,
) -> anyhow::Result<()> {
    // A `Moved` event means *no* mouse button is pressed
    // (crossterm reports button-held motion as `Drag`). If
    // a drag is still recorded here, its `Up` was missed —
    // e.g. the button was released *outside* the terminal
    // window, so the terminal never delivered the release
    // and the highlight would stay stuck. End the drag now
    // and force a repaint so the accent color resets. Any
    // real (button-held) drag produces `Drag` events, never
    // `Moved`, so this can never fire mid-drag.
    if state.scrollbar_drag.is_some()
        || splitter_drag.is_some()
        || state.splitter_hover.results_col_resize_drag.is_some()
        || state.splitter_hover.dragging_flags().iter().any(|&f| f)
    {
        *splitter_drag = None;
        clear_active_drags(state);
        *dirty = true;
    }
    // Hover is a continuous gesture: only request a
    // redraw when some hover bit actually toggles,
    // avoiding wasteful repaints on every mouse pixel.
    if update_splitter_hover(state, mouse.column, mouse.row, size) {
        *dirty = true;
    }
    Ok(())
}

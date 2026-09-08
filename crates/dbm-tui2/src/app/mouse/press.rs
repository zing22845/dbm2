//! Button-press handling.
//!
//! `handle_down` is the largest branch: it decides whether the press grabs a
//! scrollbar or splitter, moves focus, or becomes the focused pane's click. The
//! two modal handlers cover presses on the confirm dialogs, which are hit-tested
//! before the app body because they render on top of it.

use std::time::Instant;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{
    Size, {Position, Rect},
};
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::effect::EffectRunner;
use crate::app_shell::pane::Pane;
use crate::common::layout::pane_scrollbar::{ActiveScrollbar, ScrollbarDrag};
use crate::features::header::msg::{HeaderMessage, HeaderMsg};

use super::click::{
    discover_subpane_for_click, explorer_child_areas, explorer_click_hits_row,
    explorer_pane_for_click, explorer_row_click_msgs, is_explorer_toggle_click, sql_click_msgs,
};
use super::splitter::{SplitterDrag, resolve_splitter_drag};
use crate::app::geometry::{
    app_explorer_rect, iw_body_area_for_hit, sql_picker_area_for_hit, sql_tab_area_for_hit,
    workspace_rect_for_hit,
};
use crate::app::round::process_message_round;

/// A left press while a confirm modal is open: hit-test the Yes/No
/// buttons against the rendered popup.
pub(crate) fn handle_confirm_modal_click(
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
) -> anyhow::Result<()> {
    let footer_h = crate::app::geometry::global_footer_height(state, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    // The confirm modal renders over the live workspace
    // (app_body_layout), so hit-test against the same.
    if let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state) {
        let popup = crate::common::layout::modal::confirm_popup_rect(
            workspace,
            crate::common::view::modal::confirm_body_rows(state.modal.as_ref().unwrap()),
        );
        let buttons = crate::common::view::modal::confirm_buttons(popup);
        let msg = if buttons.yes_rect.contains(point) {
            crate::app::confirm::confirm_yes_msg(state.modal.as_ref().unwrap(), state)
        } else if buttons.no_rect.contains(point) {
            Some(AppMsg::CloseModal)
        } else {
            None
        };
        if let Some(msg) = msg {
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }
    Ok(())
}

/// A left press while a results picker (rows-per-page / page jump) is open:
/// clicking a row-limit preset applies that limit, a click outside the popup
/// closes it, and a click elsewhere inside the popup keeps it open (mirroring
/// the original dbm's `handle_results_row_limit_mouse` /
/// `handle_results_page_input_mouse`).
pub(crate) fn handle_results_picker_modal_click(
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
) -> anyhow::Result<()> {
    use crate::app::state::ModalKind;
    let footer_h = crate::app::geometry::global_footer_height(state, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    // The picker popups float inside the workspace (see `app::view`), so they
    // are hit-tested against the same workspace region.
    let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state) else {
        return Ok(());
    };
    let Some(picker) = crate::app::geometry::results_picker_popup(state, workspace) else {
        return Ok(());
    };
    let msg = if picker.popup.contains(point) {
        // A click on a row-limit preset row applies it; anywhere else inside
        // the popup (page input / hint row) keeps the picker open.
        if let Some(ModalKind::ResultsRowLimitPicker { limits, .. }) = &state.modal {
            let limit = picker
                .preset_rows
                .iter()
                .enumerate()
                .find(|(_, rect)| rect.contains(point))
                .and_then(|(idx, _)| limits.get(idx).copied());
            limit.map(|limit| {
                use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
                use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
                use crate::features::sql_workspace::sql_tab::results::msg::{
                    ResultsMessage, ResultsMsg,
                };
                let Some(tab_id) = state.sql.sql_tab.active_tab().map(|t| t.session.id) else {
                    return AppMsg::CloseModal;
                };
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Results {
                        tab_id,
                        msg: ResultsMsg::Message(ResultsMessage::SetRowLimit { limit }),
                    },
                ))))
            })
        } else {
            None
        }
    } else {
        // Clicking outside the anchored popup closes it (like Esc).
        Some(AppMsg::CloseModal)
    };
    if let Some(msg) = msg {
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }
    Ok(())
}

/// A left press on discover's close-confirmation dialog.
pub(crate) fn handle_discover_close_confirm_click(
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
) -> anyhow::Result<()> {
    let footer_h = crate::app::geometry::global_footer_height(state, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    // The workspace region matches the render exactly
    // (using the live Explorer splitter width, not a
    // hard-coded 20%), and the close-confirm popup is
    // centered inside the discover overlay (75% of the
    // workspace), matching `render_modal_popup` — so the
    // popup the mouse hits is the same one that is drawn.
    let msg = if let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state) {
        // The close-confirm popup is centered inside the
        // discover overlay (75% of the workspace), matching
        // `render_modal_popup`, so the popup the mouse hits
        // is the same one that is drawn.
        let discover_popup = crate::common::layout::modal::popup_rect(workspace, 75, 75);
        // Discover's close-confirm body is a single line.
        let popup = crate::common::layout::modal::confirm_popup_rect(discover_popup, 1);
        let buttons = crate::common::view::modal::confirm_buttons(popup);
        if buttons.yes_rect.contains(point) {
            Some(AppMsg::Discover(
                crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::Close,
                ),
            ))
        } else if buttons.no_rect.contains(point) {
            Some(AppMsg::Discover(
                crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::CancelClose,
                ),
            ))
        } else {
            None
        }
    } else {
        None
    };
    if let Some(msg) = msg {
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }
    Ok(())
}

/// A left press on the app body: focus the clicked pane, grab a scrollbar
/// or splitter, or dispatch the pane's own click action.
// Four of these are the shared dispatch plumbing (terminal, state, the message
// round) and three are the gesture outputs it owns. Bundling them into a
// context struct is the natural follow-up once the click routing is split out
// further; a flat signature keeps the moved body readable until then.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_down(
    mouse: &MouseEvent,
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
    splitter_drag: &mut Option<SplitterDrag>,
    last_click: &mut Option<(Position, Instant)>,
) -> anyhow::Result<()> {
    // Double-click detection: a second press at the same
    // cell within the window is treated as a double click.
    let is_double_click = last_click
        .as_ref()
        .is_some_and(|(p, t)| *p == point && t.elapsed() < std::time::Duration::from_millis(400));
    *last_click = Some((point, std::time::Instant::now()));

    // Map the click to a focus pane by region. The layout
    // mirrors `app/view.rs`: header (top 3 rows), explorer
    // (left 20% of the body), workspace (right 80%).
    let footer_h = crate::app::geometry::global_footer_height(state, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    // Use the live Explorer column width (not a hard-coded
    // 20%) so clicks/hits inside the resizable Explorer
    // agree with the rendered splitter.
    let explorer_w = app_explorer_rect(size, body_top, body_h, state)
        .map(|r| r.width)
        .unwrap_or((size.width.saturating_mul(2) / 10).max(1));
    // Starting a drag on the discover targets/results
    // splitter (only while discover owns focus) begins a
    // resize gesture. It is checked before sub-pane
    // switching below. The body uses the same live
    // workspace/popup/engine geometry the render uses.
    if matches!(state.focus, Pane::Discover(_))
        && !state.discover.close_confirm
        && let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
        && let discover_popup = crate::common::layout::modal::popup_rect(workspace, 75, 75)
        && let body =
            crate::features::discover::layout::discover_body_area(discover_popup, &state.discover)
        && body.height >= 3
        && let layout = crate::features::discover::splitter::layout::discover_body_layout(
            body,
            state.discover.splitter.targets_height,
        )
        && crate::features::discover::splitter::view::splitter_at(&layout, point.x, point.y)
    {
        *splitter_drag = Some(SplitterDrag::Discover);
        state.splitter_hover.discover_splitter_drag = true;
        tracing::debug!("discover targets/results splitter drag started");
    }

    // A click outside the open context picker closes it
    // (mirroring the original dbm), regardless of which
    // pane the click lands in. The SQL workspace click
    // handler also closes for clicks in its body, so this
    // only fires for clicks outside the picker area.
    if let Some(picker_area) = sql_picker_area_for_hit(size, state)
        && !picker_area.contains(point)
    {
        let msg = sql_click_msgs(
            &state.sql.sql_tab,
            crate::features::sql_workspace::sql_tab::input::SqlClickAction::CloseContextPicker,
        );
        for m in msg {
            let result = process_message_round(effect_runner, action_rx, m, state);
            *dirty |= result.dirty;
        }
    }

    // Map the click to a focus pane by region. While the
    // discover parent pane owns focus, any attempt to
    // move focus away is rejected by the shell's
    // FocusChanged handler (update.rs), so clicks outside
    // the discover popup stay in discover. Clicks inside
    // the popup switch discover sub-panes below.
    let target_pane = if mouse.row < body_top {
        Some(Pane::Header)
    } else if mouse.row >= body_top + body_h {
        None
    } else if mouse.column < explorer_w {
        // The explorer is a parent pane hosting the
        // instances (top) and objects (bottom) trees;
        // map the click row to the matching sub-pane
        // so mouse navigation agrees with Ctrl+j/k.
        // Exception: clicks on the explorer instances/
        // objects splitter keep the current sub-pane —
        // the splitter is a drag handle, not a clickable
        // region.
        let current_sub = match state.focus {
            Pane::Explorer(sub) => Some(sub),
            _ => None,
        };
        let hit_splitter = if let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
            && explorer.height >= 3
        {
            let inner = Rect::new(
                explorer.x.saturating_add(1),
                explorer.y.saturating_add(1),
                explorer.width.saturating_sub(2),
                explorer.height.saturating_sub(2),
            );
            let layout = crate::features::explorer::splitter::layout::explorer_body_layout(
                inner,
                state.explorer.splitter.instances_height,
            );
            crate::features::explorer::splitter::view::splitter_at(&layout, mouse.column, mouse.row)
        } else {
            false
        };
        if hit_splitter && let Some(sub) = current_sub {
            Some(Pane::Explorer(sub))
        } else {
            Some(Pane::Explorer(explorer_pane_for_click(
                mouse.row,
                body_top,
                body_h,
                state.explorer.splitter.instances_height,
            )))
        }
    } else if !state.instance_workspace_open() {
        Some(Pane::SQLWorkspace)
    } else {
        // The workspace region holds the instance pane
        // (outer border included). A click on its tab bar
        // switches the active sub-pane (overview /
        // connections); a click elsewhere keeps the
        // current sub-pane (default when not focused).
        let ws = Rect::new(
            explorer_w,
            body_top,
            size.width.saturating_sub(explorer_w),
            body_h,
        );
        let current = match state.focus {
            Pane::InstanceWorkspace(p) => p,
            _ => crate::app_shell::nav::IwPane::default(),
        };
        Some(Pane::InstanceWorkspace(
            crate::features::instance_workspace::view::iw_tab_at(ws, mouse.column, mouse.row)
                .unwrap_or(current),
        ))
    };
    tracing::debug!(
        point = ?point,
        target_pane = ?target_pane,
        current_focus = ?state.focus,
        "mouse click pane mapping"
    );
    if let Some(pane) = target_pane.filter(|z| *z != state.focus) {
        let msg = AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged { pane });
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }
    press_explorer_content(
        mouse,
        point,
        size,
        body_top,
        body_h,
        explorer_w,
        is_double_click,
        state,
        effect_runner,
        action_rx,
        dirty,
    )?;
    press_sql(
        mouse,
        size,
        is_double_click,
        target_pane,
        state,
        effect_runner,
        action_rx,
        dirty,
    )?;
    press_discover(
        mouse,
        point,
        size,
        body_top,
        body_h,
        is_double_click,
        state,
        effect_runner,
        action_rx,
        dirty,
        splitter_drag,
    )?;
    press_explorer_scrollbars(
        mouse,
        point,
        size,
        body_top,
        body_h,
        state,
        effect_runner,
        action_rx,
        dirty,
    )?;
    press_iw(
        mouse,
        point,
        size,
        is_double_click,
        state,
        effect_runner,
        action_rx,
        dirty,
    )?;
    // Left-click on the header `Discover` button activates
    // it, in addition to moving focus to the header.
    let header_area = Rect::new(0, 0, size.width, 3);
    let button_rect = crate::features::header::view::discover_button_rect(header_area);
    let clicked = button_rect.is_some_and(|r| r.contains(point));
    tracing::debug!(clicked, "header button click resolved");
    if clicked {
        // Clicking the header button is an explicit user
        // intent: move focus to the Header pane (via the
        // shell message), then dispatch Activate. Both go
        // through `update` so every state change flows
        // through the single state-transition channel.
        let result = process_message_round(
            effect_runner,
            action_rx,
            AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged { pane: Pane::Header }),
            state,
        );
        *dirty |= result.dirty;
        let result = process_message_round(
            effect_runner,
            action_rx,
            AppMsg::Header(HeaderMsg::Message(HeaderMessage::Activate)),
            state,
        );
        *dirty |= result.dirty;
        tracing::debug!("dispatching HeaderMessage::Activate");
    }

    // Resolve the press to the *single* splitter being
    // dragged (see `resolve_splitter_drag`). The
    // discover splitter is resolved earlier — before the
    // click re-maps focus — and already holds the slot
    // when it hit, which is why this is skipped then.
    if splitter_drag.is_none()
        && let Some(target) = resolve_splitter_drag(state, size, point.x, point.y)
    {
        *splitter_drag = Some(target);
        use crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter;
        let sh = &mut state.splitter_hover;
        match target {
            SplitterDrag::App => sh.app_splitter_drag = true,
            SplitterDrag::Explorer => sh.explorer_splitter_drag = true,
            SplitterDrag::Discover => sh.discover_splitter_drag = true,
            SplitterDrag::Sql(_, SqlSplitter::EditorResults) => sh.sql_editor_results_drag = true,
            SplitterDrag::Sql(_, SqlSplitter::EditorHistory) => sh.sql_editor_history_drag = true,
            SplitterDrag::Sql(_, SqlSplitter::HistoryDetail) => sh.sql_history_detail_drag = true,
            SplitterDrag::Sql(_, SqlSplitter::ResultsDetail) => sh.sql_results_detail_drag = true,
        }
        tracing::debug!(?target, "splitter drag started");
    }
    Ok(())
}

/// Explorer content clicks: a single click moves the cursor to the
/// clicked tree row, a double click activates the node. Scrollbar-track
/// clicks are excluded so they start a drag instead.
// Takes the shared press plumbing; a context struct is a possible
// follow-up if the list keeps growing.
#[allow(clippy::too_many_arguments)]
fn press_explorer_content(
    mouse: &MouseEvent,
    point: Position,
    size: Size,
    body_top: u16,
    body_h: u16,
    explorer_w: u16,
    is_double_click: bool,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    dirty: &mut bool,
) -> anyhow::Result<()> {
    // A single click inside the explorer's instances or
    // objects tree moves the selection (cursor) to the
    // clicked row — UNLESS the click lands on a v_scrollbar
    // or h_scrollbar track, which must NOT also move the
    // cursor to that row.
    let click_hits_explorer_scrollbar = {
        let mut hits = false;
        if let Some(explorer) = app_explorer_rect(size, body_top, body_h, state) {
            let (inst_area, objs_area) =
                explorer_child_areas(explorer, state.explorer.splitter.instances_height);
            use crate::features::explorer::instances as inst_mod;
            use crate::features::explorer::objects as obj_mod;
            if inst_area.contains(point) {
                hits |= inst_mod::view::v_scrollbar_hit(
                    inst_area,
                    &state.explorer.instances,
                    mouse.column,
                    mouse.row,
                )
                .is_some();
                hits |= inst_mod::view::h_scrollbar_hit(
                    inst_area,
                    &state.explorer.instances,
                    mouse.column,
                    mouse.row,
                )
                .is_some();
            }
            if objs_area.contains(point) {
                hits |= obj_mod::view::v_scrollbar_hit(
                    objs_area,
                    &state.explorer.objects,
                    mouse.column,
                    mouse.row,
                )
                .is_some();
                hits |= obj_mod::view::h_scrollbar_hit(
                    objs_area,
                    &state.explorer.objects,
                    mouse.column,
                    mouse.row,
                )
                .is_some();
            }
        }
        hits
    };
    if mouse.column < explorer_w
        && !click_hits_explorer_scrollbar
        && let Some(click_msgs) =
            explorer_row_click_msgs(size, body_top, body_h, mouse.column, mouse.row, state)
    {
        for click_msg in click_msgs {
            let result = process_message_round(effect_runner, action_rx, click_msg, state);
            *dirty |= result.dirty;
        }
    }

    // A double click in the explorer activates the node
    // (Select), like pressing Enter on it — except on an
    // expand/collapse marker (toggle only) or on blank
    // space (no node: do nothing, not act on the cursor).
    if is_double_click
        && mouse.column < explorer_w
        && !click_hits_explorer_scrollbar
        && explorer_click_hits_row(explorer_w, body_top, body_h, mouse.row, state)
        && !is_explorer_toggle_click(explorer_w, body_top, body_h, mouse.column, mouse.row, state)
    {
        let select = AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
            match explorer_pane_for_click(
                mouse.row,
                body_top,
                body_h,
                state.explorer.splitter.instances_height,
            ) {
                crate::app_shell::nav::ExplorerPane::Instances => {
                    crate::features::explorer::msg::ExplorerMessage::Instances(
                        crate::features::explorer::instances::msg::InstancesMsg::Message(
                            crate::features::explorer::instances::msg::InstancesMessage::Select,
                        ),
                    )
                }
                crate::app_shell::nav::ExplorerPane::Objects => {
                    crate::features::explorer::msg::ExplorerMessage::Objects(
                        crate::features::explorer::objects::msg::ObjectsMsg::Message(
                            crate::features::explorer::objects::msg::ObjectsMessage::Select,
                        ),
                    )
                }
            },
        ));
        let result = process_message_round(effect_runner, action_rx, select, state);
        *dirty |= result.dirty;
    }
    Ok(())
}

/// SQL workspace clicks: route to a sub-pane, activate the clicked tab,
/// or start a scrollbar / column-resize drag.
// Takes the shared press plumbing; a context struct is a possible
// follow-up if the list keeps growing.
#[allow(clippy::too_many_arguments)]
fn press_sql(
    mouse: &MouseEvent,
    size: Size,
    is_double_click: bool,
    target_pane: Option<Pane>,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    dirty: &mut bool,
) -> anyhow::Result<()> {
    // A click inside the SQL workspace also routes to a
    // sub-pane (editor/history/results) or activates the
    // clicked tab, mirroring the mouse support of the
    // original dbm.
    if matches!(target_pane, Some(Pane::SQLWorkspace))
        && let Some(tab_area) = sql_tab_area_for_hit(size, state)
        && let Some(action) = crate::features::sql_workspace::sql_tab::input::sql_workspace_click(
            &state.sql.sql_tab,
            tab_area,
            mouse.column,
            mouse.row,
            is_double_click,
        )
    {
        // If the click was on the h_scrollbar, start dragging.
        if let crate::features::sql_workspace::sql_tab::input::SqlClickAction::HistoryHScrollbar {
            track_x,
            x: _,
            max_scroll,
            viewport_width,
        } = action
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::HistoryH,
                track_start: track_x,
                viewport_len: viewport_width,
                max_scroll,
            });
        }
        // If the click was on the v_scrollbar, start dragging.
        if let crate::features::sql_workspace::sql_tab::input::SqlClickAction::HistoryVScrollbar {
            track_y,
            y: _,
            max_scroll,
            viewport_height,
        } = action
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::HistoryV,
                track_start: track_y,
                viewport_len: viewport_height,
                max_scroll,
            });
        }
        // If the click was on the editor body's v_scrollbar, start dragging.
        if let crate::features::sql_workspace::sql_tab::input::SqlClickAction::EditorVScrollbar {
            track_y,
            y: _,
            max_scroll,
            viewport_height,
        } = action
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::SqlV,
                track_start: track_y,
                viewport_len: viewport_height,
                max_scroll,
            });
        }
        // If the click was on the results list h_scrollbar, start dragging.
        if let crate::features::sql_workspace::sql_tab::input::SqlClickAction::ResultsHScrollbar {
            track_x,
            x: _,
            max_scroll,
            viewport_width,
        } = action
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::ResultsH,
                track_start: track_x,
                viewport_len: viewport_width,
                max_scroll,
            });
        }
        // If the click was on the results list v_scrollbar, start dragging.
        if let crate::features::sql_workspace::sql_tab::input::SqlClickAction::ResultsVScrollbar {
            track_y,
            y: _,
            max_scroll,
            viewport_height,
        } = action
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::ResultsV,
                track_start: track_y,
                viewport_len: viewport_height,
                max_scroll,
            });
        }
        // A single click on a results column header
        // splitter begins a column-width resize drag.
        if let crate::features::sql_workspace::sql_tab::input::SqlClickAction::ResultsColResize {
            col,
        } = action
        {
            state.splitter_hover.results_col_resize_drag = Some(col);
            tracing::debug!(col, "results column resize drag started");
        }
        // A press on the editor's *text* (not its scrollbar / header / footer
        // rows, which the action above already claims) begins mouse text
        // selection: arm the capture so subsequent Drag/Up events route back to
        // the editor, and let edtui place the cursor / clear the old selection.
        let text_click = matches!(
            action,
            crate::features::sql_workspace::sql_tab::input::SqlClickAction::FocusSubPane(
                crate::features::sql_workspace::sql_tab::state::SqlFocus::Editor
            )
        ) && state.focus == Pane::SQLWorkspace
            && state
                .sql_editor_mouse_area
                .is_some_and(|area| area.contains(Position::new(mouse.column, mouse.row)));
        if text_click {
            state.sql_editor_selecting = true;
        }
        // The same for the results detail cell editor: once it holds focus, a
        // press on its text begins mouse selection there (click to place the
        // caret, drag to select, double-click to select the word). The hit
        // region only exists while that editor is actually focused.
        let detail_text_click = matches!(
            action,
            crate::features::sql_workspace::sql_tab::input::SqlClickAction::FocusSubPane(
                crate::features::sql_workspace::sql_tab::state::SqlFocus::Results
            )
        ) && state.focus == Pane::SQLWorkspace
            && state
                .results_detail_mouse_area
                .is_some_and(|area| area.contains(Position::new(mouse.column, mouse.row)));
        if detail_text_click {
            state.results_detail_selecting = true;
        }
        let mut msgs = sql_click_msgs(&state.sql.sql_tab, action);
        if text_click
            && let Some(msg) = super::editor_gesture::editor_gesture_msg(
                state,
                MouseEventKind::Down(MouseButton::Left),
                mouse.column,
                mouse.row,
                is_double_click,
            )
        {
            msgs.push(msg);
        }
        if detail_text_click
            && let Some(msg) = super::editor_gesture::detail_gesture_msg(
                state,
                MouseEventKind::Down(MouseButton::Left),
                mouse.column,
                mouse.row,
                is_double_click,
            )
        {
            msgs.push(msg);
        }
        for msg in msgs {
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }
    Ok(())
}

/// Discover popup clicks: switch sub-panes, select targets rows/cells and
/// results rows, and start their scrollbar drags.
// Takes the shared press plumbing; a context struct is a possible
// follow-up if the list keeps growing.
#[allow(clippy::too_many_arguments)]
fn press_discover(
    mouse: &MouseEvent,
    point: Position,
    size: Size,
    body_top: u16,
    body_h: u16,
    is_double_click: bool,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    dirty: &mut bool,
    splitter_drag: &mut Option<SplitterDrag>,
) -> anyhow::Result<()> {
    // Inside the discover popup: map the click's row to a
    // discover child pane and switch focus to it. This is
    // suppressed while the close-confirmation dialog is
    // open, so clicks on the discover pane behind it are
    // ignored (only Enter/Esc operate on the dialog).
    if let Pane::Discover(sub) = state.focus
        && !state.discover.close_confirm
        && splitter_drag.is_none()
        && let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
        && let Some(next) =
            discover_subpane_for_click(mouse.column, mouse.row, workspace, &state.discover)
        && next != sub
    {
        let msg = AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
            crate::features::discover::msg::DiscoverMessage::Focus(next),
        ));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
        tracing::debug!(to = ?next, "mouse click switched discover sub-pane");
    }

    // Inside the discover popup's targets pane: row/cell
    // selection on single click, begin-edit on double
    // click (matching original dbm behavior).
    if let Pane::Discover(sub) = state.focus
        && !state.discover.close_confirm
        && splitter_drag.is_none()
        && matches!(sub, crate::app_shell::nav::DiscoverPane::Targets)
        && let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
        && let discover_popup = crate::common::layout::modal::popup_rect(workspace, 75, 75)
        && let body =
            crate::features::discover::layout::discover_body_area(discover_popup, &state.discover)
        && !body.is_empty()
        && let layout = crate::features::discover::splitter::layout::discover_body_layout(
            body,
            state.discover.splitter.targets_height,
        )
        && layout.targets.contains(point)
    {
        use crate::features::discover::targets::{
            msg::{TargetsMessage, TargetsMsg},
            view,
        };
        // 1. Scrollbar hit-test first — scrollbar sits
        //    outside content area so hit_test misses it.
        //    If we hit the v_scrollbar, start a drag.
        if let Some(si) = view::v_scrollbar_hit(
            layout.targets,
            &state.discover.targets,
            mouse.column,
            mouse.row,
        ) {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::DiscoverTargetsV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::layout::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg =
                crate::app::AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::Targets(TargetsMsg::Message(
                        TargetsMessage::SetVScroll {
                            position: new_scroll,
                        },
                    )),
                ));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
        // 2. Content-area click: select row / cell.
        else if let Some((row, col)) = view::hit_test(
            layout.targets,
            &state.discover.targets,
            mouse.column,
            mouse.row,
        ) {
            let msg = if is_double_click {
                if let Some(col) = col {
                    AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                        crate::features::discover::msg::DiscoverMessage::Targets(
                            TargetsMsg::Message(TargetsMessage::BeginEditCell { row, col }),
                        ),
                    ))
                } else {
                    AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                        crate::features::discover::msg::DiscoverMessage::Targets(
                            TargetsMsg::Message(TargetsMessage::SelectRow { row }),
                        ),
                    ))
                }
            } else if let Some(col) = col {
                AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::Targets(TargetsMsg::Message(
                        TargetsMessage::SelectCell { row, col },
                    )),
                ))
            } else {
                AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::Targets(TargetsMsg::Message(
                        TargetsMessage::SelectRow { row },
                    )),
                ))
            };
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Discover results pane click handler — v_scrollbar
    // hit-test first, then content row click (select +
    // toggle). Much simpler than targets (no cells).
    if let Pane::Discover(sub) = state.focus
        && !state.discover.close_confirm
        && splitter_drag.is_none()
        && matches!(sub, crate::app_shell::nav::DiscoverPane::Results)
        && let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
        && let discover_popup = crate::common::layout::modal::popup_rect(workspace, 75, 75)
        && let body =
            crate::features::discover::layout::discover_body_area(discover_popup, &state.discover)
        && !body.is_empty()
        && let layout = crate::features::discover::splitter::layout::discover_body_layout(
            body,
            state.discover.splitter.targets_height,
        )
        && layout.results.contains(point)
    {
        use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
        use crate::features::discover::results::{
            msg::{ResultsMessage, ResultsMsg},
            view,
        };
        // 1. Scrollbar hit-test first.
        if let Some(si) = view::v_scrollbar_hit(
            layout.results,
            &state.discover.results,
            mouse.column,
            mouse.row,
        ) {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::DiscoverResultsV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::layout::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
                ResultsMsg::Message(ResultsMessage::SetVScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
        // 2. Content-area click: select row. A single
        //    click moves cursor + toggles selection (the
        //    discover results pane's primary interaction).
        else if let Some(row) = view::hit_test(
            layout.results,
            &state.discover.results,
            mouse.column,
            mouse.row,
        ) {
            // Move cursor to clicked row first (also
            // clears scroll_locked), then toggle selection.
            let focus_msg = AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
                ResultsMsg::Message(ResultsMessage::SetCursor { row }),
            )));
            let r1 = process_message_round(effect_runner, action_rx, focus_msg, state);
            *dirty |= r1.dirty;
            let toggle_msg = AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
                ResultsMsg::Message(ResultsMessage::ToggleSelect),
            )));
            let r2 = process_message_round(effect_runner, action_rx, toggle_msg, state);
            *dirty |= r2.dirty;
        }
    }
    Ok(())
}

/// Explorer objects/instances scrollbar drags (hit-tested before the
/// generic row jump so a track click starts a drag instead).
// Takes the shared press plumbing; a context struct is a possible
// follow-up if the list keeps growing.
#[allow(clippy::too_many_arguments)]
fn press_explorer_scrollbars(
    mouse: &MouseEvent,
    point: Position,
    size: Size,
    body_top: u16,
    body_h: u16,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    dirty: &mut bool,
) -> anyhow::Result<()> {
    // Explorer objects v_scrollbar hit-test — runs BEFORE
    // the generic explorer_row_click_msgs jump so a
    // scrollbar click starts a drag instead of moving
    // the cursor to that row.
    if let Pane::Explorer(_) = state.focus
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && let (_inst_area, objs_area) =
            explorer_child_areas(explorer, state.explorer.splitter.instances_height)
        && objs_area.contains(point)
    {
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        use crate::features::explorer::objects::{msg::*, view};
        if let Some(si) =
            view::v_scrollbar_hit(objs_area, &state.explorer.objects, mouse.column, mouse.row)
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::ObjectsV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::layout::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
                ObjectsMsg::Message(ObjectsMessage::SetVScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Explorer instances v_scrollbar hit-test — before
    // explorer_row_click_msgs so a scrollbar click
    // starts a drag instead of moving the cursor.
    if let Pane::Explorer(_) = state.focus
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && let (inst_area, _objs_area) =
            explorer_child_areas(explorer, state.explorer.splitter.instances_height)
        && inst_area.contains(point)
    {
        use crate::features::explorer::instances::{msg::*, view};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        if let Some(si) = view::v_scrollbar_hit(
            inst_area,
            &state.explorer.instances,
            mouse.column,
            mouse.row,
        ) {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::TreeV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::layout::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                InstancesMsg::Message(InstancesMessage::SetVScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Explorer objects h_scrollbar hit-test.
    if let Pane::Explorer(_) = state.focus
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && let (_inst_area, objs_area) =
            explorer_child_areas(explorer, state.explorer.splitter.instances_height)
        && objs_area.contains(point)
    {
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        use crate::features::explorer::objects::{msg::*, view};
        if let Some(si) =
            view::h_scrollbar_hit(objs_area, &state.explorer.objects, mouse.column, mouse.row)
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::ObjectsH,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::layout::pane_scrollbar::scroll_offset_from_track(
                mouse.column,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
                ObjectsMsg::Message(ObjectsMessage::SetHScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Explorer instances h_scrollbar hit-test.
    if let Pane::Explorer(_) = state.focus
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && let (inst_area, _objs_area) =
            explorer_child_areas(explorer, state.explorer.splitter.instances_height)
        && inst_area.contains(point)
    {
        use crate::features::explorer::instances::{msg::*, view};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        if let Some(si) = view::h_scrollbar_hit(
            inst_area,
            &state.explorer.instances,
            mouse.column,
            mouse.row,
        ) {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::TreeH,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::layout::pane_scrollbar::scroll_offset_from_track(
                mouse.column,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                InstancesMsg::Message(InstancesMessage::SetHScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }
    Ok(())
}

/// Instance-workspace clicks: connections list / edit form and overview
/// rows, plus their scrollbar drags.
// Takes the shared press plumbing; a context struct is a possible
// follow-up if the list keeps growing.
#[allow(clippy::too_many_arguments)]
fn press_iw(
    mouse: &MouseEvent,
    point: Position,
    size: Size,
    is_double_click: bool,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    dirty: &mut bool,
) -> anyhow::Result<()> {
    // IW connections v_scrollbar hit-test.
    if let Some(body) = iw_body_area_for_hit(size, state)
        && body.contains(point)
        && matches!(state.iw.pane, crate::app_shell::nav::IwPane::Connections)
    {
        use crate::features::instance_workspace::connections::{msg::*, view};
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        if let Some(si) =
            view::v_scrollbar_hit(body, &state.iw.connections, mouse.column, mouse.row)
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::ConnectionsV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::layout::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::SetVScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // IW connections list click: a single click moves the
    // cursor to the clicked row; a double click opens the
    // edit form for it. Excludes clicks on the v_scrollbar
    // track (which start a drag instead) and clicks while
    // the add/edit form is open (the popup owns the input).
    if let Some(body) = iw_body_area_for_hit(size, state)
        && body.contains(point)
        && matches!(state.iw.pane, crate::app_shell::nav::IwPane::Connections)
        && state.iw.connections.form.is_none()
    {
        use crate::features::instance_workspace::connections::{msg::*, view};
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        let on_scrollbar =
            view::v_scrollbar_hit(body, &state.iw.connections, mouse.column, mouse.row).is_some();
        if !on_scrollbar && let Some(row) = view::row_at(body, &state.iw.connections, mouse.row) {
            let msg = if is_double_click {
                // Double click edits the connection, like
                // pressing `i` on the row (BeginEdit).
                ConnectionsMessage::BeginEdit
            } else {
                // Single click selects the row (JumpTo).
                ConnectionsMessage::JumpTo { row }
            };
            let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                ConnectionsMsg::Message(msg),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // IW connections form click: when the add/edit form
    // popup is open, a single click on a field line
    // selects that field and a double click enters insert
    // mode on it (matching the original dbm's form field
    // click handling). Clicks outside a field line are
    // ignored.
    if let Some(body) = iw_body_area_for_hit(size, state)
        && body.contains(point)
        && matches!(state.iw.pane, crate::app_shell::nav::IwPane::Connections)
        && state.iw.connections.form.is_some()
    {
        use crate::features::instance_workspace::connections::{msg::*, view};
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        if let Some(field) = view::form_field_at(body, mouse.column, mouse.row) {
            let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::FormClick {
                    field,
                    is_double: is_double_click,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // IW overview v_scrollbar hit-test.
    if let Some(body) = iw_body_area_for_hit(size, state)
        && body.contains(point)
        && matches!(state.iw.pane, crate::app_shell::nav::IwPane::Overview)
    {
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        use crate::features::instance_workspace::overview::{msg::*, view};
        // overview_rows conn_count parameter for viewport calc;
        // 0 is fine for geometry (only affects row content).
        if let Some(si) =
            view::v_scrollbar_hit(body, &state.iw.overview, 0, mouse.column, mouse.row)
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::OverviewV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::layout::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Overview(OverviewMsg::Message(
                OverviewMessage::SetVScroll {
                    position: new_scroll,
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // IW overview: clicking a row moves the overview
    // cursor to it (like the connections list's single
    // click). The overview has no double-click action.
    if let Some(body) = iw_body_area_for_hit(size, state)
        && body.contains(point)
        && matches!(state.iw.pane, crate::app_shell::nav::IwPane::Overview)
    {
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        use crate::features::instance_workspace::overview::{msg::*, view};
        let on_scrollbar =
            view::v_scrollbar_hit(body, &state.iw.overview, 0, mouse.column, mouse.row).is_some();
        if !on_scrollbar && let Some(row) = view::row_at(body, &state.iw.overview, 0, mouse.row) {
            let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Overview(OverviewMsg::Message(
                OverviewMessage::SetCursor { index: row },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }
    Ok(())
}

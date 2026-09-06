//! Top-level rendering. This module is purely presentational: it lays out the
//! shell regions and delegates to each feature's `view::render`. It contains no
//! message scheduling, intent routing or event-loop logic; those live in
//! `crate::app::loop_mod`.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::state::{AppState, ModalKind};
use crate::app_shell::pane::Pane;
use crate::features::app_splitter::view as splitter_view;
use crate::features::discover::view as discover_view;
use crate::features::explorer::view as explorer_view;
use crate::features::global_footer::layout as footer_layout;
use crate::features::global_footer::view as footer_view;
use crate::features::header::view as header_view;
use crate::features::instance_workspace::view as iw_view;
use crate::features::perf_monitor::view as perf_view;
use crate::features::sql_workspace::view as sql_view;

/// Top-level render: lays out the shell regions and delegates to each
/// feature's `view::render`. Returns the editor's hardware cursor (when the
/// active SQL tab's editor sub-pane holds focus) so the run loop can place the
/// terminal caret.
pub fn render(
    frame: &mut ratatui::Frame,
    state: &AppState,
) -> (
    Option<crate::common::editor::EditorHardwareCursor>,
    Option<crate::features::discover::targets::view::TargetsLayoutInfo>,
    Option<usize>,
    Option<usize>,
) {
    // The footer height is dynamic: one line of hints plus the (wrapped) status
    // line when present.
    let footer_h = footer_layout::footer_height(&state.footer, frame.area().width);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),        // header (title bar with borders)
            Constraint::Min(0),           // body (explorer + workspace)
            Constraint::Length(footer_h), // footer
        ])
        .split(frame.area());

    // The body is a horizontal split: Explorer (left) + a resizable vertical
    // splitter + the workspace region (right). The splitter width is owned by
    // the app-level `splitter` feature.
    let body = crate::features::app_splitter::layout::app_body_layout(
        chunks[1],
        state.splitter.explorer_pane_width,
    );

    // The workspace region holds only the SQL view (main). The performance
    // readout moved into the footer row (right-aligned) to save vertical space.
    let workspace = body.workspace;

    // Pass whether each region owns the shell focus so the views can highlight
    // the active pane's border (otherwise focus changes are invisible).
    let header_focused = matches!(state.focus, Pane::Header);
    let explorer_focused = matches!(state.focus, Pane::Explorer(_));
    let workspace_focused = matches!(state.focus, Pane::SQLWorkspace)
        || matches!(state.focus, Pane::InstanceWorkspace(_));
    header_view::render(
        frame,
        &state.theme,
        chunks[0],
        &state.header,
        header_focused,
    );
    let active_scrollbar = state.scrollbar_drag.map(|d| d.which);
    explorer_view::render(
        frame,
        &state.theme,
        body.explorer,
        &state.explorer,
        explorer_focused,
        state.splitter_hover.explorer_splitter,
        state.splitter_hover.explorer_splitter_drag,
        active_scrollbar,
    );
    // Draw the resizable Explorer / workspace splitter strip.
    splitter_view::render(
        frame,
        &body,
        state.splitter_hover.app_splitter,
        state.splitter_hover.app_splitter_drag,
    );
    // The workspace region shows whichever workspace is active, driven by the
    // explorer tree's `active_workspace` marker (the original dbm's
    // `is_instance_workspace()`), not by keyboard focus. Opening a connection
    // sets the active marker to the connection, so the display switches from
    // the instance workspace to the SQL workspace. With no active workspace we
    // still show the SQL workspace (its empty-state hint) — the "connection
    // zone" the original dbm keeps visible after the last tab closes.
    let (editor_cursor, history_v_scroll) = if state.explorer.instances.active_is_instance() {
        iw_view::render(
            frame,
            &state.theme,
            workspace,
            &state.iw,
            workspace_focused,
            active_scrollbar,
        );
        (None, None)
    } else {
        sql_view::render(
            frame,
            &state.theme,
            workspace,
            &state.sql,
            workspace_focused,
            &state.splitter_hover,
            active_scrollbar,
        )
    };
    // The bottom row holds the global footer on the left and the performance
    // readout on the right.
    render_footer_with_perf(frame, state, chunks[2]);

    // The discover parent pane renders as a centered overlay over the workspace
    // region while it is focused. Its inline-edit caret is captured here and
    // takes precedence over the editor caret underneath.
    let mut discover_caret = None;
    let targets_layout_ref = std::cell::RefCell::new(None);
    let results_layout_ref = std::cell::RefCell::new(None);
    if let Pane::Discover(sub) = state.focus {
        let discover_caret_ref = std::cell::RefCell::new(None);
        crate::common::view::modal::render_modal_popup(
            frame,
            &state.theme,
            workspace,
            75,
            75,
            &state.discover,
            |f, t, a, s| {
                let c = discover_view::render(
                    f,
                    t,
                    a,
                    s,
                    sub,
                    state.splitter_hover.discover_splitter,
                    state.splitter_hover.discover_splitter_drag,
                    &targets_layout_ref,
                    &results_layout_ref,
                    active_scrollbar,
                );
                *discover_caret_ref.borrow_mut() = c;
            },
        );
        discover_caret = discover_caret_ref.into_inner();
    }

    let results_scroll_out = results_layout_ref.into_inner();

    // Render any active modal (data popup) as a centered overlay.
    if let Some(modal) = &state.modal {
        // Generic titled popup for the picker/confirm/commit-preview modals.
        render_popup_modal(frame, &state.theme, workspace, modal, state);
        // A modal overlays the workspace, so the editor caret is hidden.
        return (
            None,
            targets_layout_ref.into_inner(),
            history_v_scroll,
            results_scroll_out,
        );
    }
    // The discover overlay's inline-edit caret wins over the editor caret
    // underneath when discover is focused.
    (
        discover_caret.or(editor_cursor),
        targets_layout_ref.into_inner(),
        history_v_scroll,
        results_scroll_out,
    )
}

/// Render the footer row: the global footer hints on the left (flexible width)
/// and the performance readout pinned to the right. The perf portion is
/// right-aligned; when there is not enough room the perf strip is skipped.
fn render_footer_with_perf(frame: &mut ratatui::Frame, state: &AppState, area: Rect) {
    // Reserve a fixed right portion for the perf readout. The footer hints get
    // the remainder (which is what makes the footer "shorter" in practice).
    let perf_w = perf_view::perf_width(&state.perf).min(area.width / 3);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(1),         // footer hints
            Constraint::Length(perf_w), // perf readout
        ])
        .split(area);
    footer_view::render(frame, &state.theme, chunks[0], &state.footer);
    perf_view::render(frame, &state.theme, chunks[1], &state.perf);
}

/// Render a data-carrying modal. Confirm-style modals (delete connection /
/// unregister / commit preview) use the generic centered Yes/No confirm popup;
/// the results rows-per-page / page-input pickers instead float a small popup
/// just above their toolbar button, so they never cover the whole workspace.
fn render_popup_modal(
    frame: &mut ratatui::Frame,
    theme: &crate::common::view::theme::Theme,
    base: Rect,
    modal: &ModalKind,
    state: &AppState,
) {
    use crate::common::view::modal::{is_confirm_modal, modal_title, render_confirm_popup};
    if is_confirm_modal(modal) {
        let title = modal_title(modal);
        let body = match modal {
            ModalKind::DeleteConnectionConfirm { connection, .. } => {
                vec![Line::from(Span::raw(format!(
                    "Delete connection `{connection}`?"
                )))]
            }
            // The instance is already shown in the title, so the body only
            // states what unregistering does.
            ModalKind::UnregisterInstanceConfirm { .. } => {
                vec![Line::from(Span::raw(
                    "This will remove the instance and its connections.",
                ))]
            }
            ModalKind::ResultsEditCommitPreview { statements } => {
                let shown: Vec<Line> = statements
                    .iter()
                    .take(6)
                    .map(|s| Line::from(Span::raw(s.clone())))
                    .collect();
                if statements.len() > 6 {
                    let mut with_overflow = shown;
                    with_overflow.push(Line::from(Span::styled(
                        format!("… and {} more", statements.len() - 6),
                        Style::default().fg(theme.palette().muted),
                    )));
                    with_overflow
                } else {
                    shown
                }
            }
            ModalKind::ResultsRowLimitPicker { .. } | ModalKind::ResultsPageInput { .. } => {
                Vec::new() // unreachable: not a confirm modal
            }
        };
        render_confirm_popup(frame, theme, base, &title, body, true);
        return;
    }

    render_results_picker_popup(frame, theme, base, modal, state);
}

/// Render the rows-per-page / page-input pickers as a compact popup anchored
/// above their toolbar button (the `[r]rows` / `[p]page` controls). The popup
/// is sized to its content and clamps inside the results pane; nothing behind
/// it is dimmed or covered.
fn render_results_picker_popup(
    frame: &mut ratatui::Frame,
    theme: &crate::common::view::theme::Theme,
    base: Rect,
    modal: &ModalKind,
    state: &AppState,
) {
    use crate::common::view::modal::{modal_title, render_titled_popup};
    use crate::common::view::overlay_clear::clear_overlay;

    let body = match modal {
        ModalKind::ResultsRowLimitPicker { current, limits } => {
            let mut rows: Vec<Line> = limits
                .iter()
                .map(|l| {
                    let marker = if *l == *current { "◄" } else { " " };
                    Line::from(Span::raw(format!("{marker} {l} rows")))
                })
                .collect();
            rows.push(Line::from(Span::styled(
                "Select: ENTER · Move: j/k · Close: ESC",
                Style::default().fg(theme.palette().muted),
            )));
            rows
        }
        ModalKind::ResultsPageInput {
            current_page,
            total_pages,
            input,
        } => {
            let total = total_pages
                .map(|t| t.to_string())
                .unwrap_or_else(|| "?".to_string());
            vec![
                Line::from(Span::raw(format!(
                    "Current: page {current_page} of {total}"
                ))),
                Line::from(Span::styled(
                    format!("Go to: [{input}]"),
                    Style::default().fg(theme.palette().accent),
                )),
                Line::from(Span::styled(
                    "Go: ENTER · Close: ESC",
                    Style::default().fg(theme.palette().muted),
                )),
            ]
        }
        _ => return, // unreachable: confirm modals handled by the caller
    };

    // Same anchored geometry the mouse hit-testing uses (see
    // `app::geometry::results_picker_popup`).
    let Some(picker) = crate::app::geometry::results_picker_popup(state, base) else {
        return;
    };
    clear_overlay(frame, picker.popup);
    render_titled_popup(frame, theme, picker.popup, &modal_title(modal), body);
}

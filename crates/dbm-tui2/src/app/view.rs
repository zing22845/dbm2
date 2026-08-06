//! Top-level rendering. This module is purely presentational: it lays out the
//! shell regions and delegates to each feature's `view::render`. It contains no
//! message scheduling, intent routing or event-loop logic; those live in
//! `crate::app::loop_mod`.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::state::{AppState, ModalKind};
use crate::features::discover::view as discover_view;
use crate::features::explorer::view as explorer_view;
use crate::features::global_footer::view as footer_view;
use crate::features::header::view as header_view;
use crate::features::instance_workspace::view as iw_view;
use crate::features::perf_monitor::view as perf_view;
use crate::features::sql_workspace::view as sql_view;

/// Top-level render: lays out the shell regions and delegates to each
/// feature's `view::render`.
///
/// Takes `&mut AppState` so each view can record layout hit-boxes (e.g. the
/// header button rect) for mouse hit-testing, mirroring the original's
/// `ui_layout`.
pub fn render(frame: &mut ratatui::Frame, state: &mut AppState) {
    // The footer height is dynamic: one line of hints plus the (wrapped) status
    // line when present.
    let footer_h = footer_view::footer_height(&state.footer, frame.area().width);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header (title bar with borders)
            Constraint::Min(0),     // body (explorer + workspace)
            Constraint::Length(footer_h), // footer
        ])
        .split(frame.area());

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20), // explorer
            Constraint::Percentage(80), // workspace (iw + sql + perf)
        ])
        .split(chunks[1]);

    // The workspace region holds only the SQL view (main). The performance
    // readout moved into the footer row (right-aligned) to save vertical space.
    let workspace = body[1];

    header_view::render(frame, &state.theme, chunks[0], &mut state.header);
    explorer_view::render(frame, &state.theme, body[0], &state.explorer);
    // The main workspace region shows the instance workspace when an instance
    // is open, otherwise the SQL workspace. A future tab mechanism will make
    // this explicit.
    if state.iw.instance_name.is_empty() {
        sql_view::render(frame, &state.theme, workspace, &state.sql);
    } else {
        iw_view::render(frame, &state.theme, workspace, &state.iw);
    }
    // The bottom row holds the global footer on the left and the performance
    // readout on the right.
    render_footer_with_perf(frame, state, chunks[2]);

    // Render any active modal as a centered overlay over the workspace region.
    match &state.modal {
        Some(ModalKind::Discover) => {
            render_modal_popup(frame, &state.theme, workspace, &state.discover, |f, t, a, s| {
                discover_view::render(f, t, a, s)
            });
        }
        Some(modal) => {
            // Generic titled popup for the picker/confirm/commit-preview modals.
            render_popup_modal(frame, &state.theme, workspace, modal);
        }
        None => {}
    }
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
            Constraint::Min(1),   // footer hints
            Constraint::Length(perf_w), // perf readout
        ])
        .split(area);
    footer_view::render(frame, &state.theme, chunks[0], &state.footer);
    perf_view::render(frame, &state.theme, chunks[1], &state.perf);
}

/// Render a data-carrying modal (row-limit picker / page input / confirm /
/// commit-preview) as a centered, titled popup with a body summarizing it.
fn render_popup_modal(
    frame: &mut ratatui::Frame,
    theme: &crate::common::view::theme::Theme,
    base: Rect,
    modal: &ModalKind,
) {
    use crate::common::view::modal::{modal_title, render_popup, render_titled_popup};
    render_popup(frame, base, 45, 20, |f, area| {
        let body = match modal {
            ModalKind::ResultsRowLimitPicker { current, limits } => {
                
                limits
                    .iter()
                    .map(|l| {
                        let marker = if *l == *current { "◄" } else { " " };
                        Line::from(Span::raw(format!("{marker} {l} rows")))
                    })
                    .collect()
            }
            ModalKind::ResultsPageInput { current_page, total_pages } => {
                let total = total_pages
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "?".to_string());
                vec![Line::from(Span::raw(format!(
                    "Page {current_page} of {total} — type a page number"
                )))]
            }
            ModalKind::DeleteConnectionConfirm { instance, connection } => {
                vec![
                    Line::from(Span::raw(format!("Connection: {connection}"))),
                    Line::from(Span::raw(format!("Instance: {instance}"))),
                    Line::from(Span::raw("This will remove the stored connection.")),
                ]
            }
            ModalKind::UnregisterInstanceConfirm { instance } => {
                vec![
                    Line::from(Span::raw(format!("Instance: {instance}"))),
                    Line::from(Span::raw(
                        "This will remove the instance and its connections.",
                    )),
                ]
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
            ModalKind::Discover => Vec::new(),
        };
        render_titled_popup(f, theme, area, &modal_title(modal), body);
    });
}

/// Render a modal as a centered, bordered popup over `base`.
fn render_modal_popup<'a, S, F>(
    frame: &mut ratatui::Frame,
    theme: &crate::common::view::theme::Theme,
    base: Rect,
    state: &'a S,
    inner: F,
) where
    F: Fn(&mut ratatui::Frame, &crate::common::view::theme::Theme, Rect, &'a S),
{
    let w = (base.width * 3) / 4;
    let h = (base.height * 3) / 4;
    if w < 2 || h < 2 {
        return;
    }
    let popup = Rect {
        x: base.x + (base.width - w) / 2,
        y: base.y + (base.height - h) / 2,
        width: w,
        height: h,
    };
    inner(frame, theme, popup, state);
}

//! Top-level rendering. This module is purely presentational: it lays out the
//! shell regions and delegates to each feature's `view::render`. It contains no
//! message scheduling, intent routing or event-loop logic; those live in
//! `crate::app::loop_mod`.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

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
pub fn render(frame: &mut ratatui::Frame, state: &AppState) {
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

    // The workspace region holds the SQL view (main) plus a thin performance
    // readout strip at the bottom. A future tab-switching mechanism will swap
    // `iw` / `perf` here based on their visibility.
    let workspace = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0), // sql workspace
            Constraint::Length(3), // perf readout
        ])
        .split(body[1]);

    header_view::render(frame, &state.theme, chunks[0], &state.header);
    explorer_view::render(frame, &state.theme, body[0], &state.explorer);
    // The main workspace region shows the instance workspace when an instance
    // is open, otherwise the SQL workspace. A future tab mechanism will make
    // this explicit.
    if state.iw.instance_name.is_empty() {
        sql_view::render(frame, &state.theme, workspace[0], &state.sql);
    } else {
        iw_view::render(frame, &state.theme, workspace[0], &state.iw);
    }
    // perf_monitor adopts the theme-as-rendering-context convention (footer and
    // it are the migrated features with the `theme` parameter).
    perf_view::render(frame, &state.theme, workspace[1], &state.perf);
    footer_view::render(frame, &state.theme, chunks[2], &state.footer);

    // Render any active modal as a centered overlay.
    match state.modal {
        Some(ModalKind::Discover) => {
            render_modal_popup(frame, &state.theme, workspace[1], &state.discover, |f, t, a, s| {
                discover_view::render(f, t, a, s)
            });
        }
        None => {}
    }
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

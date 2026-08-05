//! Top-level rendering. This module is purely presentational: it lays out the
//! shell regions and delegates to each feature's `view::render`. It contains no
//! message scheduling, intent routing or event-loop logic; those live in
//! `crate::app::loop_mod`.

use ratatui::layout::{Constraint, Direction, Layout};

use crate::app::state::AppState;
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
            Constraint::Length(1),  // header
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
    explorer_view::render(frame, body[0], &state.explorer);
    sql_view::render(frame, workspace[0], &state.sql);
    // perf_monitor adopts the theme-as-rendering-context convention (footer and
    // it are the migrated features with the `theme` parameter).
    perf_view::render(frame, &state.theme, workspace[1], &state.perf);
    footer_view::render(frame, &state.theme, chunks[2], &state.footer);

    // The discover / iw features are wired into the message router but their
    // rendering is currently disabled (no dedicated screen region yet). These
    // placeholder references keep the render functions and states linked until
    // they are integrated into the workspace area or shown on demand.
    let _ = (&state.discover, &state.iw);
    let _ = (
        discover_view::render as fn(&mut ratatui::Frame, ratatui::layout::Rect, &crate::features::discover::state::DiscoverState),
        iw_view::render as fn(&mut ratatui::Frame, ratatui::layout::Rect, &crate::features::instance_workspace::state::IwState),
    );
}

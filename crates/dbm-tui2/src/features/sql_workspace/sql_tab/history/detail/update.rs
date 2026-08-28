//! History detail sub-feature update.
//!
//! Pure by-value transition over `DetailState`. All scroll operations need
//! the content SQL and viewport width to clamp correctly (wrapped line count
//! can't be computed from state alone). The caller passes these context
//! values; this module never touches list state.

use crate::common::components::search::PaneSearch;

use super::msg::DetailMessage;
use super::state::DetailState;
use super::view as detail_view;

/// Update the detail state. Pure by-value transition.
///
/// `text_width_after` and `viewport` are the current detail pane's content
/// width and visible row count — needed to compute wrap-based max scroll.
///
/// Returns `(new_state, dirty)`.
#[allow(clippy::too_many_arguments)]
pub fn update(
    msg: DetailMessage,
    mut state: DetailState,
    sql: &str,
    text_width_after: u16,
    viewport: usize,
) -> (DetailState, bool) {
    let dirty = match msg {
        DetailMessage::Scroll { delta } => {
            if sql.is_empty() {
                false
            } else {
                let before = state.scroll;
                if delta > 0 {
                    state.scroll = state.scroll.saturating_add(delta as usize);
                } else {
                    state.scroll = state.scroll.saturating_sub(delta.unsigned_abs() as usize);
                }
                detail_view::clamp_detail_scroll(&mut state, sql, text_width_after, viewport);
                state.scroll != before
            }
        }
        DetailMessage::ScrollPage { down } => {
            if sql.is_empty() {
                false
            } else {
                let before = state.scroll;
                detail_view::scroll_half_page(&mut state, sql, text_width_after, viewport, down);
                state.scroll != before
            }
        }
        DetailMessage::SetScroll { position } => {
            if sql.is_empty() {
                false
            } else {
                let before = state.scroll;
                state.scroll = position;
                detail_view::clamp_detail_scroll(&mut state, sql, text_width_after, viewport);
                state.scroll != before
            }
        }
        DetailMessage::PinSql { sql } => {
            state.pin(sql);
            true
        }
        DetailMessage::Reset => {
            state.reset();
            true
        }
    };
    (state, dirty)
}

/// On list selection change: reset scroll, or jump to the first matching
/// display row when filtered. Lives here so both parent `update` and view
/// layout_out can call it.
pub fn reconcile_on_selection_change(
    state: &mut DetailState,
    sql: &str,
    search: &PaneSearch,
    text_width_after: u16,
    viewport_lines: usize,
) {
    if search.has_filter()
        && let Some(row) = detail_view::first_match_display_line(sql, &search.query, search.options, text_width_after)
    {
        let max_scroll =
            detail_view::detail_display_line_count_at(text_width_after, sql).saturating_sub(viewport_lines.max(1));
        state.scroll = row.min(max_scroll);
        return;
    }
    state.scroll = 0;
}

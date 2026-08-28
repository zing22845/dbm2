//! History feature rendering.
//!
//! The History feature owns two child panes (list + detail) and a splitter.
//! This module orchestrates: it draws the single shared History border +
//! title, splits the inner area into `[detail | splitter | list]` (when the
//! detail is visible) or lets the list fill the whole pane (when it is not),
//! and delegates sub-pane rendering to the `detail` and `list` child features.
//! The list returns a reconciled viewport start row (discover-style layout_out
//! pattern) that the caller feeds back to state.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::common::components::search::pane_search_title_line;
use crate::common::view::hints::{draw_footer, footer_height, history_list_footer_text};
use crate::common::view::theme::Theme;

use super::detail::view::draw_history_detail;
use super::list::view as list_view;
use super::splitter::state::clamp_detail_pane_width;
use super::splitter::view as splitter_view;
use super::state::HistoryState;
use super::store::SqlHistoryStore;

/// Render the History pane. The single History border + title wraps the whole
/// `area` (both the list and, when `detail_visible`, the detail preview).
#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &HistoryState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    focused: bool,
    detail_visible: bool,
    detail_w: u16,
    splitter_hover: bool,
    splitter_drag: bool,
) -> Option<usize> {
    let search_active = state.list.search.text_input_active();
    let list_footer = history_list_footer_text(search_active, state.list.search.has_filter(), true);

    let p = theme.palette();
    let entries = store.entries(instance, connection);
    let visible = state.list.visible_indices(store, instance, connection);
    let cursor = state.list.cursor.min(visible.len().saturating_sub(1));

    let detail_w = if detail_visible {
        clamp_detail_pane_width(detail_w)
    } else {
        0
    };
    let list_w = if detail_visible {
        area.width.saturating_sub(detail_w).saturating_sub(1) // detail + splitter
    } else {
        area.width
    };

    let title = pane_search_title_line(
        " [H] History",
        &state.list.search,
        true,
        false,
        Style::default().fg(p.muted),
        cursor,
        visible.len(),
        Some(area.width.saturating_sub(6)),
        None,
        Some(Style::default().fg(if focused { p.accent } else { p.muted })),
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let (list_area, list_footer_area) = if detail_visible {
        // [detail | splitter | list] inside the shared border.
        let detail_w = clamp_detail_pane_width(detail_w).min(inner.width.saturating_sub(2));
        let body_h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(detail_w),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);
        tracing::debug!("history: split done inner={inner:?} detail={:?} list={:?}", body_h[0], body_h[2]);
        // Detail preview of the selected / pinned / first statement.
        if let Some(sql) = state
            .list.selected_entry(store, instance, connection)
            .or_else(|| state.detail.pinned_sql.clone())
            .or_else(|| store.entries(instance, connection).first().cloned())
        {
            let mut content = Rect::default();
            let mut v_bar = Rect::default();
            draw_history_detail(
                frame,
                body_h[0],
                &sql,
                &state.detail,
                &state.list.search,
                theme,
                &mut content,
                &mut v_bar,
            );
        }
        tracing::debug!("history: detail drawn");
        splitter_view::render(frame, body_h[1], splitter_hover, splitter_drag);
        // Each sub-pane's footer is inside the shared border, wrapped to its
        // own column width.
        let footer_h = footer_height(&list_footer, list_w.saturating_sub(2))
            .min(inner.height.saturating_sub(3));
        let list_col = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(footer_h)])
            .split(body_h[2]);
        (list_col[0], list_col[1])
    } else {
        let footer_h = footer_height(&list_footer, inner.width.saturating_sub(2))
            .min(inner.height.saturating_sub(3));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(footer_h)])
            .split(inner);
        (chunks[0], chunks[1])
    };

    let v_scroll_out = list_view::render(
        frame, theme, list_area, &state.list, entries, &visible, cursor, focused,
    );

    // The list footer hints (inside the shared border).
    draw_footer(frame, theme, list_footer_area, &list_footer);

    v_scroll_out
}

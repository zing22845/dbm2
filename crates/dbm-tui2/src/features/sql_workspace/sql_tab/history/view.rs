//! History feature rendering.
//!
//! The History feature owns two sub-panes: the **list** (searchable, applies a
//! statement) and the **detail** (preview of the selected statement). Mirrors
//! the original dbm: a single History border + title wraps BOTH sub-panes; when
//! the detail is visible the inner area is split into `[detail | splitter |
//! list]`, otherwise the list fills the whole pane. The list and detail do not
//! draw their own outer borders — the History pane provides it.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::components::search::pane_search_title_line;
use crate::common::view::hints::{draw_footer, footer_height, history_list_footer_text};
use crate::common::view::pane_scrollbar::{draw_vertical_pane_scrollbar, pane_scroll_layout};
use crate::common::view::splitter::{draw as draw_splitter, SplitOrientation};
use crate::common::view::theme::Theme;

use super::detail::{clamp_detail_pane_width, draw_history_detail};
use super::state::HistoryState;
use super::store::history_one_line;

/// Render the History pane. The single History border + title wraps the whole
/// `area` (both the list and, when `detail_visible`, the detail preview).
/// `detail_visible` is computed by the caller from the original dbm's
/// `detail_visible` condition (`workspace_pane == History` + has an entry);
/// `detail_w` is the width of the detail pane (clamped to its allowed range).
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &HistoryState,
    instance: &str,
    connection: &str,
    focused: bool,
    detail_visible: bool,
    detail_w: u16,
) {
    let search_active = state.search.text_input_active();
    let list_footer = history_list_footer_text(search_active, state.search.has_filter(), true);

    let p = theme.palette();
    let entries = state.store.entries(instance, connection);
    let visible = state.visible_indices_for(entries);
    let cursor = state.cursor.min(visible.len().saturating_sub(1));

    // The footer width depends on which sub-pane it belongs to: when the detail
    // is visible each sub-pane carries its own footer inside the shared border.
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
        &state.search,
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
        return;
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
        tracing::debug!("history: split done inner={:?} detail={:?} list={:?}", inner, body_h[0], body_h[2]);
        // Detail preview of the selected / pinned / first statement.
        if let Some(sql) = state
            .selected_sql(instance, connection)
            .or_else(|| state.detail.pinned_sql.clone())
            .or_else(|| state.store.entries(instance, connection).first().cloned())
        {
            let mut content = Rect::default();
            let mut v_bar = Rect::default();
            draw_history_detail(
                frame,
                body_h[0],
                &sql,
                &state.detail,
                &state.search,
                theme,
                &mut content,
                &mut v_bar,
            );
        }
        tracing::debug!("history: detail drawn");
        draw_splitter(
            frame,
            body_h[1],
            SplitOrientation::Vertical,
            false,
            false,
        );
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

    render_list_rows(frame, theme, list_area, state, &entries, &visible, cursor, focused);

    // The list footer hints (inside the shared border).
    draw_footer(frame, theme, list_footer_area, &list_footer);
}

/// Render the list rows (and its scrollbar) into `area`. Used inside the shared
/// History border; the list does not draw its own border.
fn render_list_rows(
    frame: &mut Frame,
    theme: &Theme,
    list_area: Rect,
    state: &HistoryState,
    entries: &[String],
    visible: &[usize],
    cursor: usize,
    _focused: bool,
) {
    let p = theme.palette();
    if visible.is_empty() {
        let hint = if state.search.has_filter() {
            "No matching history"
        } else {
            "No history yet"
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                hint,
                Style::default().fg(p.muted),
            ))),
            list_area,
        );
        return;
    }
    let viewport_rows = list_area.height as usize;
    let layout = pane_scroll_layout(list_area, list_area.width, visible.len(), viewport_rows);
    let content = layout.content_area;
    let viewport = content.height.max(1) as usize;
    let start = cursor.saturating_sub(viewport / 2);
    let end = (start + viewport).min(visible.len());
    let start = end.saturating_sub(viewport);

    let lines: Vec<Line> = visible[start..end]
        .iter()
        .enumerate()
        .map(|(row, &idx)| {
            let sql = &entries[idx];
            let selected = start + row == cursor;
            let prefix = if selected { "▸ " } else { "  " };
            let text = format!("{prefix}{}", history_one_line(sql));
            let style = if selected {
                Style::default()
                    .fg(p.fg)
                    .bg(p.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(p.fg)
            };
            Line::from(Span::styled(text, style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), content);

    if let Some(bar) = layout.v_scrollbar {
        let max_scroll = visible.len().saturating_sub(viewport);
        draw_vertical_pane_scrollbar(
            frame,
            bar,
            start,
            viewport,
            max_scroll,
            p,
            false,
        );
    }
}

impl HistoryState {
    /// Indices into `entries` matching the current search (all when no filter).
    fn visible_indices_for(&self, entries: &[String]) -> Vec<usize> {
        self.search.matching_indices(entries)
    }

    /// The SQL text selected by the history cursor, if any.
    fn selected_sql(&self, instance: &str, connection: &str) -> Option<String> {
        let entries = self.store.entries(instance, connection);
        let visible = self.visible_indices_for(entries);
        let &idx = visible.get(self.cursor)?;
        entries.get(idx).cloned()
    }
}

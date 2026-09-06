//! Footer/hint text and its drawing.
//!
//! The pure text builders now live in `crate::common::layout::hints` (layout
//! measures footer rows from them) and are re-exported here so renderers can
//! keep importing `common::view::hints::*`; this module itself only draws.
//!

use super::theme::Theme;

pub use crate::common::layout::hints::*;

/// Draw a footer hint string into `area`, wrapping to the area width when it is
/// too narrow. Pure `state -> view`: reads only the theme and text. Callers
/// should size `area` with [`footer_height`] so wrapped lines have room.
pub fn draw_footer(
    frame: &mut ratatui::Frame,
    theme: &Theme,
    area: ratatui::layout::Rect,
    text: &str,
) {
    if text.is_empty() || area.height == 0 || area.width == 0 {
        return;
    }
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;
    let p = theme.palette();
    let style = ratatui::style::Style::default().fg(p.muted);
    let lines: Vec<Line> = text
        .split('\n')
        .map(|l| Line::from(Span::styled(l.to_string(), style)))
        .collect();
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        area,
    );
}

/// Draw a pane's footer hint line into `area` (already the bottom strip of the
/// pane's inner rect). Pure `state -> view`: reads only the theme and text.
pub fn draw_pane_footer(
    frame: &mut ratatui::Frame,
    theme: &Theme,
    area: ratatui::layout::Rect,
    text: &str,
) {
    draw_footer(frame, theme, area, text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::layout::hints::key;
    use crate::common::layout::text::footer_height;
    use crate::common::utils::shortcuts::hint_ctrl;

    #[test]
    fn key_and_join_format() {
        assert_eq!(key("Run", "ALT+ENTER"), "Run: ALT+ENTER");
        assert_eq!(join(&["a: x", "b: y"]), "a: x  b: y");
    }

    /// Ground truth: render `text` and count the rows that received content.
    fn rendered_rows(text: &str, cols: u16) -> usize {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        use ratatui::widgets::{Paragraph, Widget, Wrap};

        let area = Rect::new(0, 0, cols, 16);
        let mut buf = Buffer::empty(area);
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .render(area, &mut buf);
        let mut rows = 0usize;
        for y in 0..area.height {
            if (0..area.width).any(|x| buf[(x, y)].symbol() != " ") {
                rows = y as usize + 1;
            }
        }
        rows
    }

    #[test]
    fn explorer_footer_height_never_clips_the_hint() {
        // Regression: the explorer panes reserve `footer_height` rows for their
        // hint strip. Estimating rows as `width / cols` under-counted because the
        // renderer wraps at *word* boundaries, so on a narrow explorer the last
        // hint was clipped away (e.g. "Collapse: h" vanishing right after
        // "Expand: l"). Both explorer panes share this helper, so both are
        // covered here.
        let texts = [
            instances_pane_footer_text(true),
            instances_pane_footer_text(false),
            objects_pane_footer_text(),
        ];
        for text in texts {
            for cols in [80u16, 60, 45, 40, 35, 30, 25, 20, 15, 12] {
                let reserved = footer_height(&text, cols) as usize;
                let rows = rendered_rows(&text, cols);
                assert!(
                    reserved >= rows,
                    "footer_height {reserved} clips {rows} rendered rows at {cols} cols: {text:?}"
                );
            }
        }
    }

    #[test]
    fn pane_search_active_footer_lists_chords() {
        let footer = pane_search_active_footer(&[]);
        assert!(footer.contains(&format!("Prev: {}", hint_ctrl("p"))));
        assert!(footer.contains("Apply: ENTER"));
        assert!(footer.contains("Case: "));
    }

    #[test]
    fn sql_workspace_empty_hint_points_to_connection_selection() {
        let hint = sql_workspace_empty_hint(false);
        assert!(hint.contains("Select a connection"));
        assert!(hint.contains("open a workspace"));
    }

    #[test]
    fn sql_workspace_empty_hint_with_active_connection_points_to_query_tabs() {
        let hint = sql_workspace_empty_hint(true);
        assert!(hint.contains("No query tabs for this connection"));
    }

    #[test]
    fn sql_pane_insert_footer_differs_from_normal() {
        let insert = sql_pane_footer_text(false, false, "insert", false, true);
        assert!(insert.contains("Complete: SHIFT+TAB"));
        assert!(insert.contains("Tbl complete: ON (ALT+TAB)"));
        assert!(insert.contains("Normal: ESC"));
        let normal = sql_pane_footer_text(false, false, "normal", false, true);
        assert!(!normal.contains("Complete:"));
        assert!(normal.contains("Insert: i"));
        assert!(normal.contains("Visual: v"));
    }

    #[test]
    fn sql_pane_footer_tbl_complete_reflects_flag() {
        // The TblCmp toggle shows the live ON/OFF state plus its Alt+Tab
        // shortcut in the insert-mode footer.
        let on = sql_pane_footer_text(false, false, "insert", false, true);
        assert!(on.contains("Tbl complete: ON (ALT+TAB)"));
        let off = sql_pane_footer_text(false, false, "insert", false, false);
        assert!(off.contains("Tbl complete: OFF (ALT+TAB)"));
        // Non-insert modes never advertise the toggle.
        let normal = sql_pane_footer_text(false, false, "normal", false, true);
        assert!(!normal.contains("Tbl complete:"));
    }

    #[test]
    fn sql_pane_footer_excludes_workspace_level_tab_hints() {
        // Tab management belongs to the SQL workspace, not to a single editor
        // pane, so the editor footer must NOT advertise it.
        let normal = sql_pane_footer_text(false, false, "normal", false, true);
        assert!(!normal.contains("Close tab:"));
        assert!(!normal.contains("Switch tab:"));
        let insert = sql_pane_footer_text(false, false, "insert", false, true);
        assert!(!insert.contains("Close tab:"));
        assert!(!insert.contains("Switch tab:"));
    }

    #[test]
    fn sql_workspace_footer_advertises_tab_management() {
        let footer = sql_workspace_footer_text();
        assert!(footer.contains("TAB Ops ["));
        assert!(footer.contains("New: Alt+t"));
        assert!(footer.contains("Close: Ctrl+w"));
        assert!(footer.contains("Switch: Alt+1-9"));
        assert!(footer.contains("Next: Alt+n"));
        assert!(footer.contains("Prev: Alt+p"));
        // `Ctrl+T` is reserved for theme toggling, so open-tab must not be shown.
        assert!(!footer.contains("Open tab"));
        assert!(!footer.contains("Open:"));
    }

    #[test]
    fn sql_pane_normal_appends_search_jump_when_filtered() {
        let filtered = sql_pane_footer_text(false, false, "normal", true, false);
        assert!(filtered.contains("Next match: n/N"));
        assert!(filtered.contains("Clear filter: ESC"));
        let unfiltered = sql_pane_footer_text(false, false, "normal", false, false);
        assert!(!unfiltered.contains("Next match:"));
    }

    #[test]
    fn sql_pane_search_active_shows_search_footer() {
        let footer = sql_pane_footer_text(false, true, "normal", false, false);
        assert!(footer.contains("Prev: "));
        assert!(!footer.contains("Insert: i"));
    }

    #[test]
    fn results_pane_detail_open_esc_label_changes() {
        // The detail pane's own footer advertises "Back: ESC"; the list footer
        // intentionally drops the duplicated close / deselect / toolbar hints.
        let open = results_pane_footer_text(false, "");
        assert!(!open.contains("Close detail: ESC"));
        assert!(!open.contains("Deselect: ESC"));
        assert!(!open.contains("Toolbar: click"));
        let closed = results_pane_footer_text(false, "");
        assert!(!closed.contains("Deselect: ESC"));
        assert!(closed.contains("Col width: ,/."));
        let open_status = results_pane_footer_text(false, "updated");
        assert!(open_status.contains("\nupdated"));
    }

    #[test]
    fn history_list_focus_return_adds_hint() {
        let base = history_list_footer_text(false, false, false);
        assert!(!base.contains("Back to SQL"));
        let ret = history_list_footer_text(false, false, true);
        assert!(ret.contains("Back to SQL: ESC"));
    }

    #[test]
    fn explorer_instance_footer_varies_by_row_kind() {
        let inst = instances_pane_footer_text(true);
        assert!(inst.contains("Expand: l"));
        assert!(inst.contains("Collapse: h"));
        assert!(inst.contains("Add conn: a"));
        assert!(inst.contains("Open: ENTER / Dbl-click"));
        // Connection row shows New/Edit instead of Expand/Collapse.
        let conn = instances_pane_footer_text(false);
        assert!(conn.contains("New: n"));
        assert!(conn.contains("Edit conn: i"));
        assert!(!conn.contains("Expand:"));
    }

    #[test]
    fn explorer_objects_footer_lists_open_expand_collapse_refresh() {
        let footer = objects_pane_footer_text();
        assert!(footer.contains("Open schema: ENTER"));
        assert!(footer.contains("Expand: l"));
        assert!(footer.contains("Collapse: h"));
        assert!(footer.contains("Refresh: r"));
    }

    #[test]
    fn instance_workspace_footer_varies_by_subpane() {
        use crate::app_shell::nav::IwPane;
        let ov = instance_workspace_footer_text(IwPane::Overview);
        assert!(ov.contains("Refresh: r"));
        assert!(ov.contains("Unregister: u"));
        let conn = instance_workspace_footer_text(IwPane::Connections);
        assert!(conn.contains("Add: a"));
        assert!(conn.contains("Edit: i"));
        assert!(conn.contains("Delete: d"));
        assert!(conn.contains("Test: t"));
    }

    #[test]
    fn modal_footer_with_no_modal_is_empty() {
        assert_eq!(modal_footer_text(None), "");
    }

    #[test]
    fn discover_footers_switch_on_state() {
        // Footer lists pane nav + scan/close; a status appends a line.
        let footer = discover_footer_text("");
        assert!(footer.contains("Pane: "));
        assert!(footer.contains("Scan: s"));
        assert!(footer.contains("Close: ESC"));
        assert!(!footer.contains('\n'));
        let with_status = discover_footer_text("scanning…");
        assert!(with_status.contains("\nscanning…"));
        // Targets footer shows edit keys when not editing, commit/cancel when editing.
        let edit = discover_targets_footer_text(false, false, None);
        assert!(edit.contains("Edit: i/ENTER"));
        assert!(edit.contains("Insert: o"));
        assert!(edit.contains("Delete: d"));
        assert!(edit.contains("Paste TSV/host:ports"));
        // A loopback row appends the local-discovery note on a second line.
        let loopback = discover_targets_footer_text(false, true, None);
        assert!(loopback.contains(DISCOVER_LOOPBACK_SCAN_NOTE));
        assert!(loopback.contains('\n'));
        // A status line is appended on a second line after the hints.
        let with_status = discover_targets_footer_text(false, false, Some("Paste: 1/2 added"));
        assert!(with_status.ends_with("Paste: 1/2 added"));
        assert!(with_status.contains('\n'));
        let committing = discover_targets_footer_text(true, false, Some("Paste: 1/2 added"));
        assert!(committing.contains("Commit: ENTER"));
        assert!(committing.contains("Cancel: ESC"));
        assert!(!committing.contains("Paste: 1/2 added"));
        // Results footer lists selection / register / force-register / filter,
        // plus the mark legend on a second line.
        let results = discover_results_footer_text();
        assert!(results.contains("Select: SPACE"));
        assert!(results.contains("Register: r"));
        assert!(results.contains("Force: R"));
        assert!(results.contains("Filter: u"));
        assert!(results.contains('\n'));
        assert!(results.ends_with(RESULTS_LEGEND));
    }

    #[test]
    fn global_footer_includes_quit_and_status_on_second_line() {
        let base = global_footer_text("");
        assert!(base.contains("Quit: "));
        let with_status = global_footer_text("ready");
        let lines: Vec<_> = with_status.lines().collect();
        assert_eq!(lines.last().copied(), Some("ready"));
    }
}

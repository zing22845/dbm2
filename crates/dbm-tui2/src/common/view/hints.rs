//! Shared footer/hint text builders.
//!
//! The hint strings shown in the global footer and each pane's status line are
//! assembled here so they are unit-testable in isolation. The pure helpers
//! (`key`, `join`, `keys`, `pane_search_active_footer`) are feature-free; the
//! per-pane builders take the owning feature's state so rendering stays
//! presentational.

use crate::app::state::ModalKind;
use crate::common::components::search::PaneSearch;
use crate::common::layout::hints::{join, keys, lit};
use crate::common::utils::shortcuts::{
    copy_shortcut_label, hint_ctrl, paste_shortcut_label, quit_shortcut_label,
};
use crate::common::view::theme::Theme;

/// Empty-state hint shown in the SQL workspace when the active connection has
/// no open query tab, mirroring the original dbm's `workspace_empty_hint`.
/// `has_connection` distinguishes "a connection is active but has no tabs"
/// (show the connection-specific hint) from "no connection selected at all".
pub fn sql_workspace_empty_hint(has_connection: bool) -> &'static str {
    if has_connection {
        "No query tabs for this connection — press n or ENTER in the tree."
    } else {
        "Select a connection in the tree — ENTER or double-click to open a workspace."
    }
}

/// Workspace-level footer hint for SQL tab management (belongs to the SQL
/// workspace, not to a single editor pane). (`Ctrl+T` is reserved for theme
/// toggling, so there is no open-tab shortcut.)
pub fn sql_workspace_footer_text() -> String {
    format!(
        "TAB Ops [{}]",
        [
            "New: Alt+t",
            "Close: Ctrl+w",
            "Switch: Alt+1-9",
            "Next: Alt+n",
            "Prev: Alt+p",
        ]
        .join("  ")
    )
}

/// The footer shown while a pane `/` search input is active.
pub fn pane_search_active_footer(extra: &[(&str, String)]) -> String {
    let mut hints = keys(&[
        ("Prev", hint_ctrl("p")),
        ("Next", hint_ctrl("n")),
        ("Apply", lit("ENTER")),
        ("Cancel", lit("ESC")),
        ("Clear", hint_ctrl("u")),
        ("Case", hint_ctrl("/")),
    ]);
    if !extra.is_empty() {
        hints = join(&[&hints, &keys(extra)]);
    }
    hints
}

/// Footer for the results detail pane (back to the table).
pub fn results_detail_footer_text() -> String {
    keys(&[("Back", lit("ESC"))])
}

/// Footer for the SQL editor pane, adapted to a single tab's editor state.
pub fn sql_pane_footer_text(
    search_active: bool,
    sql_search_active: bool,
    editor_mode: &str,
    sql_search_has_filter: bool,
    complete_table_names: bool,
) -> String {
    if search_active || sql_search_active {
        return pane_search_active_footer(&[]);
    }
    match editor_mode {
        "insert" => keys(&[
            ("History", hint_ctrl("r")),
            (
                "Tbl complete",
                format!(
                    "{} ({})",
                    if complete_table_names { "ON" } else { "OFF" },
                    "ALT+TAB"
                ),
            ),
            ("Complete", "SHIFT+TAB".into()),
            ("Context", lit("click title")),
            ("Normal", lit("ESC")),
            ("Run", "ALT+ENTER".into()),
        ]),
        "visual" => keys(&[("Context", lit(", / click")), ("Normal", lit("ESC"))]),
        _ => {
            let mut hints = keys(&[
                ("History", hint_ctrl("r")),
                ("Context", lit(", / click")),
                ("Insert", lit("i")),
                ("Visual", lit("v")),
                ("Run", "ALT+ENTER".into()),
            ]);
            if sql_search_has_filter {
                hints = join(&[
                    &hints,
                    &keys(&[("Next match", lit("n/N")), ("Clear filter", lit("ESC"))]),
                ]);
            }
            hints
        }
    }
}

/// Footer for the results pane: search-active vs. table hints.
///
/// The detail pane's own footer already advertises "Back: ESC" and the global
/// footer exposes the ["/"] pane-width hint, so neither the close-ESC nor the
/// detail-width splitter is repeated here.
pub fn results_pane_footer_text(search_active: bool, status: &str) -> String {
    if search_active {
        return pane_search_active_footer(&[]);
    }
    let base = keys(&[
        ("Inspect", lit("ENTER")),
        ("Col width", lit(",/.")),
        ("Copy Col Name", hint_ctrl("n")),
        ("Flip", lit("f/b")),
        ("Top", lit("g")),
        ("Bottom", lit("G")),
    ]);
    if status.is_empty() {
        base
    } else {
        format!("{base}\n{status}")
    }
}

/// Footer for the history list pane.
pub fn history_list_footer_text(
    search_active: bool,
    search_has_filter: bool,
    focus_return: bool,
) -> String {
    if search_active {
        return pane_search_active_footer(&[]);
    }
    let mut hints = keys(&[
        ("Move", hint_ctrl("p/n")),
        ("Apply", lit("ENTER / Dbl-click")),
        ("Jump", lit("g/G")),
    ]);
    if search_has_filter {
        hints = join(&[&hints, &keys(&[("Clear filter", lit("ESC"))])]);
    }
    if focus_return {
        hints = join(&[&hints, &keys(&[("Back to SQL", lit("ESC"))])]);
    }
    hints
}

pub use crate::common::layout::hints::{discover_engine_footer_text, discover_footer_text};

/// Footer for the discover targets editor pane, switching on edit state.
/// `has_loopback` appends a note that loopback also runs local discovery, and
/// `status` (the last paste/undo/redo feedback, e.g. "Paste: 1/3 added …") is
/// appended on a second line when non-empty.
pub fn discover_targets_footer_text(
    editing: bool,
    has_loopback: bool,
    status: Option<&str>,
) -> String {
    if editing {
        return keys(&[("Commit", lit("ENTER")), ("Cancel", lit("ESC"))]);
    }
    let mut footer = keys(&[
        ("Edit", lit("i/ENTER")),
        ("Insert", lit("o")),
        ("Delete", lit("d")),
        ("Paste TSV/host:ports", paste_shortcut_label()),
        ("Undo", lit("u")),
        ("Redo", hint_ctrl("r")),
    ]);
    if has_loopback {
        footer.push('\n');
        footer.push_str(DISCOVER_LOOPBACK_SCAN_NOTE);
    }
    if let Some(status) = status.filter(|s| !s.is_empty()) {
        footer.push('\n');
        footer.push_str(status);
    }
    footer
}

/// Note shown under Targets when any row is loopback — Ports only constrain TCP
/// probes, not local discovery.
pub const DISCOVER_LOOPBACK_SCAN_NOTE: &str =
    "Loopback also runs local discovery (process/pid/socket); Ports only limit TCP probes.";

/// Row-mark legend shown under the results keys: `✓` marks a selected
/// (unregistered) instance, `×` marks one that is already registered.
pub const RESULTS_LEGEND: &str = "mark: ✓ selected · × registered";

/// Footer for the discover results list pane: a keys line plus a legend line
/// explaining the row marks (`✓` selected, `×` already registered).
pub fn discover_results_footer_text() -> String {
    format!(
        "{}\n{}",
        keys(&[
            ("Select", lit("SPACE")),
            ("Register", lit("r")),
            ("Force", lit("R")),
            ("Filter", lit("u")),
        ]),
        RESULTS_LEGEND
    )
}

/// Footer hints for the data-carrying modals; Discover draws its own footer
/// and returns empty.
pub fn modal_footer_text(modal: Option<&ModalKind>) -> String {
    crate::common::view::modal::modal_footer_text(modal)
}

/// Global footer hints (pane navigation + shortcuts).
///
/// Kept intentionally short (the perf readout occupies the footer's right side)
/// so only the essential pane navigation and search are shown. This is the
/// single source of truth for the global footer; `global_footer::view` renders
/// it instead of a hardcoded list.
pub fn global_footer_text(global_status: &str) -> String {
    let hints = keys(&[
        ("Pane", lit("TAB")),
        ("SubPane", hint_ctrl("h/j/k/l")),
        ("Search", lit("/")),
        ("Width", lit("[/]")),
        ("Height", lit("+/-")),
        ("Resize", lit("drag")),
        ("H-Scroll", lit("←/→")),
        ("Copy", copy_shortcut_label()),
        ("Quit", quit_shortcut_label()),
    ]);
    if global_status.is_empty() {
        hints
    } else {
        format!("{hints}\n{global_status}")
    }
}

/// Footer for the explorer instances tree, mirroring the original dbm. The
/// hints differ by row kind: an instance row shows Open/Add/Expand/Collapse,
/// a connection row shows New/Open/Add/Edit.
pub fn instances_pane_footer_text(instance_row: bool) -> String {
    if instance_row {
        keys(&[
            ("Open", lit("ENTER / Dbl-click")),
            ("Add conn", lit("a")),
            ("Expand", lit("l")),
            ("Collapse", lit("h")),
        ])
    } else {
        keys(&[
            ("New", lit("n")),
            ("Open", lit("ENTER / Dbl-click")),
            ("Add conn", lit("a")),
            ("Edit conn", lit("i")),
        ])
    }
}

/// Footer for the explorer objects tree, mirroring the original dbm.
pub fn objects_pane_footer_text() -> String {
    keys(&[
        ("Open schema", lit("ENTER")),
        ("Expand", lit("l")),
        ("Collapse", lit("h")),
        ("Refresh", lit("r")),
    ])
}

/// Footer for the instance workspace sub-pane, mirroring the original dbm:
/// the Overview pane shows Refresh/Unregister; the Connections pane shows
/// Add/Edit/Delete/Test.
pub fn instance_workspace_footer_text(pane: crate::app_shell::nav::IwPane) -> String {
    match pane {
        crate::app_shell::nav::IwPane::Overview => {
            keys(&[("Refresh", lit("r")), ("Unregister", lit("u"))])
        }
        crate::app_shell::nav::IwPane::Connections => keys(&[
            ("Add", lit("a")),
            ("Edit", lit("i")),
            ("Delete", lit("d")),
            ("Test", lit("t")),
        ]),
    }
}

/// Convenience: turn a `PaneSearch` into the search-active footer, or empty.
pub fn pane_search_footer_if_active(search: &PaneSearch) -> Option<String> {
    if search.text_input_active() {
        Some(pane_search_active_footer(&[]))
    } else {
        None
    }
}

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
